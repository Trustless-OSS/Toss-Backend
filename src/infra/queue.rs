use redis::AsyncCommands;
use serde::{de::DeserializeOwned, Serialize};
use tokio::time::{sleep, Duration};

use crate::{error::AppError, infra::redis::RedisClient};

const WEBHOOKS_QUEUE: &str = "queue:webhooks";
const WEBHOOKS_DEAD_LETTER_QUEUE: &str = "queue:webhooks:dead-letter";
const SYNC_QUEUE: &str = "queue:sync";
const DEDUP_PREFIX: &str = "webhook:delivery:";
const MAX_WEBHOOK_ATTEMPTS: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookEnqueueOutcome {
    Enqueued,
    Duplicate,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct WebhookJobData {
    pub event: String,
    pub action: Option<String>,
    pub payload: serde_json::Value,
    #[serde(default)]
    pub attempts: u8,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct SyncJobData {
    pub name: String,
}

#[derive(Clone)]
pub struct QueueInfra {
    redis: Option<RedisClient>,
}

impl QueueInfra {
    pub fn new(redis: Option<RedisClient>) -> Self {
        Self { redis }
    }

    pub async fn enqueue_webhook(
        &self,
        delivery_id: Option<&str>,
        data: WebhookJobData,
    ) -> Result<WebhookEnqueueOutcome, AppError> {
        let Some(client) = &self.redis else {
            return Ok(WebhookEnqueueOutcome::Unavailable);
        };

        let payload =
            serde_json::to_string(&data).map_err(|error| AppError::internal(error.to_string()))?;

        if let Some(delivery_id) = delivery_id {
            let dedup_key = format!("{DEDUP_PREFIX}{delivery_id}");
            let mut connection = client
                .get_multiplexed_async_connection()
                .await
                .map_err(|error| AppError::internal(error.to_string()))?;
            let duplicate: i64 = redis::cmd("EVAL")
                .arg(
                    "if redis.call('EXISTS', KEYS[1]) == 1 then return 1 end \
                     redis.call('SET', KEYS[1], '1', 'EX', 86400) \
                     redis.call('LPUSH', KEYS[2], ARGV[1]) \
                     return 0",
                )
                .arg(2)
                .arg(&dedup_key)
                .arg(WEBHOOKS_QUEUE)
                .arg(&payload)
                .query_async(&mut connection)
                .await
                .map_err(|error| AppError::internal(error.to_string()))?;
            return Ok(if duplicate == 1 {
                WebhookEnqueueOutcome::Duplicate
            } else {
                WebhookEnqueueOutcome::Enqueued
            });
        }

        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        connection
            .lpush::<_, _, ()>(WEBHOOKS_QUEUE, payload)
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        Ok(WebhookEnqueueOutcome::Enqueued)
    }

    pub async fn enqueue_sync(&self, data: SyncJobData) -> Result<(), AppError> {
        let Some(client) = &self.redis else {
            return Ok(());
        };
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        let payload =
            serde_json::to_string(&data).map_err(|error| AppError::internal(error.to_string()))?;
        connection
            .lpush::<_, _, ()>(SYNC_QUEUE, payload)
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        Ok(())
    }

    pub async fn pop_webhook(&self) -> Result<Option<WebhookJobData>, AppError> {
        self.pop(WEBHOOKS_QUEUE).await
    }

    pub async fn pop_sync(&self) -> Result<Option<SyncJobData>, AppError> {
        self.pop(SYNC_QUEUE).await
    }

    pub async fn stats(&self) -> Result<serde_json::Value, AppError> {
        let Some(client) = &self.redis else {
            return Ok(serde_json::json!({
                "webhooks": default_counts(),
                "escrow-operations": default_counts(),
                "sync": default_counts(),
            }));
        };

        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;

        let webhooks: i64 = connection.llen(WEBHOOKS_QUEUE).await.unwrap_or(0);
        let failed_webhooks: i64 = connection
            .llen(WEBHOOKS_DEAD_LETTER_QUEUE)
            .await
            .unwrap_or(0);
        let sync: i64 = connection.llen(SYNC_QUEUE).await.unwrap_or(0);

        Ok(serde_json::json!({
            "webhooks": { "waiting": webhooks, "active": 0, "completed": 0, "failed": failed_webhooks, "delayed": 0 },
            "escrow-operations": default_counts(),
            "sync": { "waiting": sync, "active": 0, "completed": 0, "failed": 0, "delayed": 0 },
        }))
    }

    async fn dead_letter_webhook(
        &self,
        data: &WebhookJobData,
        error: &AppError,
    ) -> Result<(), AppError> {
        let Some(client) = &self.redis else {
            return Ok(());
        };
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        let payload = serde_json::to_string(&serde_json::json!({
            "job": data,
            "error": error.to_string(),
            "failedAt": chrono::Utc::now(),
        }))
        .map_err(|error| AppError::internal(error.to_string()))?;
        connection
            .lpush::<_, _, ()>(WEBHOOKS_DEAD_LETTER_QUEUE, payload)
            .await
            .map_err(|error| AppError::internal(error.to_string()))
    }

    async fn pop<T>(&self, queue: &str) -> Result<Option<T>, AppError>
    where
        T: DeserializeOwned,
    {
        let Some(client) = &self.redis else {
            return Ok(None);
        };
        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        let payload: Option<String> = redis::cmd("RPOP")
            .arg(queue)
            .query_async(&mut connection)
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        payload
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|error| AppError::internal(error.to_string()))
    }
}

fn default_counts() -> serde_json::Value {
    serde_json::json!({
        "waiting": 0,
        "active": 0,
        "completed": 0,
        "failed": 0,
        "delayed": 0
    })
}

pub async fn start_workers(state: crate::state::AppState) {
    let webhook_state = state.clone();
    tokio::spawn(async move {
        loop {
            match webhook_state.queue.pop_webhook().await {
                Ok(Some(job)) => {
                    if let Err(error) = crate::modules::github::webhook::process_webhook_job(
                        &webhook_state,
                        job.clone(),
                    )
                    .await
                    {
                        if job.attempts < MAX_WEBHOOK_ATTEMPTS {
                            let mut retry = job;
                            retry.attempts += 1;
                            let delay = Duration::from_secs(1 << retry.attempts);
                            tracing::warn!(
                                %error,
                                attempt = retry.attempts,
                                ?delay,
                                "webhook job failed; retrying"
                            );
                            sleep(delay).await;
                            if let Err(enqueue_error) =
                                webhook_state.queue.enqueue_webhook(None, retry).await
                            {
                                tracing::error!(%enqueue_error, "failed to requeue webhook job");
                            }
                        } else {
                            tracing::error!(%error, attempts = job.attempts, "webhook job moved to dead-letter queue");
                            if let Err(dead_letter_error) =
                                webhook_state.queue.dead_letter_webhook(&job, &error).await
                            {
                                tracing::error!(%dead_letter_error, "failed to persist dead-letter webhook job");
                            }
                        }
                    }
                }
                Ok(None) => sleep(Duration::from_millis(500)).await,
                Err(error) => tracing::error!(%error, "webhook queue pop failed"),
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

    #[tokio::test]
    async fn reports_unavailable_when_queue_has_no_redis_client() {
        let queue = QueueInfra::new(None);
        let outcome = queue
            .enqueue_webhook(
                Some("delivery-id"),
                WebhookJobData {
                    event: "issues".to_string(),
                    action: Some("opened".to_string()),
                    payload: serde_json::json!({}),
                    attempts: 0,
                },
            )
            .await
            .unwrap();

        assert_eq!(outcome, WebhookEnqueueOutcome::Unavailable);
    }

    #[test]
    fn old_queued_jobs_default_attempts_to_zero() {
        let job: WebhookJobData = serde_json::from_value(serde_json::json!({
            "event": "issues",
            "action": "labeled",
            "payload": {}
        }))
        .unwrap();

        assert_eq!(job.attempts, 0);
    }
}
