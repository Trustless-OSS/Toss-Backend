use crate::{
    config::Config,
    error::AppError,
    infra::{db, email::resend_provider, queue::QueueInfra, redis, s3_client},
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
    pub cache: redis::Cache,
    pub queue: QueueInfra,
    pub email: Resend,
    pub s3_client: aws_sdk_s3::Client,
}

impl AppState {
    pub async fn new(config: Config) -> Result<Self, AppError> {
        let db = db::connect(&config.database_url).await?;
        let redis = Some(redis::build_client(&config.redis_url)?);
        let cache = redis::Cache::new(redis.clone());
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
        let s3_client =
            s3_client::client::client(&config.aws_region, config.s3_endpoint_url.as_deref()).await;

        Ok(Self {
            config: Arc::new(config),
            db,
            redis,
            http_client: Client::new(),
            cache,
            queue,
            email,
            s3_client,
        })
    }
}
