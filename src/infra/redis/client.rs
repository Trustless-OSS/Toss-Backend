use crate::error::AppError;
use redis::Client;

pub type RedisClient = Client;

pub fn build_client(redis_url: &str) -> Result<RedisClient, AppError> {
    Client::open(redis_url).map_err(|error| AppError::database(error.to_string()))
}
