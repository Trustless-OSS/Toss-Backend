use crate::{
    config::Config,
    error::AppError,
    infra::{cache::Cache, db, email::resend_provider, queue::QueueInfra, redis},
};
use reqwest::Client;
use resend_rs::Resend;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: toasty::Db,
    pub redis: Option<redis::RedisClient>,
    pub http_client: Client,
    pub cache: Cache,
    pub queue: QueueInfra,
    pub email: Resend,
}

impl AppState {
    pub async fn new(config: Config) -> Result<Self, AppError> {
        let db = db::connect(&config.database_url).await?;
        let redis = Some(redis::build_client(&config.redis_url)?);
        let cache = Cache::new(redis.clone());

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

        let email = resend_provider::connect(&config.resend_api_key).await?;

        Ok(Self {
            config: Arc::new(config),
            db,
            redis,
            http_client: Client::new(),
            cache,
            queue,
            email,
        })
    }
}
