//! Project-local Toasty migration CLI.
//!
//! Run from the crate root (so `Toasty.toml` is found):
//!
//! ```bash
//! cargo run --bin migrate -- migration generate --name initial
//! cargo run --bin migrate -- migration apply
//! ```

use toasty_cli::{Config, ToastyCli};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env the same way the server does (DATABASE_URL).
    let manifest_env = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    if dotenvy::from_path(&manifest_env).is_err() {
        let _ = dotenvy::dotenv();
    }

    let database_url = std::env::var("TOASTY_CONNECTION_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .map_err(|_| anyhow::anyhow!("DATABASE_URL or TOASTY_CONNECTION_URL must be set"))?;

    let config = Config::load()?;
    let db = toss_backend::infra::db::connect(&database_url)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    ToastyCli::with_config(db, config).parse_and_run().await?;
    Ok(())
}
