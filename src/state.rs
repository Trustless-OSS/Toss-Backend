use std::sync::Arc;

use reqwest::Client;

use crate::{
    config::Config,
    error::AppError,
    infra::{cache::Cache, db, db::DbPool, queue::QueueInfra, redis},
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Option<DbPool>,
    pub redis: Option<redis::RedisClient>,
    pub http_client: Client,
    pub cache: Cache,
    pub queue: QueueInfra,
}

impl AppState {
    pub fn new(config: Config) -> Result<Self, AppError> {
        let db = Some(db::connect_lazy(&config.database_url));
        let redis = Some(redis::build_client(&config.redis_url)?);
        let cache = Cache::new(redis.clone());
        let queue = QueueInfra::new(redis.clone(), &config);

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
