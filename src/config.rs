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
    pub stellar_network: String,
    pub app_url: String,
    pub webhook_url: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self, crate::error::AppError> {
        let manifest_env = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
        if dotenvy::from_path(&manifest_env).is_err() {
            let _ = dotenvy::dotenv();
        }

        Ok(Self {
            port: std::env::var("PORT")
                .ok()
                .and_then(|value| value.parse::<u16>().ok())
                .unwrap_or(5000),
            log_level: std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
            node_env: std::env::var("NODE_ENV").unwrap_or_else(|_| "development".to_string()),
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
            trustless_work_base_url: std::env::var("TRUSTLESS_WORK_BASE_URL")
                .unwrap_or_else(|_| "https://dev.api.trustlesswork.com".to_string()),
            stellar_network: std::env::var("STELLAR_NETWORK")
                .unwrap_or_else(|_| "testnet".to_string()),
            app_url: std::env::var("APP_URL")
                .unwrap_or_else(|_| "http://localhost:3000".to_string()),
            webhook_url: std::env::var("WEBHOOK_URL").ok(),
        })
    }

    pub fn is_mainnet(&self) -> bool {
        self.stellar_network.eq_ignore_ascii_case("mainnet")
    }
}

fn required_env(name: &str) -> Result<String, crate::error::AppError> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(crate::error::AppError::internal(format!(
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
            crate::error::AppError::internal(format!(
                "Missing required environment variable: {}",
                names.join(" or ")
            ))
        })
}
