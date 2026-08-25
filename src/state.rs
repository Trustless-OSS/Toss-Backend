use std::sync::Arc;

use reqwest::Client;

use crate::{
    config::Config,
    error::AppError,
    infra::{cache::Cache, db, queue::QueueInfra, redis},
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: toasty::Db,
    pub redis: Option<redis::RedisClient>,
    pub http_client: Client,
    pub cache: Cache,
    pub queue: QueueInfra,
}

impl AppState {
    pub async fn new(config: Config) -> Result<Self, AppError> {
        let db = db::connect(&config.database_url).await?;
        let redis = Some(redis::build_client(&config.redis_url)?);
        let cache = Cache::new(redis.clone());

        // A queue that cannot connect must not take the API down with it: fall
        // back to a disabled hub, which makes webhook routes process inline the
        // same way they did before Redis was reachable.
        let queue = match QueueInfra::connect(
            &config.redis_url,
            &config.bullmq_prefix,
            redis.clone(),
        )
        .await
        {
            Ok(queue) => queue,
            Err(error) => {
                tracing::error!(%error, "failed to connect BullMQ; background jobs are disabled");
                QueueInfra::disabled()
            }
        };

        Ok(Self {
            config: Arc::new(config),
            db,
            redis,
            http_client: Client::new(),
            cache,
            queue,
        })
    }
}
