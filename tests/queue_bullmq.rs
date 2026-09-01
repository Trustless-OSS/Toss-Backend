//! Integration tests for the BullMQ-backed job hub.
//!
//! These exercise the real Redis behaviour that the automation depends on:
//! per-issue deduplication, promoting a parked re-check, the repeating scheduler
//! and automatic retries with backoff, plus concurrent enqueue, dirty-flag drain
//! and stalled-job recovery.
//!
//! They are skipped when no Redis is reachable at `REDIS_URL` (default
//! `redis://127.0.0.1:6379`), so `cargo test` still passes on a machine without
//! one. Start the project's Redis with `docker compose up -d redis` to run them.

use std::time::Duration;

use bullmq::{
    options::{RedisConnectionOptions, WorkerOptions},
    types::BackoffStrategy,
    JobOptions, Queue, Worker,
};
use toss_backend::infra::queue::{
    BountyJobData, EnqueueOutcome, QueueInfra, WebhookEnqueueOutcome, WebhookJobData, BOUNTY_QUEUE,
};
use uuid::Uuid;

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string())
}

/// Connect a hub under a prefix unique to this test, or `None` when Redis is
/// unavailable. A unique prefix keeps concurrent tests from colliding.
async fn hub() -> Option<(QueueInfra, String)> {
    let url = redis_url();
    let client = redis::Client::open(url.clone()).ok()?;

    // Cheap reachability probe so a missing Redis skips instead of hanging.
    let mut connection = client.get_multiplexed_async_connection().await.ok()?;
    redis::cmd("PING")
        .query_async::<String>(&mut connection)
        .await
        .ok()?;

    let prefix = format!("toss-test-{}", Uuid::now_v7().simple());
    let queue = QueueInfra::connect(&url, &prefix, Some(client))
        .await
        .ok()?;
    Some((queue, prefix))
}

async fn hub_with_prefix(prefix: &str) -> Option<QueueInfra> {
    let url = redis_url();
    let client = redis::Client::open(url.clone()).ok()?;
    let mut connection = client.get_multiplexed_async_connection().await.ok()?;
    redis::cmd("PING")
        .query_async::<String>(&mut connection)
        .await
        .ok()?;
    QueueInfra::connect(&url, prefix, Some(client)).await.ok()
}

/// Delete every key this test created.
async fn cleanup(prefix: &str) {
    let Ok(client) = redis::Client::open(redis_url()) else {
        return;
    };
    let Ok(mut connection) = client.get_multiplexed_async_connection().await else {
        return;
    };
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(format!("{prefix}:*"))
        .query_async(&mut connection)
        .await
        .unwrap_or_default();
    for key in keys {
        let _: Result<i64, _> = redis::cmd("DEL")
            .arg(key)
            .query_async(&mut connection)
            .await;
    }
}

macro_rules! skip_without_redis {
    () => {
        match hub().await {
            Some(value) => value,
            None => {
                eprintln!("skipping: no Redis at {}", redis_url());
                return;
            }
        }
    };
}

fn webhook_job() -> WebhookJobData {
    WebhookJobData {
        event: "issues".to_string(),
        action: Some("labeled".to_string()),
        payload: serde_json::json!({ "issue": { "number": 5 } }),
    }
}

#[tokio::test]
async fn a_redelivered_webhook_is_recognised_as_a_duplicate() {
    let (queue, prefix) = skip_without_redis!();
    let delivery = format!("delivery-{}", Uuid::now_v7());

    let first = queue
        .enqueue_webhook(Some(&delivery), webhook_job())
        .await
        .unwrap();
    let second = queue
        .enqueue_webhook(Some(&delivery), webhook_job())
        .await
        .unwrap();

    assert_eq!(first, WebhookEnqueueOutcome::Enqueued);
    assert_eq!(second, WebhookEnqueueOutcome::Duplicate);

    let stats = queue.stats().await.unwrap();
    assert_eq!(stats["webhooks"]["waiting"], 1);

    cleanup(&prefix).await;
}

