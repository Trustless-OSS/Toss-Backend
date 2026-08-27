//! The background-job hub.
//!
//! Every queue, worker and scheduler in the backend is created here, on top of
//! [BullMQ for Rust]. The rest of the application never imports `bullmq`: it
//! holds a [`QueueInfra`] on `AppState` and only *adds* jobs. Workers — defined
//! in [`crate::infra::jobs`] — are the only place background work runs.
//!
//! ## Queues
//!
//! | Queue | Job names | Purpose |
//! | --- | --- | --- |
//! | `toss-webhooks` | `github-webhook` | Signed GitHub deliveries |
//! | `toss-bounty` | `advance-issue`, `push-milestone`, `release-payout` | The bounty state machine and every on-chain action |
//! | `toss-sync` | `escrow-balance-sync` | The repeating escrow balance reconciliation |
//!
//! ## Deduplication
//!
//! Jobs carry stable ids — `github-webhook:<delivery-id>` and
//! `<job-name>:<issue-id>` — so a burst of events collapses into a single job.
//! Enqueueing is *ensure* semantics rather than blind `add`:
//!
//! - no job with that id → add it;
//! - a finished job holds the id → drop it and add a fresh one;
//! - a delayed re-check holds it → promote it so the new event is acted on now;
//! - a job is already waiting → nothing to do;
//! - a job is *running* → set a dirty flag so the running worker re-evaluates
//!   before it finishes, which is what stops an event that lands mid-job from
//!   being silently lost.
//! - after that job leaves `active`, the dirty flag is drained into a fresh
//!   `advance-issue` so an event that arrived on the last `is_dirty` check
//!   cannot disappear with the completed job.
//!
//! Enqueueing a bounty job takes a per-id Redis lock so `get_job_state` and the
//! matching `add` / `remove` / `promote` / `mark_dirty` cannot interleave.
//!
//! Dirty-flag keys are namespaced with the same BullMQ prefix as the queues.
//!
//! [BullMQ for Rust]: https://docs.bullmq.io/rust/introduction

use std::{sync::Arc, time::Duration};

use bullmq::{
    job_scheduler::RepeatOptions,
    options::{QueueOptions, RedisConnectionOptions, WorkerOptions},
    types::{BackoffStrategy, KeepJobs, RemoveOnFinish},
    JobOptions, Queue, Worker,
};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::AppError, infra::redis::RedisClient, state::AppState};

// ── Queue names ──────────────────────────────────────────────────────────────

pub const WEBHOOK_QUEUE: &str = "toss-webhooks";
pub const BOUNTY_QUEUE: &str = "toss-bounty";
pub const SYNC_QUEUE: &str = "toss-sync";

// ── Job names ────────────────────────────────────────────────────────────────

pub const JOB_GITHUB_WEBHOOK: &str = "github-webhook";
pub const JOB_ADVANCE_ISSUE: &str = "advance-issue";
pub const JOB_PUSH_MILESTONE: &str = "push-milestone";
pub const JOB_RELEASE_PAYOUT: &str = "release-payout";
pub const JOB_ESCROW_BALANCE_SYNC: &str = "escrow-balance-sync";

// ── Retry policy ─────────────────────────────────────────────────────────────

/// Attempts for GitHub deliveries — retried on transient GitHub/network faults.
const WEBHOOK_ATTEMPTS: u32 = 5;
/// Attempts for bounty jobs, which also cover Trustless Work timeouts and 5xx.
const BOUNTY_ATTEMPTS: u32 = 5;
const SYNC_ATTEMPTS: u32 = 3;

const WEBHOOK_BACKOFF_MS: u64 = 2_000;
const BOUNTY_BACKOFF_MS: u64 = 5_000;

/// Completed webhook jobs are kept for a day so a GitHub redelivery of the same
/// `X-GitHub-Delivery` is recognised as a duplicate instead of reprocessed.
const WEBHOOK_KEEP_COMPLETED_MS: u64 = 24 * 60 * 60 * 1_000;
/// Failed jobs are the dead-letter record; keep them long enough to inspect.
const KEEP_FAILED_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

