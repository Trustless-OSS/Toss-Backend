use redis::AsyncCommands;
use serde::{de::DeserializeOwned, Serialize};
use tokio::time::{sleep, Duration};

use crate::{error::AppError, infra::redis::RedisClient};

const WEBHOOKS_QUEUE: &str = "queue:webhooks";
const SYNC_QUEUE: &str = "queue:sync";
const DEDUP_PREFIX: &str = "webhook:delivery:";

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct WebhookJobData {
    pub event: String,
    pub action: Option<String>,
    pub payload: serde_json::Value,
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
    ) -> Result<bool, AppError> {
        let Some(client) = &self.redis else {
            return Ok(false);
        };

        if let Some(delivery_id) = delivery_id {
            let dedup_key = format!("{DEDUP_PREFIX}{delivery_id}");
            let mut connection = client
                .get_multiplexed_async_connection()
                .await
                .map_err(|error| AppError::internal(error.to_string()))?;
            let inserted: bool = redis::cmd("SET")
                .arg(&dedup_key)
                .arg("1")
                .arg("NX")
                .arg("EX")
                .arg(86_400)
                .query_async(&mut connection)
                .await
                .map_err(|error| AppError::internal(error.to_string()))?;
            if !inserted {
                return Ok(true);
            }
        }

        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        let payload =
            serde_json::to_string(&data).map_err(|error| AppError::internal(error.to_string()))?;
        connection
            .lpush::<_, _, ()>(WEBHOOKS_QUEUE, payload)
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        Ok(false)
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
        let sync: i64 = connection.llen(SYNC_QUEUE).await.unwrap_or(0);

        Ok(serde_json::json!({
            "webhooks": { "waiting": webhooks, "active": 0, "completed": 0, "failed": 0, "delayed": 0 },
            "escrow-operations": default_counts(),
            "sync": { "waiting": sync, "active": 0, "completed": 0, "failed": 0, "delayed": 0 },
        }))
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
                    if let Err(error) =
                        crate::modules::github::webhook::process_webhook_job(&webhook_state, job)
                            .await
                    {
                        tracing::error!(%error, "webhook job failed");
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