#[tokio::test]
async fn a_burst_of_events_for_one_issue_collapses_to_a_single_job() {
    let (queue, prefix) = skip_without_redis!();
    let issue_id = Uuid::now_v7();

    // Five events land in the same instant, as they do when a maintainer labels,
    // assigns and merges in quick succession.
    let mut outcomes = Vec::new();
    for trigger in [
        "issue-labeled",
        "issue-assigned",
        "wallet-connected",
        "pr-merged",
        "issue-closed",
    ] {
        outcomes.push(
            queue
                .enqueue_advance_issue(BountyJobData::new(issue_id, trigger))
                .await
                .unwrap(),
        );
    }

    assert_eq!(outcomes[0], EnqueueOutcome::Enqueued);
    assert!(
        outcomes[1..]
            .iter()
            .all(|outcome| *outcome == EnqueueOutcome::AlreadyPending),
        "expected the burst to coalesce, got {outcomes:?}"
    );

    // The important part: exactly one job exists, so only one payout can ever run.
    let stats = queue.stats().await.unwrap();
    assert_eq!(stats["escrow-operations"]["waiting"], 1);

    cleanup(&prefix).await;
}

#[tokio::test]
async fn separate_issues_do_not_share_a_job() {
    let (queue, prefix) = skip_without_redis!();

    for _ in 0..3 {
        queue
            .enqueue_advance_issue(BountyJobData::new(Uuid::now_v7(), "issue-assigned"))
            .await
            .unwrap();
    }

    let stats = queue.stats().await.unwrap();
    assert_eq!(stats["escrow-operations"]["waiting"], 3);

    cleanup(&prefix).await;
}

#[tokio::test]
async fn a_real_event_promotes_a_parked_recheck_instead_of_waiting_it_out() {
    let (queue, prefix) = skip_without_redis!();
    let issue_id = Uuid::now_v7();

    // The flow parks itself waiting for a wallet, half an hour out.
    let parked = queue
        .enqueue_advance_issue_after(
            BountyJobData::new(issue_id, "scheduled-recheck").with_recheck(1),
            Duration::from_secs(1_800),
        )
        .await
        .unwrap();
    assert_eq!(parked, EnqueueOutcome::Enqueued);

    let stats = queue.stats().await.unwrap();
    assert_eq!(stats["escrow-operations"]["delayed"], 1);
    assert_eq!(stats["escrow-operations"]["waiting"], 0);

    // The contributor connects their wallet: the parked job must run now, not in
    // thirty minutes.
    let promoted = queue
        .enqueue_advance_issue(BountyJobData::new(issue_id, "wallet-connected"))
        .await
        .unwrap();
    assert_eq!(promoted, EnqueueOutcome::Promoted);

    let stats = queue.stats().await.unwrap();
    assert_eq!(stats["escrow-operations"]["delayed"], 0);
    assert_eq!(stats["escrow-operations"]["waiting"], 1);

    cleanup(&prefix).await;
}

#[tokio::test]
async fn the_escrow_sync_scheduler_keeps_exactly_one_pending_job() {
    let (queue, prefix) = skip_without_redis!();

    // Registering repeatedly is what a restart loop looks like; the old code
    // pushed a new list entry every interval and piled them up.
    for _ in 0..3 {
        queue
            .register_schedulers(Duration::from_secs(60))
            .await
            .unwrap();
    }

    let stats = queue.stats().await.unwrap();
    let pending = stats["sync"]["delayed"].as_u64().unwrap_or(0)
        + stats["sync"]["waiting"].as_u64().unwrap_or(0);
    assert_eq!(pending, 1, "scheduler should hold one pending job: {stats}");

    cleanup(&prefix).await;
}

