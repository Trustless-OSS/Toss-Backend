//! Redis-backed integration tests for the webhook queue.
//!
//! No Docker required: if `REDIS_URL` is set (or something is already
//! listening on `127.0.0.1:6379`), that instance is used; otherwise this
//! file spawns its own throwaway `redis-server` process on a free port for
//! the duration of the test run (killed when the process exits). Every
//! test gets its own Redis key namespace, so tests can run in parallel
//! (the `cargo test` default) without interfering with each other.
//!
//! If neither a reachable Redis nor a local `redis-server` binary is
//! available, every test skips itself with a message rather than failing
//! -- `cargo test` still passes. Install Redis locally to actually run
//! these (`apt install redis-server` / `brew install redis`), no Docker
//! needed.

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use toss_backend::config::Config;
use toss_backend::infra::queue::{NewWebhookJob, QueueInfra, WebhookEnqueueOutcome};
use toss_backend::infra::redis::build_client;

fn test_config() -> Config {
    Config {
        port: 5000,
        log_level: "info".into(),
        node_env: "test".into(),
        database_url: String::new(),
        redis_url: String::new(),
        supabase_url: String::new(),
        supabase_auth_api_key: String::new(),
        github_app_id: String::new(),
        github_app_private_key: String::new(),
        github_bot_token: None,
        github_webhook_secret: String::new(),
        platform_stellar_public_key: String::new(),
        platform_stellar_secret_key: String::new(),
        dispute_resolver_stellar_public_key: String::new(),
        dispute_resolver_stellar_secret_key: String::new(),
        trustless_work_api_key: String::new(),
        trustless_work_base_url: String::new(),
        stellar_network: "testnet".into(),
        app_url: String::new(),
        webhook_url: None,
        dev_webhook_proxy_enabled: false,
        smee_source_url: String::new(),
        smee_target_url: String::new(),
        // Small numbers so retry/backoff/lease tests don't have to wait long.
        webhook_max_attempts: 3,
        webhook_lease_seconds: 1,
        webhook_retry_base_ms: 10,
        webhook_retry_max_ms: 50,
        webhook_completed_dedup_ttl_seconds: 60,
        queue_admin_token: None,
    }
}

async fn is_reachable(url: &str) -> bool {
    let Ok(client) = build_client(url) else {
        return false;
    };
    let ping = tokio::time::timeout(
        Duration::from_millis(300),
        client.get_multiplexed_async_connection(),
    )
    .await;
    matches!(ping, Ok(Ok(_)))
}

fn free_port() -> Option<u16> {
    TcpListener::bind("127.0.0.1:0")
        .ok()?
        .local_addr()
        .ok()
        .map(|a| a.port())
}

/// Kept alive for the process lifetime once spawned; on Linux/macOS a
/// leftover `redis-server` on an ephemeral port is harmless (it exits with
/// the sandbox/CI runner). Run `pkill redis-server` if you want it gone
/// sooner on a long-lived dev machine.
static EPHEMERAL_REDIS: OnceLock<Option<(Mutex<Child>, String)>> = OnceLock::new();

