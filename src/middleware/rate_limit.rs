use governor::middleware::NoOpMiddleware;
use std::{sync::Arc, time::Duration};
use tower_governor::{
    governor::{GovernorConfig, GovernorConfigBuilder},
    key_extractor::SmartIpKeyExtractor,
};

use crate::{error::AppError, state::AppState};

pub type IpLimiterConfig = GovernorConfig<SmartIpKeyExtractor, NoOpMiddleware>;

const USER_LIMIT_SCRIPT: &str = r#"
local count = redis.call('INCR', KEYS[1])
if count == 1 or redis.call('TTL', KEYS[1]) < 0 then
    redis.call('EXPIRE', KEYS[1], ARGV[1])
end
return count
"#;

pub fn ip_limiter_config(refill_sec: u64, burst: u32) -> Arc<IpLimiterConfig> {
    let config = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(refill_sec)
            .burst_size(burst)
            .key_extractor(SmartIpKeyExtractor)
            .finish()
            .expect("[RL]: refill period and burst size must be non-zero"),
    );

    let limiter = config.limiter().clone();

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            limiter.retain_recent();
        }
    });

    config
}

/// Applies a fixed-window, per-user limit shared by all application instances.
pub async fn check_user_limit(
    state: &AppState,
    scope: &str,
    github_id: i64,
    max: u32,
    window_secs: u64,
    fail_open: bool,
) -> Result<(), AppError> {
    if max == 0 || window_secs == 0 {
        return Err(AppError::internal(
            "[RL]: maximum and window must be non-zero",
        ));
    }

    match incr_window(state, scope, github_id, window_secs).await {
        Ok(count) if count > max => Err(AppError::too_many_requests(format!(
            "Rate limit exceeded for {scope}. Try again in a moment."
        ))),
        Ok(_) => Ok(()),
        Err(error) if fail_open => {
            tracing::warn!(%error, scope, "user rate limit unavailable; failing open");
            Ok(())
        }
        Err(error) => Err(error),
    }
}

async fn incr_window(
    state: &AppState,
    scope: &str,
    github_id: i64,
    window_secs: u64,
) -> Result<u32, AppError> {
    let client = state
        .redis
        .as_ref()
        .ok_or_else(|| AppError::internal("[RL]: Redis is unavailable"))?;
    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .map_err(|error| AppError::internal(format!("[RL] Redis connection failed: {error}")))?;

    let key = format!("{}:[RL]:{scope}:{github_id}", state.config.bullmq_prefix);
    redis::cmd("EVAL")
        .arg(USER_LIMIT_SCRIPT)
        .arg(1)
        .arg(key)
        .arg(window_secs)
        .query_async(&mut connection)
        .await
        .map_err(|error| AppError::internal(format!("[RL] Redis command failed: {error}")))
}
