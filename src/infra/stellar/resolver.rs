use crate::config::Config;

pub fn dispute_resolver_public_key(config: &Config) -> &str {
    &config.dispute_resolver_stellar_public_key
}

pub fn dispute_resolver_secret_key(config: &Config) -> &str {
    &config.dispute_resolver_stellar_secret_key
}
