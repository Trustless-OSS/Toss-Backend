use std::{sync::Arc, time::Duration};

use bullmq::{
    options::{QueueOptions, RedisConnectionOptions, WorkerOptions},
    types::{BackoffStrategy, KeepJobs, RemoveOnFinish},
    JobOptions, Queue, Worker,
};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::AppError, infra::redis::RedisClient, state::AppState};

pub const WEBHOOK_QUEUE: &str = "toss-webhooks";
pub const BOUNTY_QUEUE: &str = "toss-bounty";
pub const SYNC_QUEUE: &str = "toss-sync";

pub const JOB_GITHUB_WEBHOOK: &str = "github-webhook";
pub const JOB_ADVANCE_ISSUE: &str = "advance-issue";
pub const JOB_PUSH_MILESTONE: &str = "push-milestone";
pub const JOB_RELEASE_PAYOUT: &str = "release-payout";
pub const JOB_ESCROW_BALANCE_SYNC: &str = "escrow-balance-sync";

const WEBHOOK_ATTEMPTS: u32 = 5;
const BOUNTY_ATTEMPTS: u32 = 5;

const WEBHOOK_BACKOFF_MS: u64 = 2_000;
const BOUNTY_BACKOFF_MS: u64 = 5_000;

const WEBHOOK_KEEP_COMPLETED_MS: u64 = 24 * 60 * 60 * 1_000;
const KEEP_FAILED_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

const DIRTY_FLAG_SUFFIX: &str = "toss:advance:dirty:";
const DIRTY_FLAG_TTL_SECS: u64 = 3_600;
const ENQUEUE_LOCK_SUFFIX: &str = "toss:enqueue:";
const ENQUEUE_LOCK_TTL_MS: u64 = 5_000;
const ENQUEUE_LOCK_RETRIES: u32 = 40;
const ENQUEUE_LOCK_WAIT: Duration = Duration::from_millis(25);

const WORKER_CLOSE_TIMEOUT_MS: u64 = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookJobData {
    pub event: String,
    pub action: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BountyJobData {
    pub issue_id: Uuid,
    pub trigger: String,
    #[serde(default)]
    pub recheck: u32,
    #[serde(default)]
    pub notify: bool,
    #[serde(default)]
    pub hops: u32,
}

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

    pub fn next(&self, trigger: impl Into<String>) -> Self {
        Self {
            issue_id: self.issue_id,
            trigger: trigger.into(),
            recheck: self.recheck,
            notify: false,
            hops: self.hops + 1,
        }
    }

    pub fn exhausted(&self) -> bool {
        self.hops > MAX_HOPS
    }

    pub fn notifying(mut self) -> Self {
        self.notify = true;
        self
    }

    pub fn with_recheck(mut self, recheck: u32) -> Self {
        self.recheck = recheck;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookEnqueueOutcome {
    Enqueued,
    Duplicate,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueOutcome {
    Enqueued,
    AlreadyPending,
    Promoted,
    Coalesced,
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

struct Queues {
    webhooks: Queue,
    bounty: Queue,
    sync: Queue,
}

#[derive(Clone)]
pub struct QueueInfra {
    queues: Option<Arc<Queues>>,
    redis: Option<RedisClient>,
    prefix: String,
}

impl QueueInfra {
    pub fn disabled() -> Self {
        Self {
            queues: None,
            redis: None,
            prefix: String::new(),
        }
    }

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
            // no delivery id — cannot dedupe
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

    pub async fn enqueue_advance_issue(
        &self,
        data: BountyJobData,
    ) -> Result<EnqueueOutcome, AppError> {
        self.enqueue_bounty_job(JOB_ADVANCE_ISSUE, data, None).await
    }

    pub async fn enqueue_advance_issue_after(
        &self,
        data: BountyJobData,
        delay: Duration,
    ) -> Result<EnqueueOutcome, AppError> {
        self.enqueue_bounty_job(JOB_ADVANCE_ISSUE, data, Some(delay))
            .await
    }

    pub async fn enqueue_push_milestone(
        &self,
        data: BountyJobData,
    ) -> Result<EnqueueOutcome, AppError> {
        self.enqueue_bounty_job(JOB_PUSH_MILESTONE, data, None)
            .await
    }

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
                // free job id so a later event can enqueue
                queues.bounty.remove(&job_id).await.map_err(queue_error)?;
            }
            bullmq::JobState::Delayed if delay.is_none() => {
                // real event — promote delayed recheck
                if let Some(job) = queues.bounty.get_job(&job_id).await.map_err(queue_error)? {
                    job.promote().await.map_err(queue_error)?;
                }
                return Ok(EnqueueOutcome::Promoted);
            }
            bullmq::JobState::Active => {
                // in-flight job will re-evaluate
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

        // lock unavailable — still enqueue rather than drop
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

    pub async fn drain_dirty_advance(&self, issue_id: Uuid) -> Result<EnqueueOutcome, AppError> {
        if !self.take_dirty(issue_id).await? {
            return Ok(EnqueueOutcome::AlreadyPending);
        }
        self.enqueue_advance_issue(BountyJobData::new(issue_id, "dirty-drain"))
            .await
    }

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

    // [ryzen-xp] : Drop repeating escrow-balance-sync — balance syncs on fund/release only
    pub async fn register_schedulers(&self, _interval: Duration) -> Result<(), AppError> {
        let Some(queues) = self.queues.as_ref() else {
            return Ok(());
        };

        match queues
            .sync
            .remove_job_scheduler(JOB_ESCROW_BALANCE_SYNC)
            .await
        {
            Ok(removed) => {
                if removed {
                    tracing::info!(
                        job = JOB_ESCROW_BALANCE_SYNC,
                        "removed repeating escrow-balance-sync scheduler"
                    );
                }
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    job = JOB_ESCROW_BALANCE_SYNC,
                    "could not remove escrow-balance-sync scheduler (may already be gone)"
                );
            }
        }

        Ok(())
    }

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

pub struct Workers {
    workers: Vec<Worker>,
}

impl Workers {
    pub async fn shutdown(self) {
        for worker in &self.workers {
            if let Err(error) = worker.close(WORKER_CLOSE_TIMEOUT_MS).await {
                tracing::error!(%error, "failed to close a queue worker cleanly");
            }
        }
        tracing::info!("background workers stopped");
    }
}

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

    // concurrency=1: serialize on-chain bounty jobs
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

pub async fn start_scheduler(state: &AppState) -> Result<(), AppError> {
    // Interval config is ignored: continuous balance polling was removed.
    let interval = Duration::from_secs(state.config.escrow_sync_interval_secs.max(1));
    state.queue.register_schedulers(interval).await?;
    tracing::info!(
        job = JOB_ESCROW_BALANCE_SYNC,
        "repeating escrow-balance-sync disabled (sync runs on fund/release only)"
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