#[tokio::test]
async fn a_transient_failure_retries_with_backoff_and_then_succeeds() {
    let url = redis_url();
    let Ok(client) = redis::Client::open(url.clone()) else {
        eprintln!("skipping: no Redis at {url}");
        return;
    };
    if client.get_multiplexed_async_connection().await.is_err() {
        eprintln!("skipping: no Redis at {url}");
        return;
    }

    let prefix = format!("toss-test-{}", Uuid::now_v7().simple());
    let queue_name = "retry-probe";

    let connection = RedisConnectionOptions {
        url: url.clone(),
        ..Default::default()
    };

    let queue = Queue::with_options(
        queue_name,
        bullmq::options::QueueOptions::new()
            .prefix(&prefix)
            .connection(connection.clone()),
    )
    .await
    .unwrap();

    let attempts = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let seen = attempts.clone();

    let _worker = Worker::with_options(
        queue_name,
        move |_job: bullmq::Job, _token| {
            let seen = seen.clone();
            async move {
                let attempt = seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if attempt == 0 {
                    // Exactly the shape `to_job_error` produces for a Trustless
                    // Work timeout or a GitHub 5xx.
                    Err(bullmq::Error::ProcessingError("upstream 503".to_string()))
                } else {
                    Ok(serde_json::json!({ "ok": true }))
                }
            }
        },
        WorkerOptions::new()
            .prefix(&prefix)
            .concurrency(1)
            .connection(connection),
    )
    .await
    .unwrap();

    queue
        .add(
            "flaky",
            serde_json::json!({ "issueId": Uuid::now_v7().to_string() }),
        )
        .options(
            JobOptions::new()
                .attempts(3)
                .backoff(BackoffStrategy::Fixed(200)),
        )
        .await
        .unwrap();

    // First attempt fails, the job is delayed by the backoff, then it succeeds —
    // with no operator involvement.
    let mut completed = 0;
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let counts = queue.get_job_counts().await.unwrap();
        if counts.completed >= 1 {
            completed = counts.completed;
            break;
        }
    }

    assert_eq!(completed, 1, "the job should have completed after retrying");
    assert_eq!(
        attempts.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "the processor should have run twice: one failure, one success"
    );

    cleanup(&prefix).await;
}

#[tokio::test]
async fn a_job_can_park_itself_and_still_be_promoted_by_a_real_event() {
    // This is the exact mechanism `advance-issue` uses when it is waiting for a
    // wallet: the running job moves *itself* to delayed, keeping its per-issue id
    // (re-adding that id would be swallowed as a duplicate while the job is
    // active). Keeping the id is what lets a later event promote it.
    let url = redis_url();
    let Ok(client) = redis::Client::open(url.clone()) else {
        eprintln!("skipping: no Redis at {url}");
        return;
    };
    if client.get_multiplexed_async_connection().await.is_err() {
        eprintln!("skipping: no Redis at {url}");
        return;
    }

    let prefix = format!("toss-test-{}", Uuid::now_v7().simple());
    let queue_name = "park-probe";
    let connection = RedisConnectionOptions {
        url: url.clone(),
        ..Default::default()
    };

    let queue = Queue::with_options(
        queue_name,
        bullmq::options::QueueOptions::new()
            .prefix(&prefix)
            .connection(connection.clone()),
    )
    .await
    .unwrap();

    let runs = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let processor_runs = runs.clone();

    let _worker = Worker::with_options(
        queue_name,
        move |job: bullmq::Job, _token| {
            let runs = processor_runs.clone();
            async move {
                let mut job = job;
                let run = runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if run == 0 {
                    // Park half an hour out, widening the re-check counter first.
                    job.update_data(serde_json::json!({ "recheck": 1 })).await?;
                    let resume_at = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64
                        + 1_800_000;
                    job.move_to_delayed(resume_at).await?;
                    Err(bullmq::Error::Delayed)
                } else {
                    Ok(serde_json::json!({ "resumed": true }))
                }
            }
        },
        WorkerOptions::new()
            .prefix(&prefix)
            .concurrency(1)
            .connection(connection),
    )
    .await
    .unwrap();

    let job_id = format!("advance-issue:{}", Uuid::now_v7());
    queue
        .add("advance-issue", serde_json::json!({ "recheck": 0 }))
        .options(JobOptions::new().attempts(5))
        .job_id(job_id.clone())
        .await
        .unwrap();

    // Wait for the job to park itself.
    let mut parked = false;
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if queue.get_job_counts().await.unwrap().delayed >= 1 {
            parked = true;
            break;
        }
    }
    assert!(parked, "the job should have parked itself in delayed");

    let counts = queue.get_job_counts().await.unwrap();
    assert_eq!(counts.failed, 0, "parking is control flow, not a failure");
    assert_eq!(counts.completed, 0);

    // The job kept its id and its widened counter survived the park.
    let job = queue
        .get_job(&job_id)
        .await
        .unwrap()
        .expect("job still exists");
    assert_eq!(job.data()["recheck"], 1);
    assert_eq!(
        job.attempts_made(),
        0,
        "parking must not consume a retry attempt"
    );

    // A real event arrives: promote instead of waiting out the delay.
    job.promote().await.unwrap();

    let mut completed = 0;
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let counts = queue.get_job_counts().await.unwrap();
        if counts.completed >= 1 {
            completed = counts.completed;
            break;
        }
    }

    assert_eq!(
        completed, 1,
        "the promoted job should have run and completed"
    );
    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 2);

    cleanup(&prefix).await;
}

