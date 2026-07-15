use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::config::Config;

pub fn init(config: &Config) {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| format!("toss_backend={},tower_http=info", config.log_level).into());

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}
