use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub log_level: String,
    pub node_env: String,
    pub database_url: String,
    pub redis_url: String,
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
