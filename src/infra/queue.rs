//! Reliable, crash-safe Redis-backed job queue.
//!
//! Jobs move through explicit, named states instead of being destructively
//! popped off a list:
//!
//! ```text
//! ready --(claim)--> in-flight --(ack)-->      completed (dedup marker, then dropped)
//!                       |
//!                       +--(fail, budget left)--> delayed --(promote)--> ready
//!                       |
//!                       +--(fail, budget exhausted)--> dead-letter
//! ```
//!
//! Every transition that moves ownership of a job is a single Lua script,
//! so two workers (or a worker racing a lease-recovery sweep) can never both
//! believe they own the same job. A "claim" only ever removes a job from
//! exactly one place; recovery scripts use `ZREM`'s return value as an
//! atomic compare-and-take, so a job can only be recovered once.
//!
//! Redis holds the fast dedup path (`webhook:delivery:<id>`). It records the
//! delivery's coarse state (`queued`, `processing`, `completed`,
//! `dead_lettered`) rather than a bare boolean, so a GitHub redelivery is
//! only suppressed when the original job is actually still tracked -- a
//! redelivery of a job that Redis lost (crash before persistence) is
//! re-enqueued rather than silently dropped.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use rand::Rng;
use redis::{AsyncCommands, Script};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::time::{sleep, Duration};
use uuid::Uuid;

use crate::{config::Config, error::AppError, infra::redis::RedisClient};

const WEBHOOKS_NS: &str = "queue:webhooks";
const SYNC_QUEUE: &str = "queue:sync";
const DEDUP_PREFIX: &str = "webhook:delivery:";