#[tokio::test]
async fn a_permanently_failed_delivery_can_be_revived_by_a_redelivery() {
    // A delivery that burned every attempt must not be treated as a duplicate
    // forever — redelivering it from GitHub is an explicit "try this again".
    let (queue, prefix) = skip_without_redis!();
    let delivery = format!("delivery-{}", Uuid::now_v7());

    // A worker that fails every job permanently, the way a malformed payload
    // does: unrecoverable errors skip retries and go straight to `failed`.
    let worker = Worker::with_options(
        "toss-webhooks",
        |_job: bullmq::Job, _token| async move {
            Err::<serde_json::Value, _>(bullmq::Error::Unrecoverable(
                "payload rejected".to_string(),
            ))
        },
        WorkerOptions::new()
            .prefix(&prefix)
            .concurrency(1)
            .connection(RedisConnectionOptions {
                url: redis_url(),
                ..Default::default()
            }),
    )
    .await
    .unwrap();

    assert_eq!(
        queue
            .enqueue_webhook(Some(&delivery), webhook_job())
            .await
            .unwrap(),
        WebhookEnqueueOutcome::Enqueued
    );

    let mut failed = false;
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if queue.stats().await.unwrap()["webhooks"]["failed"]
            .as_u64()
            .unwrap_or(0)
            >= 1
        {
            failed = true;
            break;
        }
    }
    assert!(failed, "the delivery should have failed permanently");

    // Stop the worker so the revived job stays observable.
    worker.close(2_000).await.unwrap();

    // The redelivery is accepted rather than swallowed as a duplicate.
    assert_eq!(
        queue
            .enqueue_webhook(Some(&delivery), webhook_job())
            .await
            .unwrap(),
        WebhookEnqueueOutcome::Enqueued
    );

    let stats = queue.stats().await.unwrap();
    assert_eq!(stats["webhooks"]["waiting"], 1);
    assert_eq!(stats["webhooks"]["failed"], 0);

    cleanup(&prefix).await;
}

