use crate::error::AppError;

const DEFAULT_ALLOWED_ORIGINS: [&str; 1] = ["http://localhost:3000"];

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub log_level: String,
    pub node_env: String,
    pub database_url: String,
    pub redis_url: String,
    /// Redis key prefix for every BullMQ queue (`bull` matches the BullMQ default).
    pub bullmq_prefix: String,
    /// How many jobs each BullMQ worker processes concurrently.
    pub bullmq_concurrency: usize,
    /// How long a worker holds a job lock before it can be considered stalled.
    pub bullmq_lock_duration_ms: u64,
    /// How often workers scan for stalled `active` jobs.
    pub bullmq_stalled_interval_ms: u64,
    /// How many times a stalled job is re-queued before it is failed.
    pub bullmq_max_stalled_count: u32,
    /// Interval of the repeating `escrow-balance-sync` job scheduler.
    pub escrow_sync_interval_secs: u64,
    pub supabase_url: String,
    pub supabase_auth_api_key: String,
    pub github_app_id: String,
    pub github_app_private_key: String,
    pub github_bot_token: Option<String>,
    pub github_webhook_secret: String,
    pub platform_stellar_public_key: String,
    pub platform_stellar_secret_key: String,
    pub dispute_resolver_stellar_public_key: String,
    pub dispute_resolver_stellar_secret_key: String,
    pub trustless_work_api_key: String,
    pub trustless_work_base_url: String,
    pub token_address: String,
    pub stellar_network: String,
    pub app_url: String,
    pub webhook_url: Option<String>,
    pub dev_webhook_proxy_enabled: bool,
    pub smee_source_url: String,
    pub smee_target_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, AppError> {
        let manifest_env = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
        if dotenvy::from_path(&manifest_env).is_err() {
            let _ = dotenvy::dotenv();
        }

        let port = required_u16("PORT")?;

        let node_env = required_env("NODE_ENV")?;

        let dev_webhook_proxy_enabled = node_env.eq_ignore_ascii_case("development")
            && required_bool("DEV_WEBHOOK_PROXY_ENABLED")?;

        Ok(Self {
            port,
            log_level: required_env("LOG_LEVEL")?,
            node_env,
            database_url: required_env("DATABASE_URL")?,
            redis_url: required_env("REDIS_URL")?,
            bullmq_prefix: optional_env("BULLMQ_PREFIX").unwrap_or_else(|| "bull".to_string()),
            bullmq_concurrency: optional_parsed("BULLMQ_CONCURRENCY")?.unwrap_or(4),
            bullmq_lock_duration_ms: optional_parsed("BULLMQ_LOCK_DURATION_MS")?.unwrap_or(30_000),
            bullmq_stalled_interval_ms: optional_parsed("BULLMQ_STALLED_INTERVAL_MS")?
                .unwrap_or(30_000),
            bullmq_max_stalled_count: optional_parsed("BULLMQ_MAX_STALLED_COUNT")?.unwrap_or(1),
            escrow_sync_interval_secs: optional_parsed("ESCROW_SYNC_INTERVAL_SECS")?.unwrap_or(60),
            supabase_url: required_env("SUPABASE_URL")?,
            supabase_auth_api_key: first_env(&[
                "SUPABASE_PUBLISHABLE_KEY",
                "SUPABASE_ANON_KEY",
                "NEXT_PUBLIC_SUPABASE_ANON_KEY",
                "SUPABASE_SERVICE_ROLE_KEY",
            ])?,
            github_app_id: required_env("GITHUB_APP_ID")?,
            github_app_private_key: required_env("GITHUB_APP_PRIVATE_KEY")?,
            github_bot_token: std::env::var("GITHUB_BOT_TOKEN").ok(),
            github_webhook_secret: required_env("GITHUB_WEBHOOK_SECRET")?,
            platform_stellar_public_key: required_env("PLATFORM_STELLAR_PUBLIC_KEY")?,
            platform_stellar_secret_key: required_env("PLATFORM_STELLAR_SECRET_KEY")?,
            dispute_resolver_stellar_public_key: required_env(
                "DISPUTE_RESOLVER_STELLAR_PUBLIC_KEY",
            )?,
            dispute_resolver_stellar_secret_key: required_env(
                "DISPUTE_RESOLVER_STELLAR_SECRET_KEY",
            )?,
            trustless_work_api_key: required_env("TRUSTLESS_WORK_API_KEY")?,
            trustless_work_base_url: required_env("TRUSTLESS_WORK_BASE_URL")?,
            token_address: required_env("TOKEN_ADDRESS")?,
            stellar_network: required_env("STELLAR_NETWORK")?,
            app_url: required_env("APP_URL")?,
            webhook_url: std::env::var("WEBHOOK_URL").ok(),
            dev_webhook_proxy_enabled,
            smee_source_url: required_env("SMEE_SOURCE_URL")?,
            smee_target_url: required_env("SMEE_TARGET_URL")?,
        })
    }

    pub fn is_mainnet(&self) -> bool {
        self.stellar_network.eq_ignore_ascii_case("mainnet")
    }
}

