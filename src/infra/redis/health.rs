use super::client::RedisClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedisHealthStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, Copy)]
pub struct RedisHealth {
    pub status: RedisHealthStatus,
}

pub async fn check_health(client: Option<&RedisClient>) -> RedisHealth {
    let Some(client) = client else {
        return RedisHealth {
            status: RedisHealthStatus::Error,
        };
    };

    match client.get_multiplexed_async_connection().await {
        Ok(mut connection) => {
            let ping = redis::cmd("PING")
                .query_async::<String>(&mut connection)
                .await;
            if ping.is_ok() {
                RedisHealth {
                    status: RedisHealthStatus::Ok,
                }
            } else {
                RedisHealth {
                    status: RedisHealthStatus::Error,
                }
            }
        }
        Err(_) => RedisHealth {
            status: RedisHealthStatus::Error,
        },
    }
}