#[tokio::test]
async fn concurrent_enqueues_for_one_issue_leave_exactly_one_job() {
    let (queue, prefix) = skip_without_redis!();
    let issue_id = Uuid::now_v7();

    let mut tasks = Vec::new();
    for i in 0..16 {
        let queue = queue.clone();
        tasks.push(tokio::spawn(async move {
            queue
                .enqueue_advance_issue(BountyJobData::new(issue_id, format!("burst-{i}")))
                .await
        }));
    }

    let mut outcomes = Vec::new();
    for task in tasks {
        outcomes.push(task.await.unwrap().unwrap());
    }

    let enqueued = outcomes
        .iter()
        .filter(|outcome| **outcome == EnqueueOutcome::Enqueued)
        .count();
    assert_eq!(enqueued, 1, "expected one winner, got {outcomes:?}");
    assert!(outcomes.iter().all(|outcome| {
        matches!(
            outcome,
            EnqueueOutcome::Enqueued | EnqueueOutcome::AlreadyPending
        )
    }));

    let stats = queue.stats().await.unwrap();
    let pending = stats["escrow-operations"]["waiting"].as_u64().unwrap_or(0)
        + stats["escrow-operations"]["active"].as_u64().unwrap_or(0)
        + stats["escrow-operations"]["delayed"].as_u64().unwrap_or(0);
    assert_eq!(pending, 1, "concurrent enqueue dropped the job: {stats}");

    cleanup(&prefix).await;
}

#[tokio::test]
async fn dirty_flags_do_not_leak_across_bullmq_prefixes() {
    let prefix_a = format!("toss-test-{}", Uuid::now_v7().simple());
    let prefix_b = format!("toss-test-{}", Uuid::now_v7().simple());
    let Some(queue_a) = hub_with_prefix(&prefix_a).await else {
        eprintln!("skipping: no Redis at {}", redis_url());
        return;
    };
    let Some(queue_b) = hub_with_prefix(&prefix_b).await else {
        eprintln!("skipping: no Redis at {}", redis_url());
        cleanup(&prefix_a).await;
        return;
    };

    let issue_id = Uuid::now_v7();
    queue_a.mark_dirty(issue_id).await.unwrap();

    assert!(queue_a.is_dirty(issue_id).await.unwrap());
    assert!(
        !queue_b.is_dirty(issue_id).await.unwrap(),
        "dirty keys must include the BullMQ prefix"
    );
    assert_ne!(queue_a.dirty_key(issue_id), queue_b.dirty_key(issue_id));

    cleanup(&prefix_a).await;
    cleanup(&prefix_b).await;
}

#[tokio::test]
async fn a_dirty_flag_set_while_a_job_completes_is_drained_into_a_follow_up() {
    let (queue, prefix) = skip_without_redis!();
    let issue_id = Uuid::now_v7();
    let connection = RedisConnectionOptions {
        url: redis_url(),
        ..Default::default()
    };

    let started = std::sync::Arc::new(tokio::sync::Notify::new());
    let gate = std::sync::Arc::new(tokio::sync::Notify::new());
    let worker_started = started.clone();
    let worker_gate = gate.clone();

    let worker = Worker::with_options(
        BOUNTY_QUEUE,
        move |_job: bullmq::Job, _token| {
            let started = worker_started.clone();
            let gate = worker_gate.clone();
            async move {
                started.notify_one();
                gate.notified().await;
                Ok(serde_json::json!({ "ok": true }))
            }
        },
        WorkerOptions::new()
            .prefix(&prefix)
            .concurrency(1)
            .connection(connection),
    )
    .await
    .unwrap();

    assert_eq!(
        queue
            .enqueue_advance_issue(BountyJobData::new(issue_id, "first"))
            .await
            .unwrap(),
        EnqueueOutcome::Enqueued
    );

    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .expect("worker should pick up the job");

    assert_eq!(
        queue
            .enqueue_advance_issue(BountyJobData::new(issue_id, "late-event"))
            .await
            .unwrap(),
        EnqueueOutcome::Coalesced
    );
    assert!(queue.is_dirty(issue_id).await.unwrap());

    gate.notify_one();

    let mut idle = false;
    for _ in 0..80 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let stats = queue.stats().await.unwrap();
        let active = stats["escrow-operations"]["active"].as_u64().unwrap_or(0);
        let waiting = stats["escrow-operations"]["waiting"].as_u64().unwrap_or(0);
        if active == 0 && waiting == 0 {
            idle = true;
            break;
        }
    }
    assert!(idle, "the first job should have left active");
    assert!(
        queue.is_dirty(issue_id).await.unwrap(),
        "the late event must survive the completed job"
    );

    let drained = queue.drain_dirty_advance(issue_id).await.unwrap();
    assert_eq!(drained, EnqueueOutcome::Enqueued);
    assert!(!queue.is_dirty(issue_id).await.unwrap());

    let stats = queue.stats().await.unwrap();
    let pending = stats["escrow-operations"]["waiting"].as_u64().unwrap_or(0)
        + stats["escrow-operations"]["active"].as_u64().unwrap_or(0)
        + stats["escrow-operations"]["delayed"].as_u64().unwrap_or(0);
    assert_eq!(pending, 1, "dirty drain must enqueue a follow-up: {stats}");

    worker.close(2_000).await.unwrap();
    cleanup(&prefix).await;
}