const DIRTY_FLAG_SUFFIX: &str = "toss:advance:dirty:";
const DIRTY_FLAG_TTL_SECS: u64 = 3_600;
const ENQUEUE_LOCK_SUFFIX: &str = "toss:enqueue:";
const ENQUEUE_LOCK_TTL_MS: u64 = 5_000;
const ENQUEUE_LOCK_RETRIES: u32 = 40;
const ENQUEUE_LOCK_WAIT: Duration = Duration::from_millis(25);

/// How long a worker is given to finish in-flight jobs during shutdown.
const WORKER_CLOSE_TIMEOUT_MS: u64 = 10_000;

// ── Job payloads ─────────────────────────────────────────────────────────────

/// The body of a `github-webhook` job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookJobData {
    pub event: String,
    pub action: Option<String>,
    pub payload: serde_json::Value,
}

/// The body of every job on the bounty queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BountyJobData {
    /// The issue this job operates on.
    pub issue_id: Uuid,
    /// What caused the job — carried purely for logs and traceability.
    pub trigger: String,
    /// How many delayed re-checks have already run for this issue.
    #[serde(default)]
    pub recheck: u32,
    /// Whether the worker may post a user-visible comment when it parks the flow.
    #[serde(default)]
    pub notify: bool,
    /// How many queue-to-queue handoffs led here.
    ///
    /// `advance-issue` queues `push-milestone`, which queues `advance-issue`
    /// again, and so on. The counter bounds that chain so a condition the chain
    /// never satisfies cannot ping-pong forever.
    #[serde(default)]
    pub hops: u32,
}

/// The longest legitimate handoff chain is
/// `advance` → `push-milestone` → `advance` → `release-payout`; the cap leaves
/// room for one repeat before the chain is cut.
pub const MAX_HOPS: u32 = 6;

impl BountyJobData {
    pub fn new(issue_id: Uuid, trigger: impl Into<String>) -> Self {
        Self {
            issue_id,
            trigger: trigger.into(),
            recheck: 0,
            notify: false,
            hops: 0,
        }
    }

    /// The next job in a handoff chain, carrying the hop count forward.
    pub fn next(&self, trigger: impl Into<String>) -> Self {
        Self {
            issue_id: self.issue_id,
            trigger: trigger.into(),
            recheck: self.recheck,
            notify: false,
            hops: self.hops + 1,
        }
    }

    /// Whether this job is past the handoff budget.
    pub fn exhausted(&self) -> bool {
        self.hops > MAX_HOPS
    }

    /// Allow this job to comment on the issue when it parks the flow.
    pub fn notifying(mut self) -> Self {
        self.notify = true;
        self
    }

    pub fn with_recheck(mut self, recheck: u32) -> Self {
        self.recheck = recheck;
        self
    }
}

// ── Enqueue outcomes ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookEnqueueOutcome {
    Enqueued,
    /// GitHub redelivered a `X-GitHub-Delivery` we already have.
    Duplicate,
    /// No queue is available; the caller should process the event inline.
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueOutcome {
    /// A new job was added.
    Enqueued,
    /// A job for this key was already waiting.
    AlreadyPending,
    /// A delayed re-check existed and was pulled forward to run now.
    Promoted,
    /// A job is running; it was flagged to re-evaluate before finishing.
    Coalesced,
    /// No queue is available.
    Unavailable,
}

impl EnqueueOutcome {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Enqueued => "enqueued",
            Self::AlreadyPending => "already-pending",
            Self::Promoted => "promoted",
            Self::Coalesced => "coalesced",
            Self::Unavailable => "unavailable",
        }
    }
}

// ── The hub ──────────────────────────────────────────────────────────────────

struct Queues {
    webhooks: Queue,
    bounty: Queue,
    sync: Queue,
}

/// The only queue handle the application code touches.
#[derive(Clone)]
pub struct QueueInfra {
    queues: Option<Arc<Queues>>,
    redis: Option<RedisClient>,
    prefix: String,
}

