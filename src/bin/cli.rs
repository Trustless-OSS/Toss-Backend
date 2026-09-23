use toasty_cli::{Config, ToastyCli};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let manifest_env = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    if dotenvy::from_path(&manifest_env).is_err() {
        let _ = dotenvy::dotenv();
    }

    let database_url = std::env::var("TOASTY_CONNECTION_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .map_err(|_| anyhow::anyhow!("DATABASE_URL or TOASTY_CONNECTION_URL must be set"))?;

    let config = Config::load()?;

    let db = toasty::Db::builder()
        .models(toasty::models!(
            toss_backend::shared::models::schema::Activity,
            toss_backend::shared::models::schema::Bounty,
            toss_backend::shared::models::schema::EscrowFunder,
            toss_backend::shared::models::schema::FailedTask,
            toss_backend::shared::models::schema::Notification,
            toss_backend::shared::models::schema::Profile,
            toss_backend::shared::models::schema::RepoMaintainer,
            toss_backend::shared::models::schema::Repositories,
            toss_backend::shared::models::schema::Reward,
            toss_backend::shared::models::schema::Wallet,
        ))
        .connect(&database_url)
        .await?;

    ToastyCli::with_config(db, config).parse_and_run().await?;
    Ok(())
}