/// Spawns a throwaway local `redis-server`, or returns `None` if the binary
/// isn't installed. `OnceLock::get_or_init` guarantees this spawn logic
/// runs exactly once even if several tests race to call it concurrently
/// (the default with `cargo test`) -- without that guarantee, concurrent
/// callers could each spawn their own orphaned redis-server.
fn ephemeral_redis_url() -> Option<String> {
    EPHEMERAL_REDIS
        .get_or_init(|| {
            let port = free_port()?;
            match Command::new("redis-server")
                .args([
                    "--port",
                    &port.to_string(),
                    "--save",
                    "",
                    "--appendonly",
                    "no",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(child) => Some((Mutex::new(child), format!("redis://127.0.0.1:{port}"))),
                Err(error) => {
                    eprintln!(
                        "skipping Redis-backed tests: no Redis reachable and couldn't spawn \
                         redis-server locally ({error}). Install it (apt install redis-server / \
                         brew install redis) or set REDIS_URL to a running instance -- no \
                         Docker needed."
                    );
                    None
                }
            }
        })
        .as_ref()
        .map(|(_, url)| url.clone())
}

/// Resolves a working Redis URL: an already-reachable `REDIS_URL` /
/// localhost instance first, otherwise a freshly spawned local
/// `redis-server`. Returns `None` (causing callers to skip) only if
/// neither is available.
async fn redis_url() -> Option<String> {
    if let Ok(url) = std::env::var("REDIS_URL") {
        if is_reachable(&url).await {
            return Some(url);
        }
    }
    if is_reachable("redis://127.0.0.1:6379").await {
        return Some("redis://127.0.0.1:6379".to_string());
    }

    let url = ephemeral_redis_url()?;
    for _ in 0..40 {
        if is_reachable(&url).await {
            return Some(url);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    eprintln!("skipping: spawned redis-server at {url} never became ready");
    None
}

/// Builds a queue scoped to its own Redis key namespace, so tests running
/// concurrently against the same Redis process never see each other's jobs.
async fn test_queue(namespace_suffix: &str) -> Option<QueueInfra> {
    let url = redis_url().await?;
    let client = build_client(&url).ok()?;
    let namespace = format!(
        "queue:webhooks:test:{namespace_suffix}:{}",
        uuid::Uuid::new_v4()
    );
    Some(QueueInfra::with_namespace(
        Some(client),
        &test_config(),
        namespace,
    ))
}

fn sample_job(event: &str) -> NewWebhookJob {
    NewWebhookJob {
        event: event.to_string(),
        action: Some("opened".to_string()),
        payload: serde_json::json!({ "sample": true }),
        correlation_id: uuid::Uuid::new_v4().to_string(),
    }
}

#[tokio::test]
async fn enqueue_and_claim_round_trip() {
    let Some(queue) = test_queue("basic").await else {
        return;
    };
    let delivery_id = format!("delivery-{}", uuid::Uuid::new_v4());

    let outcome = queue
        .enqueue_webhook(Some(&delivery_id), sample_job("issues"))
        .await
        .unwrap();
    assert_eq!(outcome, WebhookEnqueueOutcome::Enqueued);

    let claimed = queue
        .claim_webhook()
        .await
        .unwrap()
        .expect("job should be claimable");
    assert_eq!(
        claimed.envelope.delivery_id.as_deref(),
        Some(delivery_id.as_str())
    );
    assert_eq!(claimed.envelope.event, "issues");

    queue
        .ack_webhook(&claimed.job_id, Some(&delivery_id))
        .await
        .unwrap();
}

#[tokio::test]
async fn redelivery_of_a_completed_job_is_rejected() {
    let Some(queue) = test_queue("dedup-completed").await else {
        return;
    };
    let delivery_id = format!("delivery-{}", uuid::Uuid::new_v4());

    queue
        .enqueue_webhook(Some(&delivery_id), sample_job("issues"))
        .await
        .unwrap();
    let claimed = queue.claim_webhook().await.unwrap().unwrap();
    queue
        .ack_webhook(&claimed.job_id, Some(&delivery_id))
        .await
        .unwrap();

    let redelivered = queue
        .enqueue_webhook(Some(&delivery_id), sample_job("issues"))
        .await
        .unwrap();
    assert_eq!(redelivered, WebhookEnqueueOutcome::Duplicate);
}

#[tokio::test]
async fn redelivery_while_still_queued_is_rejected_but_not_lost() {
    let Some(queue) = test_queue("dedup-queued").await else {
        return;
    };
    let delivery_id = format!("delivery-{}", uuid::Uuid::new_v4());

    queue
        .enqueue_webhook(Some(&delivery_id), sample_job("issues"))
        .await
        .unwrap();
    // GitHub redelivers before anyone has claimed the first job.
    let redelivered = queue
        .enqueue_webhook(Some(&delivery_id), sample_job("issues"))
        .await
        .unwrap();
    assert_eq!(redelivered, WebhookEnqueueOutcome::Duplicate);

    // The original job is still there -- nothing was lost.
    let claimed = queue.claim_webhook().await.unwrap();
    assert!(claimed.is_some());
}

#[tokio::test]
async fn failed_job_is_retried_then_becomes_claimable_again() {
    let Some(queue) = test_queue("retry").await else {
        return;
    };
    let delivery_id = format!("delivery-{}", uuid::Uuid::new_v4());
    queue
        .enqueue_webhook(Some(&delivery_id), sample_job("issues"))
        .await
        .unwrap();

    let claimed = queue.claim_webhook().await.unwrap().unwrap();
    let error = toss_backend::error::AppError::internal("handler exploded");
    queue
        .report_failure(&claimed.job_id, claimed.envelope, &error)
        .await
        .unwrap();

    // Not immediately requeued...
    assert!(queue.claim_webhook().await.unwrap().is_none());

    // ...but is claimable again once the backoff delay has passed and the
    // sweeper promotes it.
    tokio::time::sleep(Duration::from_millis(200)).await;
    queue.promote_delayed().await.unwrap();
    let retried = queue
        .claim_webhook()
        .await
        .unwrap()
        .expect("job should be retried");
    assert_eq!(retried.envelope.attempts, 1);
}

#[tokio::test]
async fn job_is_dead_lettered_after_exhausting_retry_budget() {
    let Some(queue) = test_queue("dlq").await else {
        return;
    };
    let delivery_id = format!("delivery-{}", uuid::Uuid::new_v4());
    queue
        .enqueue_webhook(Some(&delivery_id), sample_job("issues"))
        .await
        .unwrap();

    // max_attempts is 3 in test_config(); fail it three times.
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        queue.promote_delayed().await.unwrap();
        let claimed = queue.claim_webhook().await.unwrap();
        let Some(claimed) = claimed else { break };
        let error = toss_backend::error::AppError::internal("still exploding");
        queue
            .report_failure(&claimed.job_id, claimed.envelope, &error)
            .await
            .unwrap();
    }

    let dead_letters = queue.list_dead_letters(10).await.unwrap();
    assert!(dead_letters
        .iter()
        .any(|entry| entry.envelope.delivery_id.as_deref() == Some(delivery_id.as_str())));
}

#[tokio::test]
async fn expired_lease_is_recovered_without_losing_the_job() {
    let Some(queue) = test_queue("lease-recovery").await else {
        return;
    };
    let delivery_id = format!("delivery-{}", uuid::Uuid::new_v4());
    queue
        .enqueue_webhook(Some(&delivery_id), sample_job("issues"))
        .await
        .unwrap();

    // Simulate a worker crashing right after claim: it never acks or fails.
    let claimed = queue.claim_webhook().await.unwrap().unwrap();
    drop(claimed);

    // Wait past the 1-second test lease, then run the sweep a real worker
    // would run periodically.
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    let recovered = queue.recover_expired_leases().await.unwrap();
    assert_eq!(recovered, 1);

    tokio::time::sleep(Duration::from_millis(100)).await;
    queue.promote_delayed().await.unwrap();
    let requeued = queue.claim_webhook().await.unwrap();
    assert!(requeued.is_some(), "job should survive a crashed worker");
}

#[tokio::test]
async fn dead_letter_replay_requeues_the_job() {
    let Some(queue) = test_queue("replay").await else {
        return;
    };
    let delivery_id = format!("delivery-{}", uuid::Uuid::new_v4());
    queue
        .enqueue_webhook(Some(&delivery_id), sample_job("issues"))
        .await
        .unwrap();

    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        queue.promote_delayed().await.unwrap();
        if let Some(claimed) = queue.claim_webhook().await.unwrap() {
            let error = toss_backend::error::AppError::internal("still exploding");
            queue
                .report_failure(&claimed.job_id, claimed.envelope, &error)
                .await
                .unwrap();
        }
    }

    let dead_letters = queue.list_dead_letters(10).await.unwrap();
    let entry = dead_letters
        .into_iter()
        .find(|entry| entry.envelope.delivery_id.as_deref() == Some(delivery_id.as_str()))
        .expect("job should be dead-lettered");

    let replayed = queue
        .replay_dead_letter(&entry.job_id, "test-operator")
        .await
        .unwrap();
    assert!(replayed);

    let reclaimed = queue
        .claim_webhook()
        .await
        .unwrap()
        .expect("replayed job should be claimable");
    assert_eq!(
        reclaimed.envelope.attempts, 0,
        "replay resets the attempt budget"
    );
}

#[tokio::test]
async fn concurrent_workers_never_claim_the_same_job() {
    let Some(queue) = test_queue("concurrency").await else {
        return;
    };
    let run_id = uuid::Uuid::new_v4();
    for i in 0..20 {
        queue
            .enqueue_webhook(
                Some(&format!("delivery-concurrency-{run_id}-{i}")),
                sample_job("issues"),
            )
            .await
            .unwrap();
    }

    let mut handles = Vec::new();
    for _ in 0..5 {
        let queue = queue.clone();
        handles.push(tokio::spawn(async move {
            let mut claimed_ids = Vec::new();
            while let Some(job) = queue.claim_webhook().await.unwrap() {
                queue
                    .ack_webhook(&job.job_id, job.envelope.delivery_id.as_deref())
                    .await
                    .unwrap();
                claimed_ids.push(job.job_id);
            }
            claimed_ids
        }));
    }

    let mut all_claimed = Vec::new();
    for handle in handles {
        all_claimed.extend(handle.await.unwrap());
    }

    let mut deduped = all_claimed.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(
        all_claimed.len(),
        deduped.len(),
        "no job claimed by two workers"
    );
    assert_eq!(all_claimed.len(), 20);
}