impl QueueInfra {
    /// A hub with no backing queues.
    ///
    /// Enqueueing reports [`WebhookEnqueueOutcome::Unavailable`] so callers fall
    /// back to inline processing, which keeps the API serving when Redis is down.
    pub fn disabled() -> Self {
        Self {
            queues: None,
            redis: None,
            prefix: String::new(),
        }
    }

    /// Connect the three BullMQ queues against `redis_url`.
    pub async fn connect(
        redis_url: &str,
        prefix: &str,
        redis: Option<RedisClient>,
    ) -> Result<Self, AppError> {
        let options = || {
            QueueOptions::new()
                .prefix(prefix)
                .connection(RedisConnectionOptions {
                    url: redis_url.to_string(),
                    ..Default::default()
                })
        };

        Ok(Self {
            queues: Some(Arc::new(Queues {
                webhooks: Queue::with_options(WEBHOOK_QUEUE, options())
                    .await
                    .map_err(queue_error)?,
                bounty: Queue::with_options(BOUNTY_QUEUE, options())
                    .await
                    .map_err(queue_error)?,
                sync: Queue::with_options(SYNC_QUEUE, options())
                    .await
                    .map_err(queue_error)?,
            })),
            redis,
            prefix: prefix.to_string(),
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.queues.is_some()
    }

    // ── Producers ────────────────────────────────────────────────────────────

    /// Queue a signed GitHub delivery for background processing.
    ///
    /// The delivery id becomes the BullMQ job id, so GitHub's own at-least-once
    /// delivery cannot cause the same event to be handled twice.
    pub async fn enqueue_webhook(
        &self,
        delivery_id: Option<&str>,
        data: WebhookJobData,
    ) -> Result<WebhookEnqueueOutcome, AppError> {
        let Some(queues) = self.queues.as_ref() else {
            return Ok(WebhookEnqueueOutcome::Unavailable);
        };

        let options = JobOptions::new()
            .attempts(WEBHOOK_ATTEMPTS)
            .backoff(BackoffStrategy::Exponential(WEBHOOK_BACKOFF_MS))
            .remove_on_complete(RemoveOnFinish::Options(KeepJobs {
                age: Some(WEBHOOK_KEEP_COMPLETED_MS),
                count: Some(1_000),
                limit: None,
            }))
            .remove_on_fail(RemoveOnFinish::Options(KeepJobs {
                age: Some(KEEP_FAILED_MS),
                count: Some(1_000),
                limit: None,
            }));

        let Some(delivery_id) = delivery_id else {
            // No delivery header to deduplicate on — add it with a generated id.
            queues
                .webhooks
                .add(JOB_GITHUB_WEBHOOK, &data)
                .options(options)
                .await
                .map_err(queue_error)?;
            return Ok(WebhookEnqueueOutcome::Enqueued);
        };

        let job_id = format!("{JOB_GITHUB_WEBHOOK}:{delivery_id}");
        match queues
            .webhooks
            .get_job_state(&job_id)
            .await
            .map_err(queue_error)?
        {
            bullmq::JobState::Unknown => {}
            // A delivery that exhausted its attempts should be revivable: GitHub
            // redelivering it is an explicit "try this again", not a duplicate.
            bullmq::JobState::Failed => {
                queues.webhooks.remove(&job_id).await.map_err(queue_error)?;
            }
            _ => return Ok(WebhookEnqueueOutcome::Duplicate),
        }

        queues
            .webhooks
            .add(JOB_GITHUB_WEBHOOK, &data)
            .options(options)
            .job_id(job_id)
            .await
            .map_err(queue_error)?;

        Ok(WebhookEnqueueOutcome::Enqueued)
    }

    /// Ask the state machine to re-evaluate an issue.
    ///
    /// This is the single entry point every producer uses — a label, an
    /// assignment, a wallet connect, a merged PR and the admin retry endpoint all
    /// funnel here.
    pub async fn enqueue_advance_issue(
        &self,
        data: BountyJobData,
    ) -> Result<EnqueueOutcome, AppError> {
        self.enqueue_bounty_job(JOB_ADVANCE_ISSUE, data, None).await
    }

    /// Re-evaluate an issue after `delay`, used when the flow is parked on an
    /// external condition such as a missing wallet.
    pub async fn enqueue_advance_issue_after(
        &self,
        data: BountyJobData,
        delay: Duration,
    ) -> Result<EnqueueOutcome, AppError> {
        self.enqueue_bounty_job(JOB_ADVANCE_ISSUE, data, Some(delay))
            .await
    }

    /// Queue the on-chain milestone push. Only the `advance-issue` worker calls
    /// this, and the push worker re-checks the rules before it acts.
    pub async fn enqueue_push_milestone(
        &self,
        data: BountyJobData,
    ) -> Result<EnqueueOutcome, AppError> {
        self.enqueue_bounty_job(JOB_PUSH_MILESTONE, data, None)
            .await
    }

    /// Queue the payout. Only the `advance-issue` worker calls this, and the
    /// release worker re-checks the rules before any funds move.
    pub async fn enqueue_release_payout(
        &self,
        data: BountyJobData,
    ) -> Result<EnqueueOutcome, AppError> {
        self.enqueue_bounty_job(JOB_RELEASE_PAYOUT, data, None)
            .await
    }

    async fn enqueue_bounty_job(
        &self,
        job_name: &str,
        data: BountyJobData,
        delay: Option<Duration>,
    ) -> Result<EnqueueOutcome, AppError> {
        if self.queues.is_none() {
            return Ok(EnqueueOutcome::Unavailable);
        }

        let job_id = format!("{job_name}:{}", data.issue_id);
        self.with_enqueue_lock(&job_id, || {
            self.enqueue_bounty_job_locked(job_name, data, delay)
        })
        .await
    }

    async fn enqueue_bounty_job_locked(
        &self,
        job_name: &str,
        data: BountyJobData,
        delay: Option<Duration>,
    ) -> Result<EnqueueOutcome, AppError> {
        let Some(queues) = self.queues.as_ref() else {
            return Ok(EnqueueOutcome::Unavailable);
        };

        let issue_id = data.issue_id;
        let job_id = format!("{job_name}:{issue_id}");

        let options = JobOptions::new()
            .attempts(BOUNTY_ATTEMPTS)
            .backoff(BackoffStrategy::Exponential(BOUNTY_BACKOFF_MS))
            // Completed bounty jobs are removed so the per-issue id frees up
            // immediately; a later event must always be able to enqueue again.
            .remove_on_complete(RemoveOnFinish::Bool(true))
            .remove_on_fail(RemoveOnFinish::Options(KeepJobs {
                age: Some(KEEP_FAILED_MS),
                count: Some(1_000),
                limit: None,
            }));

        match queues
            .bounty
            .get_job_state(&job_id)
            .await
            .map_err(queue_error)?
        {
            bullmq::JobState::Unknown => {}
            bullmq::JobState::Completed | bullmq::JobState::Failed => {
                // A finished job still owns the id. Clear it so this new event is
                // not swallowed — a permanently failed job must never wedge an
                // issue shut.
                queues.bounty.remove(&job_id).await.map_err(queue_error)?;
            }
            bullmq::JobState::Delayed if delay.is_none() => {
                // A re-check is already parked in the future; a real event just
                // arrived, so run it now instead of waiting out the delay.
                if let Some(job) = queues.bounty.get_job(&job_id).await.map_err(queue_error)? {
                    job.promote().await.map_err(queue_error)?;
                }
                return Ok(EnqueueOutcome::Promoted);
            }
            bullmq::JobState::Active => {
                // The worker is mid-flight. Flag the issue so it re-evaluates
                // before finishing rather than dropping this event.
                self.mark_dirty(issue_id).await?;
                return Ok(EnqueueOutcome::Coalesced);
            }
            _ => return Ok(EnqueueOutcome::AlreadyPending),
        }

        let mut add = queues
            .bounty
            .add(job_name, &data)
            .options(options)
            .job_id(job_id);
        if let Some(delay) = delay {
            add = add.delay(delay);
        }
        add.await.map_err(queue_error)?;

        Ok(EnqueueOutcome::Enqueued)
    }

    /// Serialise `get_job_state` with the matching mutate so two producers
    /// cannot observe different states for the same job id.
    async fn with_enqueue_lock<F, Fut>(
        &self,
        job_id: &str,
        action: F,
    ) -> Result<EnqueueOutcome, AppError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<EnqueueOutcome, AppError>>,
    {
        let Some(token) = self.acquire_enqueue_lock(job_id).await? else {
            return action().await;
        };
        let result = action().await;
        self.release_enqueue_lock(job_id, &token).await?;
        result
    }

    async fn acquire_enqueue_lock(&self, job_id: &str) -> Result<Option<String>, AppError> {
        let Some(client) = self.redis.as_ref() else {
            return Ok(None);
        };
        let key = self.enqueue_lock_key(job_id);
        let token = Uuid::now_v7().to_string();

        for _ in 0..ENQUEUE_LOCK_RETRIES {
            let mut connection = client
                .get_multiplexed_async_connection()
                .await
                .map_err(|error| AppError::internal(error.to_string()))?;
            let acquired: Option<String> = redis::cmd("SET")
                .arg(&key)
                .arg(&token)
                .arg("NX")
                .arg("PX")
                .arg(ENQUEUE_LOCK_TTL_MS)
                .query_async(&mut connection)
                .await
                .map_err(|error| AppError::internal(error.to_string()))?;
            if acquired.is_some() {
                return Ok(Some(token));
            }
            tokio::time::sleep(ENQUEUE_LOCK_WAIT).await;
        }

        // Prefer sending the event without the lock over dropping it.
        Ok(None)
    }

    async fn release_enqueue_lock(&self, job_id: &str, token: &str) -> Result<(), AppError> {
        let Some(client) = self.redis.as_ref() else {
            return Ok(());
        };
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        let _: () = redis::cmd("EVAL")
            .arg(
                "if redis.call('GET', KEYS[1]) == ARGV[1] then return redis.call('DEL', KEYS[1]) else return 0 end",
            )
            .arg(1)
            .arg(self.enqueue_lock_key(job_id))
            .arg(token)
            .query_async(&mut connection)
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        Ok(())
    }

    fn enqueue_lock_key(&self, job_id: &str) -> String {
        format!("{}:{ENQUEUE_LOCK_SUFFIX}{job_id}", self.prefix)
    }

    // ── Coalescing flag ──────────────────────────────────────────────────────

    /// Record that an issue changed while its job was running.
    pub async fn mark_dirty(&self, issue_id: Uuid) -> Result<(), AppError> {
        let Some(client) = self.redis.as_ref() else {
            return Ok(());
        };
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        connection
            .set_ex::<_, _, ()>(self.dirty_key(issue_id), 1, DIRTY_FLAG_TTL_SECS)
            .await
            .map_err(|error| AppError::internal(error.to_string()))
    }

    /// Clear the flag before a worker reads state, so anything that arrives from
    /// now on is guaranteed to be noticed.
    pub async fn clear_dirty(&self, issue_id: Uuid) -> Result<(), AppError> {
        let Some(client) = self.redis.as_ref() else {
            return Ok(());
        };
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        connection
            .del::<_, ()>(self.dirty_key(issue_id))
            .await
            .map_err(|error| AppError::internal(error.to_string()))
    }

    /// Whether new events arrived while the current pass was running.
    pub async fn is_dirty(&self, issue_id: Uuid) -> Result<bool, AppError> {
        let Some(client) = self.redis.as_ref() else {
            return Ok(false);
        };
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        connection
            .exists::<_, bool>(self.dirty_key(issue_id))
            .await
            .map_err(|error| AppError::internal(error.to_string()))
    }

    /// Atomically read and clear the dirty flag.
    pub async fn take_dirty(&self, issue_id: Uuid) -> Result<bool, AppError> {
        let Some(client) = self.redis.as_ref() else {
            return Ok(false);
        };
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        let value: Option<String> = redis::cmd("GETDEL")
            .arg(self.dirty_key(issue_id))
            .query_async(&mut connection)
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        Ok(value.is_some())
    }

    /// If events landed while an `advance-issue` job was leaving `active`,
    /// enqueue another pass so they are not lost.
    pub async fn drain_dirty_advance(&self, issue_id: Uuid) -> Result<EnqueueOutcome, AppError> {
        if !self.take_dirty(issue_id).await? {
            return Ok(EnqueueOutcome::AlreadyPending);
        }
        self.enqueue_advance_issue(BountyJobData::new(issue_id, "dirty-drain"))
            .await
    }

    /// Wait until `advance-issue:<id>` is no longer `active`, then drain.
    ///
    /// Called after the processor returns `Done` or `Delayed`. The job is still
    /// `active` until BullMQ records the result, so this runs in the background.
    pub fn schedule_dirty_drain(&self, issue_id: Uuid) {
        let queue = self.clone();
        tokio::spawn(async move {
            queue.wait_until_advance_idle(issue_id).await;
            if let Err(error) = queue.drain_dirty_advance(issue_id).await {
                tracing::warn!(%error, %issue_id, "failed to drain dirty advance flag");
            }
        });
    }

    async fn wait_until_advance_idle(&self, issue_id: Uuid) {
        let Some(queues) = self.queues.as_ref() else {
            return;
        };
        let job_id = format!("{JOB_ADVANCE_ISSUE}:{issue_id}");
        for _ in 0..100 {
            match queues.bounty.get_job_state(&job_id).await {
                Ok(bullmq::JobState::Active) => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                _ => return,
            }
        }
    }

    pub fn dirty_key(&self, issue_id: Uuid) -> String {
        dirty_key(&self.prefix, issue_id)
    }

    // ── Observability ────────────────────────────────────────────────────────

    /// Live BullMQ counts for every queue.
    pub async fn stats(&self) -> Result<serde_json::Value, AppError> {
        let Some(queues) = self.queues.as_ref() else {
            return Ok(serde_json::json!({
                "webhooks": empty_counts(),
                "escrow-operations": empty_counts(),
                "sync": empty_counts(),
            }));
        };

        let (webhooks, bounty, sync) = tokio::join!(
            queues.webhooks.get_job_counts(),
            queues.bounty.get_job_counts(),
            queues.sync.get_job_counts(),
        );

        Ok(serde_json::json!({
            "webhooks": counts_json(webhooks.map_err(queue_error)?),
            "escrow-operations": counts_json(bounty.map_err(queue_error)?),
            "sync": counts_json(sync.map_err(queue_error)?),
        }))
    }

    /// Register the repeating escrow balance sync.
    ///
    /// A BullMQ job scheduler holds exactly one pending job at a time, replacing
    /// the old loop that pushed a new list entry every 60 seconds whether or not
    /// the previous one had been consumed.
    pub async fn register_schedulers(&self, interval: Duration) -> Result<(), AppError> {
        let Some(queues) = self.queues.as_ref() else {
            return Ok(());
        };

        queues
            .sync
            .upsert_job_scheduler(
                JOB_ESCROW_BALANCE_SYNC,
                RepeatOptions {
                    every: Some(interval.as_millis() as u64),
                    ..Default::default()
                },
                Some(JOB_ESCROW_BALANCE_SYNC),
                Some(serde_json::json!({})),
                Some(
                    JobOptions::new()
                        .attempts(SYNC_ATTEMPTS)
                        .backoff(BackoffStrategy::Exponential(BOUNTY_BACKOFF_MS))
                        .remove_on_complete(RemoveOnFinish::Count(100))
                        .remove_on_fail(RemoveOnFinish::Count(100)),
                ),
            )
            .await
            .map_err(queue_error)?;

        Ok(())
    }

    /// Close the Redis connections behind the queues.
    pub async fn close(&self) {
        let Some(queues) = self.queues.as_ref() else {
            return;
        };
        queues.webhooks.close().await;
        queues.bounty.close().await;
        queues.sync.close().await;
    }
}

fn dirty_key(prefix: &str, issue_id: Uuid) -> String {
    format!("{prefix}:{DIRTY_FLAG_SUFFIX}{issue_id}")
}

fn empty_counts() -> serde_json::Value {
    serde_json::json!({
        "waiting": 0,
        "active": 0,
        "completed": 0,
        "failed": 0,
        "delayed": 0,
    })
}

fn counts_json(counts: bullmq::types::JobCounts) -> serde_json::Value {
    serde_json::json!({
        "waiting": counts.waiting,
        "active": counts.active,
        "completed": counts.completed,
        "failed": counts.failed,
        "delayed": counts.delayed,
    })
}

fn queue_error(error: bullmq::Error) -> AppError {
    AppError::internal(format!("queue error: {error}"))
}

// ── Workers ──────────────────────────────────────────────────────────────────

/// The running BullMQ workers, held by `main` so they are closed on shutdown.
pub struct Workers {
    workers: Vec<Worker>,
}

impl Workers {
    /// Stop accepting jobs and let in-flight work finish.
    pub async fn shutdown(self) {
        for worker in &self.workers {
            if let Err(error) = worker.close(WORKER_CLOSE_TIMEOUT_MS).await {
                tracing::error!(%error, "failed to close a queue worker cleanly");
            }
        }
        tracing::info!("background workers stopped");
    }
}

/// Start one worker per queue.
///
/// Processors live in [`crate::infra::jobs`]; this function only wires them to
/// their queue and retry settings.
pub async fn start_workers(state: AppState) -> Result<Workers, AppError> {
    if !state.queue.is_enabled() {
        tracing::warn!("queue infrastructure is unavailable; workers were not started");
        return Ok(Workers {
            workers: Vec::new(),
        });
    }

    let redis_url = state.config.redis_url.clone();
    let prefix = state.config.bullmq_prefix.clone();
    let concurrency = state.config.bullmq_concurrency.max(1);
    let lock_duration = Duration::from_millis(state.config.bullmq_lock_duration_ms.max(1));
    let stalled_interval = Duration::from_millis(state.config.bullmq_stalled_interval_ms.max(1));
    let max_stalled_count = state.config.bullmq_max_stalled_count.max(1);

    let worker_options = |name: &str, concurrency: usize| {
        WorkerOptions::new()
            .name(name)
            .prefix(&prefix)
            .concurrency(concurrency)
            .lock_duration(lock_duration)
            .stalled_interval(stalled_interval)
            .max_stalled_count(max_stalled_count)
            .connection(RedisConnectionOptions {
                url: redis_url.clone(),
                ..Default::default()
            })
    };

    let webhook_state = state.clone();
    let webhooks = Worker::with_options(
        WEBHOOK_QUEUE,
        move |job: bullmq::Job, _token| {
            let state = webhook_state.clone();
            async move { crate::infra::jobs::process(&state, job).await }
        },
        worker_options(WEBHOOK_QUEUE, concurrency),
    )
    .await
    .map_err(queue_error)?;

    // The bounty queue moves money. One job at a time per process keeps the
    // on-chain call sequence for a repository predictable.
    let bounty_state = state.clone();
    let bounty = Worker::with_options(
        BOUNTY_QUEUE,
        move |job: bullmq::Job, _token| {
            let state = bounty_state.clone();
            async move { crate::infra::jobs::process(&state, job).await }
        },
        worker_options(BOUNTY_QUEUE, 1),
    )
    .await
    .map_err(queue_error)?;

    let sync_state = state.clone();
    let sync = Worker::with_options(
        SYNC_QUEUE,
        move |job: bullmq::Job, _token| {
            let state = sync_state.clone();
            async move { crate::infra::jobs::process(&state, job).await }
        },
        worker_options(SYNC_QUEUE, 1),
    )
    .await
    .map_err(queue_error)?;

    tracing::info!(
        webhooks = WEBHOOK_QUEUE,
        bounty = BOUNTY_QUEUE,
        sync = SYNC_QUEUE,
        concurrency,
        lock_duration_ms = lock_duration.as_millis() as u64,
        stalled_interval_ms = stalled_interval.as_millis() as u64,
        max_stalled_count,
        "BullMQ workers started"
    );

    Ok(Workers {
        workers: vec![webhooks, bounty, sync],
    })
}

/// Register every repeating job.
pub async fn start_scheduler(state: &AppState) -> Result<(), AppError> {
    let interval = Duration::from_secs(state.config.escrow_sync_interval_secs.max(1));
    state.queue.register_schedulers(interval).await?;
    tracing::info!(
        job = JOB_ESCROW_BALANCE_SYNC,
        interval_secs = interval.as_secs(),
        "repeating job scheduler registered"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_unavailable_when_no_queue_is_connected() {
        let queue = QueueInfra::disabled();

        let outcome = queue
            .enqueue_webhook(
                Some("delivery-id"),
                WebhookJobData {
                    event: "issues".to_string(),
                    action: Some("opened".to_string()),
                    payload: serde_json::json!({}),
                },
            )
            .await
            .unwrap();

        assert_eq!(outcome, WebhookEnqueueOutcome::Unavailable);
        assert!(!queue.is_enabled());
    }

    #[tokio::test]
    async fn advancing_an_issue_without_a_queue_is_a_no_op() {
        let queue = QueueInfra::disabled();

        let outcome = queue
            .enqueue_advance_issue(BountyJobData::new(Uuid::now_v7(), "test"))
            .await
            .unwrap();

        assert_eq!(outcome, EnqueueOutcome::Unavailable);
    }

    #[tokio::test]
    async fn stats_report_zeroed_counts_without_a_queue() {
        let stats = QueueInfra::disabled().stats().await.unwrap();

        for key in ["webhooks", "escrow-operations", "sync"] {
            assert_eq!(stats[key]["waiting"], 0, "{key} waiting");
            assert_eq!(stats[key]["failed"], 0, "{key} failed");
        }
    }

    #[test]
    fn bounty_jobs_default_to_a_silent_first_pass() {
        let data = BountyJobData::new(Uuid::now_v7(), "wallet-connected");
        assert_eq!(data.recheck, 0);
        assert_eq!(data.hops, 0);
        assert!(!data.notify);
        assert!(data.notifying().notify);
    }

    #[test]
    fn handoffs_carry_and_eventually_exhaust_the_hop_budget() {
        let mut data = BountyJobData::new(Uuid::now_v7(), "pr-merged");
        assert!(!data.exhausted());

        for _ in 0..MAX_HOPS {
            data = data.next("rules-passed");
            assert!(
                !data.exhausted(),
                "hop {} should still be allowed",
                data.hops
            );
        }

        assert!(data.next("rules-passed").exhausted());
    }

    #[test]
    fn a_handoff_never_re_notifies_the_issue() {
        let data = BountyJobData::new(Uuid::now_v7(), "pr-merged").notifying();
        assert!(data.notify);
        assert!(!data.next("rules-passed").notify);
    }

    #[test]
    fn old_payloads_without_the_optional_fields_still_deserialize() {
        let data: BountyJobData = serde_json::from_value(serde_json::json!({
            "issueId": Uuid::nil(),
            "trigger": "pr-merged",
        }))
        .unwrap();

        assert_eq!(data.recheck, 0);
        assert_eq!(data.hops, 0);
        assert!(!data.notify);
    }

    #[test]
    fn dirty_keys_include_the_bullmq_prefix() {
        let issue_id = Uuid::nil();
        assert_eq!(
            dirty_key("bull", issue_id),
            format!("bull:toss:advance:dirty:{issue_id}")
        );
        assert_ne!(dirty_key("alpha", issue_id), dirty_key("beta", issue_id));
    }
}