fn optional_env(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        _ => None,
    }
}

/// Read an optional environment variable and parse it, rejecting malformed values
/// instead of silently falling back to the default.
fn optional_parsed<T>(name: &str) -> Result<Option<T>, crate::error::AppError>
where
    T: std::str::FromStr,
{
    match optional_env(name) {
        None => Ok(None),
        Some(value) => value.parse::<T>().map(Some).map_err(|_| {
            crate::error::AppError::env_var_error(format!(
                "Invalid value for environment variable: {name}"
            ))
        }),
    }
}

fn optional_bool(name: &str) -> Option<bool> {
    std::env::var(name)
        .ok()
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
}

fn required_bool(name: &str) -> Result<bool, crate::error::AppError> {
    let _value = required_env(name)?;
    optional_bool(name).ok_or_else(|| {
        crate::error::AppError::env_var_error(format!(
            "Invalid boolean value for environment variable: {name}"
        ))
    })
}

fn required_u16(name: &str) -> Result<u16, crate::error::AppError> {
    let value = required_env(name)?;
    value.parse::<u16>().map_err(|_| {
        crate::error::AppError::env_var_error(format!(
            "Invalid u16 value for environment variable: {name}"
        ))
    })
}

fn required_env(name: &str) -> Result<String, crate::error::AppError> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(crate::error::AppError::env_var_error(format!(
            "Missing required environment variable: {name}"
        ))),
    }
}

fn first_env(names: &[&str]) -> Result<String, crate::error::AppError> {
    names
        .iter()
        .find_map(|name| match std::env::var(name) {
            Ok(value) if !value.trim().is_empty() => Some(value),
            _ => None,
        })
        .ok_or_else(|| {
            crate::error::AppError::env_var_error(format!(
                "Missing required environment variable: {}",
                names.join(" or ")
            ))
        })
}

pub fn parse_cors_allowed_origins(raw: &str) -> Vec<String> {
    let values: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| entry.to_string())
        .collect();

    if values.is_empty() {
        DEFAULT_ALLOWED_ORIGINS
            .into_iter()
            .map(str::to_string)
            .collect()
    } else {
        values
    }
}

#[cfg(test)]
mod tests {
    use super::parse_cors_allowed_origins;

    #[test]
    fn parses_comma_separated_allowed_origins() {
        let origins = parse_cors_allowed_origins(
            "https://trustless-oss.vercel.app, https://www.trustless-oss.vercel.app, http://localhost:3000",
        );

        assert_eq!(
            origins,
            vec![
                "https://trustless-oss.vercel.app".to_string(),
                "https://www.trustless-oss.vercel.app".to_string(),
                "http://localhost:3000".to_string(),
            ]
        );
    }

    #[test]
    fn falls_back_to_default_frontend_origins_when_env_missing() {
        let origins = parse_cors_allowed_origins("");

        assert_eq!(origins, vec!["http://localhost:3000".to_string()]);
    }

    #[test]
    fn uses_app_url_when_cors_env_is_missing() {
        let origins = parse_cors_allowed_origins("http://localhost:3000");

        assert_eq!(origins, vec!["http://localhost:3000".to_string()]);
    }
}
