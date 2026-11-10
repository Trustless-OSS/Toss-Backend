//! Uses only a disposable database explicitly named toss_security_tests.
use rust_decimal::Decimal;
use std::sync::Arc;
use toss_backend::{
    config::Config,
    infra,
    modules::{github::repository, repo::repository as repos},
    shared::models::schema,
    state::AppState,
};
use uuid::Uuid;

async fn state() -> AppState {
    let url = std::env::var("SECURITY_TEST_DATABASE_URL").expect("disposable PostgreSQL required");
    assert!(url.ends_with("/toss_security_tests"));
    let db = infra::db::connect(&url).await.unwrap();
    let config = Config {
        port: 0,
        log_level: "error".into(),
        node_env: "test".into(),
        database_url: url,
        redis_url: String::new(),
        bullmq_prefix: "reinstall-test".into(),
        bullmq_concurrency: 1,
        supabase_url: String::new(),
        supabase_auth_api_key: String::new(),
        github_app_id: String::new(),
        github_app_private_key: String::new(),
        github_webhook_secret: String::new(),
        platform_stellar_public_key: String::new(),
        platform_stellar_secret_key: String::new(),
        dispute_resolver_stellar_public_key: String::new(),
        dispute_resolver_stellar_secret_key: String::new(),
        trustless_work_api_key: String::new(),
        trustless_work_base_url: String::new(),
        trustless_work_health_escrow_contract: String::new(),
        token_address: String::new(),
        stellar_network: "testnet".into(),
        app_url: String::new(),
        webhook_url: None,
        dev_webhook_proxy_enabled: false,
        auth_bypass: false,
        smee_source_url: String::new(),
        smee_target_url: String::new(),
        resend_api_key: String::new(),
        email_from: String::new(),
        aws_access_key_id: String::new(),
        aws_secret_access_key: String::new(),
        aws_region: "us-east-1".into(),
        s3_endpoint_url: None,
        s3_bucket_name: String::new(),
    };
    AppState {
        config: Arc::new(config),
        db,
        redis: None,
        cache: infra::redis::Cache::new(None),
        queue: infra::queue::QueueInfra::disabled(),
        http_client: reqwest::Client::new(),
        email: resend_rs::Resend::new("test"),
        s3_client: aws_sdk_s3::Client::from_conf(
            aws_sdk_s3::config::Builder::new()
                .behavior_version_latest()
                .region(aws_sdk_s3::config::Region::new("us-east-1"))
                .build(),
        ),
    }
}

#[tokio::test]
#[ignore = "requires SECURITY_TEST_DATABASE_URL pointing to disposable toss_security_tests"]
async fn reinstall_preserves_funded_escrow_rewards_and_original_repo_id() {
    let state = state().await;
    let mut db = state.db.clone();
    let unique = Uuid::new_v4();
    let github_repo_id = (unique.as_u128() & i64::MAX as u128) as i64;
    let github_user_id = github_repo_id + 1;
    let login = format!("test-{}", unique.simple());
    let first_installation = github_repo_id + 2;
    let second_installation = github_repo_id + 3;
    let original = schema::Repository::create()
        .github_repo_id(github_repo_id)
        .full_name(format!("owner/repo-{unique}"))
        .github_install_id(Some(first_installation))
        .escrow_contract_id(Some("CFUNDEDESCROW".to_string()))
        .escrow_balance(Some(Decimal::from(75)))
        .exec(&mut db)
        .await
        .unwrap();
    schema::Reward::upsert_by_repo_id_and_label(original.id, "high")
        .amount(Decimal::from(75))
        .exec(&mut db)
        .await
        .unwrap();
    repository::upsert_installation_repo(
        &state,
        github_repo_id,
        &original.full_name,
        github_user_id,
        &login,
        first_installation,
    )
    .await
    .unwrap();
    assert!(repos::is_maintainer(&state, github_user_id, original.id)
        .await
        .unwrap());

    repository::detach_repos_by_installation_id(&state, first_installation)
        .await
        .unwrap();
    let detached = schema::Repository::filter_by_github_repo_id(github_repo_id)
        .first()
        .exec(&mut db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detached.id, original.id);
    assert_eq!(
        detached.escrow_contract_id.as_deref(),
        Some("CFUNDEDESCROW")
    );
    assert_eq!(detached.escrow_balance, Some(Decimal::from(75)));
    assert_eq!(detached.github_install_id, None);
    assert!(!repos::is_maintainer(&state, github_user_id, original.id)
        .await
        .unwrap());

    repository::upsert_installation_repo(
        &state,
        github_repo_id,
        &original.full_name,
        github_user_id,
        &login,
        second_installation,
    )
    .await
    .unwrap();
    repository::detach_repo_from_installation(&state, github_repo_id, first_installation)
        .await
        .unwrap();
    let restored = schema::Repository::filter_by_github_repo_id(github_repo_id)
        .first()
        .exec(&mut db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.id, original.id);
    assert_eq!(restored.github_install_id, Some(second_installation));
    assert_eq!(
        restored.escrow_contract_id.as_deref(),
        Some("CFUNDEDESCROW")
    );
    assert_eq!(restored.escrow_balance, Some(Decimal::from(75)));
    assert!(repos::is_maintainer(&state, github_user_id, original.id)
        .await
        .unwrap());
    let reward = schema::Reward::filter_by_repo_id(original.id)
        .exec(&mut db)
        .await
        .unwrap();
    assert_eq!(
        reward.iter().find(|r| r.label == "high").unwrap().amount,
        Decimal::from(75)
    );
}

#[tokio::test]
#[ignore = "requires SECURITY_TEST_DATABASE_URL pointing to disposable toss_security_tests"]
async fn removing_one_repo_keeps_its_data_and_other_repos_installed() {
    let state = state().await;
    let mut db = state.db.clone();
    let unique = Uuid::new_v4();
    let installation = (unique.as_u128() & i64::MAX as u128) as i64;
    let first = schema::Repository::create()
        .github_repo_id(installation + 1)
        .full_name(format!("owner/first-{unique}"))
        .github_install_id(Some(installation))
        .escrow_contract_id(Some("CFUNDED".to_string()))
        .exec(&mut db)
        .await
        .unwrap();
    let second = schema::Repository::create()
        .github_repo_id(installation + 2)
        .full_name(format!("owner/second-{unique}"))
        .github_install_id(Some(installation))
        .exec(&mut db)
        .await
        .unwrap();
    repository::detach_repo_from_installation(&state, first.github_repo_id, installation)
        .await
        .unwrap();
    let first = schema::Repository::filter_by_id(first.id)
        .first()
        .exec(&mut db)
        .await
        .unwrap()
        .unwrap();
    let second = schema::Repository::filter_by_id(second.id)
        .first()
        .exec(&mut db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.github_install_id, None);
    assert_eq!(first.escrow_contract_id.as_deref(), Some("CFUNDED"));
    assert_eq!(second.github_install_id, Some(installation));
}