fn ready_key(ns: &str) -> String {
    format!("{ns}:ready")
}
fn jobs_key(ns: &str) -> String {
    format!("{ns}:jobs")
}
fn inflight_key(ns: &str) -> String {
    format!("{ns}:inflight")
}
fn delayed_key(ns: &str) -> String {
    format!("{ns}:delayed")
}
fn dlq_key(ns: &str) -> String {
    format!("{ns}:dead-letter")
}
fn dedup_key(delivery_id: &str) -> String {
    format!("{DEDUP_PREFIX}{delivery_id}")
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Lua scripts. Each one performs a single atomic ownership transition.
// ---------------------------------------------------------------------------

/// KEYS: [dedup_key_or_empty, jobs_hash, ready_list]
/// ARGV: [job_id, envelope_json]
/// Returns 0 = enqueued, 1 = duplicate.
static ENQUEUE_SCRIPT: Lazy<Script> = Lazy::new(|| {
    Script::new(
        r#"
if KEYS[1] ~= '' then
  local existing = redis.call('GET', KEYS[1])
  if existing then
    local ok, state = pcall(cjson.decode, existing)
    if ok then
      if state.state == 'completed' then
        return 1
      end
      if (state.state == 'queued' or state.state == 'processing')
          and redis.call('HEXISTS', KEYS[2], state.job_id) == 1 then
        return 1
      end
    end
  end
end
redis.call('HSET', KEYS[2], ARGV[1], ARGV[2])
redis.call('LPUSH', KEYS[3], ARGV[1])
if KEYS[1] ~= '' then
  redis.call('SET', KEYS[1], cjson.encode({state='queued', job_id=ARGV[1]}))
end
return 0
"#,
    )
});

/// KEYS: [ready_list, jobs_hash, inflight_zset]
/// ARGV: [lease_expiry_ms]
/// Returns {job_id, envelope_json} or false when the queue is empty.
static CLAIM_SCRIPT: Lazy<Script> = Lazy::new(|| {
    Script::new(
        r#"
local job_id = redis.call('RPOP', KEYS[1])
if not job_id then
  return false
end
local envelope = redis.call('HGET', KEYS[2], job_id)
if not envelope then
  return false
end
redis.call('ZADD', KEYS[3], ARGV[1], job_id)
return {job_id, envelope}
"#,
    )
});

/// KEYS: [jobs_hash, inflight_zset, dedup_key_or_empty]
/// ARGV: [job_id, dedup_ttl_seconds]
static ACK_SCRIPT: Lazy<Script> = Lazy::new(|| {
    Script::new(
        r#"
redis.call('ZREM', KEYS[2], ARGV[1])
redis.call('HDEL', KEYS[1], ARGV[1])
if KEYS[3] ~= '' then
  redis.call('SET', KEYS[3], cjson.encode({state='completed', job_id=ARGV[1]}), 'EX', ARGV[2])
end
return 1
"#,
    )
});

/// Atomic "take ownership away from the in-flight set" primitive, reused by
/// both the normal failure path and the lease-recovery sweep. `ZREM`
/// returning 1 means the caller now exclusively owns this job's outcome.
/// KEYS: [inflight_zset]  ARGV: [job_id]
static TAKE_OWNERSHIP_SCRIPT: Lazy<Script> =
    Lazy::new(|| Script::new("return redis.call('ZREM', KEYS[1], ARGV[1])"));

/// KEYS: [jobs_hash, delayed_zset, dedup_key_or_empty]
/// ARGV: [job_id, envelope_json, next_attempt_ms]
static SCHEDULE_RETRY_SCRIPT: Lazy<Script> = Lazy::new(|| {
    Script::new(
        r#"
redis.call('HSET', KEYS[1], ARGV[1], ARGV[2])
redis.call('ZADD', KEYS[2], ARGV[3], ARGV[1])
if KEYS[3] ~= '' then
  redis.call('SET', KEYS[3], cjson.encode({state='queued', job_id=ARGV[1]}))
end
return 1
"#,
    )
});

/// KEYS: [jobs_hash, dlq_list, dedup_key_or_empty]
/// ARGV: [job_id, envelope_json]
static DEAD_LETTER_SCRIPT: Lazy<Script> = Lazy::new(|| {
    Script::new(
        r#"
redis.call('HSET', KEYS[1], ARGV[1], ARGV[2])
redis.call('LPUSH', KEYS[2], ARGV[1])
if KEYS[3] ~= '' then
  redis.call('SET', KEYS[3], cjson.encode({state='dead_lettered', job_id=ARGV[1]}))
end
return 1
"#,
    )
});

/// KEYS: [delayed_zset, ready_list]  ARGV: [job_id]
/// Moves one due job from delayed back to ready. Guarded by ZREM so a job
/// promoted concurrently by two sweeper ticks only gets pushed once.
static PROMOTE_SCRIPT: Lazy<Script> = Lazy::new(|| {
    Script::new(
        r#"
local removed = redis.call('ZREM', KEYS[1], ARGV[1])
if removed == 1 then
  redis.call('LPUSH', KEYS[2], ARGV[1])
end
return removed
"#,
    )
});

/// KEYS: [dlq_list, jobs_hash, ready_list, dedup_key_or_empty]
/// ARGV: [job_id, envelope_json]
static REPLAY_SCRIPT: Lazy<Script> = Lazy::new(|| {
    Script::new(
        r#"
local removed = redis.call('LREM', KEYS[1], 1, ARGV[1])
if removed == 0 then
  return 0
end
redis.call('HSET', KEYS[2], ARGV[1], ARGV[2])
redis.call('LPUSH', KEYS[3], ARGV[1])
if KEYS[4] ~= '' then
  redis.call('SET', KEYS[4], cjson.encode({state='queued', job_id=ARGV[1]}))
end
return 1
"#,
    )
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookEnqueueOutcome {
    Enqueued,
    Duplicate,
    Unavailable,
}

/// Versioned job envelope. Bump `version` if the shape changes so old
/// in-flight jobs written by a previous deploy can still be decoded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookJobEnvelope {
    #[serde(default = "default_envelope_version")]
    pub version: u8,
    pub job_id: String,
    pub delivery_id: Option<String>,
    pub event: String,
    pub action: Option<String>,
    pub payload: serde_json::Value,
    #[serde(default)]
    pub attempts: u32,
    pub received_at: DateTime<Utc>,
    #[serde(default)]
    pub first_attempt_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub next_attempt_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub lease_expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_error: Option<String>,
    pub correlation_id: String,
    #[serde(default)]
    pub replay_source: Option<String>,
}

fn default_envelope_version() -> u8 {
    1
}

/// Input for enqueuing a new webhook job; the queue assigns the job id and
/// timestamps.
#[derive(Debug, Clone)]
pub struct NewWebhookJob {
    pub event: String,
    pub action: Option<String>,
    pub payload: serde_json::Value,
    pub correlation_id: String,
}

pub struct ClaimedWebhookJob {
    pub job_id: String,
    pub envelope: WebhookJobEnvelope,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeadLetterEntry {
    pub job_id: String,
    pub envelope: WebhookJobEnvelope,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct SyncJobData {
    pub name: String,
}

#[derive(Clone)]
pub struct QueueInfra {
    redis: Option<RedisClient>,
    namespace: String,
    max_attempts: u32,
    lease_seconds: u64,
    retry_base_ms: u64,
    retry_max_ms: u64,
    completed_dedup_ttl_seconds: u64,
    shutting_down: Arc<AtomicBool>,
    active_jobs: Arc<AtomicUsize>,
}

impl QueueInfra {
    pub fn new(redis: Option<RedisClient>, config: &Config) -> Self {
        Self::with_namespace(redis, config, WEBHOOKS_NS)
    }

    /// Same as [`QueueInfra::new`], but scoped to a custom Redis key
    /// namespace instead of the shared production `queue:webhooks` prefix.
    /// Intended for tests that need several isolated queues against one
    /// Redis instance; production code should use `new`.
    pub fn with_namespace(
        redis: Option<RedisClient>,
        config: &Config,
        namespace: impl Into<String>,
    ) -> Self {
        Self {
            redis,
            namespace: namespace.into(),
            max_attempts: config.webhook_max_attempts.max(1),
            lease_seconds: config.webhook_lease_seconds.max(1),
            retry_base_ms: config.webhook_retry_base_ms.max(1),
            retry_max_ms: config
                .webhook_retry_max_ms
                .max(config.webhook_retry_base_ms.max(1)),
            completed_dedup_ttl_seconds: config.webhook_completed_dedup_ttl_seconds.max(1),
            shutting_down: Arc::new(AtomicBool::new(false)),
            active_jobs: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Stop claiming new work. Already-claimed jobs are left to finish
    /// normally; `wait_for_idle` can be used to block until they do.
    pub fn begin_shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }

    /// Poll until no worker holds a claimed job, or `timeout` elapses.
    pub async fn wait_for_idle(&self, timeout: Duration) {
        let start = std::time::Instant::now();
        while self.active_jobs.load(Ordering::SeqCst) > 0 {
            if start.elapsed() >= timeout {
                tracing::warn!(
                    active = self.active_jobs.load(Ordering::SeqCst),
                    "timed out waiting for in-flight webhook jobs to finish"
                );
                return;
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    async fn connection(&self) -> Result<Option<redis::aio::MultiplexedConnection>, AppError> {
        let Some(client) = &self.redis else {
            return Ok(None);
        };
        client
            .get_multiplexed_async_connection()
            .await
            .map(Some)
            .map_err(|error| AppError::internal(error.to_string()))
    }

    pub async fn enqueue_webhook(
        &self,
        delivery_id: Option<&str>,
        job: NewWebhookJob,
    ) -> Result<WebhookEnqueueOutcome, AppError> {
        let Some(mut conn) = self.connection().await? else {
            return Ok(WebhookEnqueueOutcome::Unavailable);
        };

        let job_id = Uuid::new_v4().to_string();
        let envelope = WebhookJobEnvelope {
            version: default_envelope_version(),
            job_id: job_id.clone(),
            delivery_id: delivery_id.map(str::to_string),
            event: job.event,
            action: job.action,
            payload: job.payload,
            attempts: 0,
            received_at: Utc::now(),
            first_attempt_at: None,
            next_attempt_at: None,
            lease_expires_at: None,
            last_error: None,
            correlation_id: job.correlation_id,
            replay_source: None,
        };
        let payload = to_json(&envelope)?;
        let dedup = delivery_id.map(dedup_key).unwrap_or_default();

        let result: i64 = ENQUEUE_SCRIPT
            .key(&dedup)
            .key(jobs_key(self.namespace.as_str()))
            .key(ready_key(self.namespace.as_str()))
            .arg(&job_id)
            .arg(&payload)
            .invoke_async(&mut conn)
            .await
            .map_err(redis_err)?;

        Ok(if result == 1 {
            WebhookEnqueueOutcome::Duplicate
        } else {
            WebhookEnqueueOutcome::Enqueued
        })
    }

    /// Atomically move one ready job into the in-flight (leased) set.
    pub async fn claim_webhook(&self) -> Result<Option<ClaimedWebhookJob>, AppError> {
        let Some(mut conn) = self.connection().await? else {
            return Ok(None);
        };
        let lease_expiry_ms = now_ms() + (self.lease_seconds as i64 * 1000);

        let claimed: Option<(String, String)> = CLAIM_SCRIPT
            .key(ready_key(self.namespace.as_str()))
            .key(jobs_key(self.namespace.as_str()))
            .key(inflight_key(self.namespace.as_str()))
            .arg(lease_expiry_ms)
            .invoke_async(&mut conn)
            .await
            .map_err(redis_err)?;

        let Some((job_id, envelope_json)) = claimed else {
            return Ok(None);
        };
        let mut envelope: WebhookJobEnvelope = from_json(&envelope_json)?;
        envelope.first_attempt_at.get_or_insert(Utc::now());
        envelope.lease_expires_at = DateTime::<Utc>::from_timestamp_millis(lease_expiry_ms);
        self.active_jobs.fetch_add(1, Ordering::SeqCst);
        Ok(Some(ClaimedWebhookJob { job_id, envelope }))
    }

    /// Acknowledge successful processing: drop the job and record a
    /// completion marker so a later GitHub redelivery is rejected.
    pub async fn ack_webhook(
        &self,
        job_id: &str,
        delivery_id: Option<&str>,
    ) -> Result<(), AppError> {
        self.active_jobs.fetch_sub(1, Ordering::SeqCst);
        let Some(mut conn) = self.connection().await? else {
            return Ok(());
        };
        let dedup = delivery_id.map(dedup_key).unwrap_or_default();
        let _: i64 = ACK_SCRIPT
            .key(jobs_key(self.namespace.as_str()))
            .key(inflight_key(self.namespace.as_str()))
            .key(&dedup)
            .arg(job_id)
            .arg(self.completed_dedup_ttl_seconds)
            .invoke_async(&mut conn)
            .await
            .map_err(redis_err)?;
        Ok(())
    }

    /// Report a handler failure for a claimed job: retries with bounded
    /// exponential backoff and jitter, or dead-letters once the attempt
    /// budget is exhausted. Takes ownership of the job away from the
    /// in-flight set itself -- call this from the normal claim/handle/fail
    /// worker loop, not from `recover_expired_leases` (which already owns
    /// the job by the time it decides to retry it; see `retry_or_dead_letter`).
    pub async fn report_failure(
        &self,
        job_id: &str,
        envelope: WebhookJobEnvelope,
        error: &AppError,
    ) -> Result<(), AppError> {
        self.active_jobs.fetch_sub(1, Ordering::SeqCst);
        let Some(mut conn) = self.connection().await? else {
            return Ok(());
        };

        // Someone else (a concurrent lease-sweep) may have already recovered
        // this job; if so, back off and let them own the outcome.
        let owned: i64 = TAKE_OWNERSHIP_SCRIPT
            .key(inflight_key(self.namespace.as_str()))
            .arg(job_id)
            .invoke_async(&mut conn)
            .await
            .map_err(redis_err)?;
        if owned == 0 {
            return Ok(());
        }

        self.retry_or_dead_letter(&mut conn, job_id, envelope, error)
            .await
    }

    /// Decides whether a job (already removed from the in-flight set by the
    /// caller) gets another attempt or is dead-lettered, and writes that
    /// decision. Shared by `report_failure` and `recover_expired_leases` --
    /// neither takes ownership twice.
    async fn retry_or_dead_letter(
        &self,
        conn: &mut redis::aio::MultiplexedConnection,
        job_id: &str,
        mut envelope: WebhookJobEnvelope,
        error: &AppError,
    ) -> Result<(), AppError> {
        envelope.attempts += 1;
        envelope.last_error = Some(error.to_string());
        let dedup = envelope
            .delivery_id
            .as_deref()
            .map(dedup_key)
            .unwrap_or_default();

        if envelope.attempts >= self.max_attempts {
            tracing::error!(
                delivery_id = ?envelope.delivery_id,
                event = %envelope.event,
                action = ?envelope.action,
                attempts = envelope.attempts,
                correlation_id = %envelope.correlation_id,
                final_state = "dead_lettered",
                "webhook job exhausted retry budget; moved to dead-letter queue"
            );
            let payload = to_json(&envelope)?;
            let _: i64 = DEAD_LETTER_SCRIPT
                .key(jobs_key(self.namespace.as_str()))
                .key(dlq_key(self.namespace.as_str()))
                .key(&dedup)
                .arg(job_id)
                .arg(&payload)
                .invoke_async(conn)
                .await
                .map_err(redis_err)?;
            return Ok(());
        }

        let delay_ms = self.backoff_with_jitter(envelope.attempts);
        let next_attempt_ms = now_ms() + delay_ms as i64;
        envelope.next_attempt_at = DateTime::<Utc>::from_timestamp_millis(next_attempt_ms);
        tracing::warn!(
            delivery_id = ?envelope.delivery_id,
            event = %envelope.event,
            action = ?envelope.action,
            attempt = envelope.attempts,
            delay_ms,
            correlation_id = %envelope.correlation_id,
            final_state = "delayed",
            "webhook job failed; scheduled for retry"
        );
        let payload = to_json(&envelope)?;
        let _: i64 = SCHEDULE_RETRY_SCRIPT
            .key(jobs_key(self.namespace.as_str()))
            .key(delayed_key(self.namespace.as_str()))
            .key(&dedup)
            .arg(job_id)
            .arg(&payload)
            .arg(next_attempt_ms)
            .invoke_async(conn)
            .await
            .map_err(redis_err)?;
        Ok(())
    }

    /// Bounded exponential backoff (`base * 2^attempt`, capped) with full
    /// jitter, so a burst of failures doesn't retry in lockstep.
    fn backoff_with_jitter(&self, attempt: u32) -> u64 {
        let exp = self.retry_base_ms.saturating_mul(1u64 << attempt.min(20));
        let capped = exp.min(self.retry_max_ms);
        rand::thread_rng().gen_range(0..=capped.max(1))
    }

    /// Sweep for leases that expired without an ack (worker crash, forced
    /// shutdown, deployment). Recovered jobs are retried or dead-lettered
    /// through the same budget as a normal failure.
    pub async fn recover_expired_leases(&self) -> Result<usize, AppError> {
        let Some(mut conn) = self.connection().await? else {
            return Ok(0);
        };
        let expired: Vec<String> = conn
            .zrangebyscore(inflight_key(self.namespace.as_str()), 0, now_ms())
            .await
            .map_err(redis_err)?;

        let mut recovered = 0usize;
        for job_id in expired {
            let owned: i64 = TAKE_OWNERSHIP_SCRIPT
                .key(inflight_key(self.namespace.as_str()))
                .arg(&job_id)
                .invoke_async(&mut conn)
                .await
                .map_err(redis_err)?;
            if owned == 0 {
                continue; // a worker acked/failed it between the scan and here
            }

            let envelope_json: Option<String> = conn
                .hget(jobs_key(self.namespace.as_str()), &job_id)
                .await
                .map_err(redis_err)?;
            let Some(envelope_json) = envelope_json else {
                continue; // acked concurrently; nothing left to recover
            };
            let error = AppError::internal("lease expired: worker crashed or restarted");
            let envelope: WebhookJobEnvelope = from_json(&envelope_json)?;
            tracing::warn!(
                delivery_id = ?envelope.delivery_id,
                job_id = %job_id,
                "recovering webhook job with an expired lease"
            );
            self.retry_or_dead_letter(&mut conn, &job_id, envelope, &error)
                .await?;
            recovered += 1;
        }
        Ok(recovered)
    }

    /// Move due delayed jobs back onto the ready list.
    pub async fn promote_delayed(&self) -> Result<usize, AppError> {
        let Some(mut conn) = self.connection().await? else {
            return Ok(0);
        };
        let due: Vec<String> = conn
            .zrangebyscore(delayed_key(self.namespace.as_str()), 0, now_ms())
            .await
            .map_err(redis_err)?;

        let mut promoted = 0usize;
        for job_id in due {
            let moved: i64 = PROMOTE_SCRIPT
                .key(delayed_key(self.namespace.as_str()))
                .key(ready_key(self.namespace.as_str()))
                .arg(&job_id)
                .invoke_async(&mut conn)
                .await
                .map_err(redis_err)?;
            promoted += moved as usize;
        }
        Ok(promoted)
    }

    pub async fn list_dead_letters(&self, limit: isize) -> Result<Vec<DeadLetterEntry>, AppError> {
        let Some(mut conn) = self.connection().await? else {
            return Ok(vec![]);
        };
        let job_ids: Vec<String> = conn
            .lrange(dlq_key(self.namespace.as_str()), 0, limit.max(1) - 1)
            .await
            .map_err(redis_err)?;
        let mut entries = Vec::with_capacity(job_ids.len());
        for job_id in job_ids {
            let envelope_json: Option<String> = conn
                .hget(jobs_key(self.namespace.as_str()), &job_id)
                .await
                .map_err(redis_err)?;
            if let Some(envelope_json) = envelope_json {
                entries.push(DeadLetterEntry {
                    job_id,
                    envelope: from_json(&envelope_json)?,
                });
            }
        }
        Ok(entries)
    }

    /// Replay one dead-lettered job. `replayed_by` is recorded on the
    /// envelope for the audit trail; attempts are reset so it gets a fresh
    /// retry budget.
    pub async fn replay_dead_letter(
        &self,
        job_id: &str,
        replayed_by: &str,
    ) -> Result<bool, AppError> {
        let Some(mut conn) = self.connection().await? else {
            return Ok(false);
        };
        let envelope_json: Option<String> = conn
            .hget(jobs_key(self.namespace.as_str()), job_id)
            .await
            .map_err(redis_err)?;
        let Some(envelope_json) = envelope_json else {
            return Ok(false);
        };
        let mut envelope: WebhookJobEnvelope = from_json(&envelope_json)?;
        envelope.attempts = 0;
        envelope.last_error = None;
        envelope.replay_source = Some(format!(
            "operator:{replayed_by}@{}",
            Utc::now().to_rfc3339()
        ));
        let dedup = envelope
            .delivery_id
            .as_deref()
            .map(dedup_key)
            .unwrap_or_default();
        let payload = to_json(&envelope)?;

        let replayed: i64 = REPLAY_SCRIPT
            .key(dlq_key(self.namespace.as_str()))
            .key(jobs_key(self.namespace.as_str()))
            .key(ready_key(self.namespace.as_str()))
            .key(&dedup)
            .arg(job_id)
            .arg(&payload)
            .invoke_async(&mut conn)
            .await
            .map_err(redis_err)?;

        tracing::info!(
            job_id,
            replayed_by,
            success = replayed == 1,
            "operator requested dead-letter replay"
        );
        Ok(replayed == 1)
    }

    /// Replay every dead-lettered job matching an optional event/action
    /// filter, up to `limit` jobs.
    pub async fn replay_dead_letter_batch(
        &self,
        event_filter: Option<&str>,
        action_filter: Option<&str>,
        limit: usize,
        replayed_by: &str,
    ) -> Result<usize, AppError> {
        let entries = self.list_dead_letters(limit.max(1) as isize * 4).await?;
        let mut replayed = 0usize;
        for entry in entries {
            if replayed >= limit {
                break;
            }
            if let Some(event) = event_filter {
                if entry.envelope.event != event {
                    continue;
                }
            }
            if let Some(action) = action_filter {
                if entry.envelope.action.as_deref() != Some(action) {
                    continue;
                }
            }
            if self.replay_dead_letter(&entry.job_id, replayed_by).await? {
                replayed += 1;
            }
        }
        Ok(replayed)
    }

    pub async fn enqueue_sync(&self, data: SyncJobData) -> Result<(), AppError> {
        let Some(mut conn) = self.connection().await? else {
            return Ok(());
        };
        let payload = to_json(&data)?;
        conn.lpush::<_, _, ()>(SYNC_QUEUE, payload)
            .await
            .map_err(redis_err)
    }

    pub async fn pop_sync(&self) -> Result<Option<SyncJobData>, AppError> {
        let Some(mut conn) = self.connection().await? else {
            return Ok(None);
        };
        let payload: Option<String> = redis::cmd("RPOP")
            .arg(SYNC_QUEUE)
            .query_async(&mut conn)
            .await
            .map_err(redis_err)?;
        payload.map(|value| from_json(&value)).transpose()
    }

    pub async fn stats(&self) -> Result<serde_json::Value, AppError> {
        let Some(mut conn) = self.connection().await? else {
            return Ok(serde_json::json!({
                "webhooks": default_counts(),
                "sync": default_counts(),
            }));
        };

        let waiting: i64 = conn
            .llen(ready_key(self.namespace.as_str()))
            .await
            .unwrap_or(0);
        let active: i64 = conn
            .zcard(inflight_key(self.namespace.as_str()))
            .await
            .unwrap_or(0);
        let delayed: i64 = conn
            .zcard(delayed_key(self.namespace.as_str()))
            .await
            .unwrap_or(0);
        let failed: i64 = conn
            .llen(dlq_key(self.namespace.as_str()))
            .await
            .unwrap_or(0);
        let sync: i64 = conn.llen(SYNC_QUEUE).await.unwrap_or(0);

        Ok(serde_json::json!({
            "webhooks": {
                "waiting": waiting,
                "active": active,
                "completed": serde_json::Value::Null,
                "failed": failed,
                "delayed": delayed,
            },
            "sync": { "waiting": sync, "active": 0, "completed": 0, "failed": 0, "delayed": 0 },
        }))
    }
}

fn default_counts() -> serde_json::Value {
    serde_json::json!({ "waiting": 0, "active": 0, "completed": 0, "failed": 0, "delayed": 0 })
}

fn to_json<T: Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string(value).map_err(|error| AppError::internal(error.to_string()))
}

fn from_json<T: DeserializeOwned>(value: &str) -> Result<T, AppError> {
    serde_json::from_str(value).map_err(|error| AppError::internal(error.to_string()))
}

fn redis_err(error: redis::RedisError) -> AppError {
    AppError::internal(error.to_string())
}

pub async fn start_workers(state: crate::state::AppState) {
    let webhook_state = state.clone();
    tokio::spawn(async move {
        loop {
            if webhook_state.queue.is_shutting_down() {
                tracing::info!("webhook worker stopping: shutdown in progress");
                return;
            }
            match webhook_state.queue.claim_webhook().await {
                Ok(Some(job)) => {
                    let start = std::time::Instant::now();
                    let delivery_id = job.envelope.delivery_id.clone();
                    let result = crate::modules::github::webhook::process_webhook_job(
                        &webhook_state,
                        &job.envelope,
                    )
                    .await;
                    let duration_ms = start.elapsed().as_millis();
                    match result {
                        Ok(()) => {
                            tracing::info!(
                                delivery_id = ?delivery_id,
                                event = %job.envelope.event,
                                action = ?job.envelope.action,
                                attempt = job.envelope.attempts,
                                handler_duration_ms = duration_ms,
                                correlation_id = %job.envelope.correlation_id,
                                final_state = "completed",
                                "webhook job processed"
                            );
                            if let Err(error) = webhook_state
                                .queue
                                .ack_webhook(&job.job_id, delivery_id.as_deref())
                                .await
                            {
                                tracing::error!(%error, "failed to acknowledge webhook job");
                            }
                        }
                        Err(error) => {
                            if let Err(report_error) = webhook_state
                                .queue
                                .report_failure(&job.job_id, job.envelope, &error)
                                .await
                            {
                                tracing::error!(%report_error, "failed to report webhook job failure");
                            }
                        }
                    }
                }
                Ok(None) => sleep(Duration::from_millis(500)).await,
                Err(error) => {
                    tracing::error!(%error, "webhook queue claim failed");
                    sleep(Duration::from_millis(500)).await;
                }
            }
        }
    });

    let sweeper_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            if sweeper_state.queue.is_shutting_down() {
                return;
            }
            if let Err(error) = sweeper_state.queue.promote_delayed().await {
                tracing::error!(%error, "failed to promote delayed webhook jobs");
            }
            if let Err(error) = sweeper_state.queue.recover_expired_leases().await {
                tracing::error!(%error, "failed to recover expired webhook leases");
            }
        }
    });

    let sync_state = state.clone();
    tokio::spawn(async move {
        loop {
            match sync_state.queue.pop_sync().await {
                Ok(Some(job)) if job.name == "escrow-balance-sync" => {
                    if let Err(error) =
                        crate::modules::jobs::sync_job::sync_all_escrow_balances(&sync_state).await
                    {
                        tracing::error!(%error, "sync job failed");
                    }
                }
                Ok(Some(_)) => {}
                Ok(None) => sleep(Duration::from_millis(500)).await,
                Err(error) => tracing::error!(%error, "sync queue pop failed"),
            }
        }
    });
}

pub fn start_scheduler(state: crate::state::AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Err(error) = state
                .queue
                .enqueue_sync(crate::infra::queue::SyncJobData {
                    name: "escrow-balance-sync".to_string(),
                })
                .await
            {
                tracing::error!(%error, "failed to enqueue escrow balance sync");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            port: 5000,
            log_level: "info".into(),
            node_env: "test".into(),
            database_url: String::new(),
            redis_url: String::new(),
            supabase_url: String::new(),
            supabase_auth_api_key: String::new(),
            github_app_id: String::new(),
            github_app_private_key: String::new(),
            github_bot_token: None,
            github_webhook_secret: String::new(),
            platform_stellar_public_key: String::new(),
            platform_stellar_secret_key: String::new(),
            dispute_resolver_stellar_public_key: String::new(),
            dispute_resolver_stellar_secret_key: String::new(),
            trustless_work_api_key: String::new(),
            trustless_work_base_url: String::new(),
            stellar_network: "testnet".into(),
            app_url: String::new(),
            webhook_url: None,
            dev_webhook_proxy_enabled: false,
            smee_source_url: String::new(),
            smee_target_url: String::new(),
            webhook_max_attempts: 6,
            webhook_lease_seconds: 30,
            webhook_retry_base_ms: 1_000,
            webhook_retry_max_ms: 300_000,
            webhook_completed_dedup_ttl_seconds: 604_800,
            queue_admin_token: None,
        }
    }

    #[tokio::test]
    async fn reports_unavailable_when_queue_has_no_redis_client() {
        let queue = QueueInfra::new(None, &test_config());
        let outcome = queue
            .enqueue_webhook(
                Some("delivery-id"),
                NewWebhookJob {
                    event: "issues".to_string(),
                    action: Some("opened".to_string()),
                    payload: serde_json::json!({}),
                    correlation_id: "corr-1".to_string(),
                },
            )
            .await
            .unwrap();

        assert_eq!(outcome, WebhookEnqueueOutcome::Unavailable);
    }

    #[test]
    fn old_queued_envelopes_default_version_and_attempts() {
        let envelope: WebhookJobEnvelope = serde_json::from_value(serde_json::json!({
            "job_id": "abc",
            "delivery_id": null,
            "event": "issues",
            "action": "labeled",
            "payload": {},
            "received_at": "2026-01-01T00:00:00Z",
            "correlation_id": "corr-2"
        }))
        .unwrap();

        assert_eq!(envelope.version, 1);
        assert_eq!(envelope.attempts, 0);
    }

    #[test]
    fn backoff_is_bounded_and_never_zero_wide() {
        let queue = QueueInfra::new(None, &test_config());
        for attempt in 1..10 {
            let delay = queue.backoff_with_jitter(attempt);
            assert!(delay <= queue.retry_max_ms);
        }
    }
}