#[tokio::test]
async fn a_killed_worker_does_not_leave_a_job_active_past_the_stall_window() {
    let (queue, prefix) = skip_without_redis!();
    let issue_id = Uuid::now_v7();
    let connection = RedisConnectionOptions {
        url: redis_url(),
        ..Default::default()
    };

    let started = std::sync::Arc::new(tokio::sync::Notify::new());
    let worker_started = started.clone();

    let hanging = Worker::with_options(
        BOUNTY_QUEUE,
        move |_job: bullmq::Job, _token| {
            let started = worker_started.clone();
            async move {
                started.notify_one();
                std::future::pending::<Result<serde_json::Value, bullmq::Error>>().await
            }
        },
        WorkerOptions::new()
            .prefix(&prefix)
            .concurrency(1)
            .lock_duration(Duration::from_millis(400))
            .stalled_interval(Duration::from_millis(400))
            .max_stalled_count(1)
            .skip_lock_renewal()
            .skip_stalled_check()
            .connection(connection.clone()),
    )
    .await
    .unwrap();

    assert_eq!(
        queue
            .enqueue_advance_issue(BountyJobData::new(issue_id, "stall"))
            .await
            .unwrap(),
        EnqueueOutcome::Enqueued
    );

    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .expect("hanging worker should pick up the job");

    hanging.close(0).await.unwrap();

    let recovered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = recovered.clone();
    let _rescuer = Worker::with_options(
        BOUNTY_QUEUE,
        move |_job: bullmq::Job, _token| {
            let seen = seen.clone();
            async move {
                seen.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(serde_json::json!({ "recovered": true }))
            }
        },
        WorkerOptions::new()
            .prefix(&prefix)
            .concurrency(1)
            .lock_duration(Duration::from_millis(400))
            .stalled_interval(Duration::from_millis(400))
            .max_stalled_count(1)
            .connection(connection),
    )
    .await
    .unwrap();

    let mut recovered_or_moved = false;
    for _ in 0..80 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if recovered.load(std::sync::atomic::Ordering::SeqCst) {
            recovered_or_moved = true;
            break;
        }
        let stats = queue.stats().await.unwrap();
        if stats["escrow-operations"]["active"].as_u64().unwrap_or(0) == 0
            && (stats["escrow-operations"]["waiting"].as_u64().unwrap_or(0) >= 1
                || stats["escrow-operations"]["completed"]
                    .as_u64()
                    .unwrap_or(0)
                    >= 1
                || stats["escrow-operations"]["failed"].as_u64().unwrap_or(0) >= 1)
        {
            recovered_or_moved = true;
            break;
        }
    }

    assert!(
        recovered_or_moved || recovered.load(std::sync::atomic::Ordering::SeqCst),
        "stalled active job was not recovered"
    );

    cleanup(&prefix).await;
}
