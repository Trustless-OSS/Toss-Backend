//! Serialize identical comment deliveries locally and across backend workers.
use crate::error::AppError;
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const SENT_TTL: Duration = Duration::from_secs(3600);
type DeliveryState = Arc<Mutex<Option<Instant>>>;
type DeliveryRegistry = HashMap<String, DeliveryState>;

static DELIVERIES: LazyLock<Mutex<DeliveryRegistry>> = LazyLock::new(|| Mutex::new(HashMap::new()));

const CLAIM: &str = r#"
local value = redis.call('GET', KEYS[1])
if value == 'sent' then return 2 end
if value then return 0 end
redis.call('SET', KEYS[1], ARGV[1], 'EX', 60)
return 1
"#;
const FINISH: &str = r#"
if redis.call('GET', KEYS[1]) ~= ARGV[1] then return 0 end
if ARGV[2] == 'sent' then
    redis.call('SET', KEYS[1], 'sent', 'EX', 3600)
else
    redis.call('DEL', KEYS[1])
end
return 1
"#;

pub(super) async fn once<F, Fut>(
    redis: Option<&redis::Client>,
    key: String,
    send: F,
) -> Result<(), AppError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), AppError>>,
{
    let entry = {
        let mut entries = DELIVERIES.lock().await;
        entries.retain(|_, entry| {
            Arc::strong_count(entry) > 1
                || entry
                    .try_lock()
                    .map_or(true, |sent| sent.is_some_and(|at| at.elapsed() < SENT_TTL))
        });
        entries
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(None)))
            .clone()
    };
    let mut sent = entry.lock().await;
    if sent.is_some_and(|at| at.elapsed() < SENT_TTL) {
        return Ok(());
    }

    // Fail closed if configured Redis is unavailable: another worker may be sending.
    let mut connection = match redis {
        Some(redis) => Some(
            redis
                .get_multiplexed_async_connection()
                .await
                .map_err(redis_error)?,
        ),
        None => None,
    };
    let token = uuid::Uuid::now_v7().to_string();
    if let Some(connection) = &mut connection {
        let mut acquired = false;
        for _ in 0..50 {
            let claim: i32 = redis::Script::new(CLAIM)
                .key(&key)
                .arg(&token)
                .invoke_async(connection)
                .await
                .map_err(redis_error)?;
            match claim {
                2 => {
                    *sent = Some(Instant::now());
                    return Ok(());
                }
                1 => {
                    acquired = true;
                    break;
                }
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
        if !acquired {
            return Err(AppError::github(
                "Identical comment delivery is still in progress; retry later",
            ));
        }
    }

    // Bound the whole delivery below the lease so another worker cannot acquire
    // it during a slow token lookup or GitHub POST.
    let result = tokio::time::timeout(Duration::from_secs(45), send())
        .await
        .unwrap_or_else(|_| Err(AppError::github("GitHub comment delivery timed out")));
    if result.is_ok() {
        *sent = Some(Instant::now());
    }
    if let Some(connection) = &mut connection {
        let _: i32 = redis::Script::new(FINISH)
            .key(&key)
            .arg(&token)
            .arg(if result.is_ok() { "sent" } else { "retry" })
            .invoke_async(connection)
            .await
            .map_err(redis_error)?;
    }
    result
}

fn redis_error(error: redis::RedisError) -> AppError {
    AppError::github(format!("Comment delivery coordination failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn simultaneous_identical_comments_send_once() {
        let key = uuid::Uuid::now_v7().to_string();
        let count = Arc::new(AtomicUsize::new(0));
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..12 {
            let key = key.clone();
            let count = count.clone();
            tasks.spawn(async move {
                once(None, key, || async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Ok(())
                })
                .await
                .unwrap();
            });
        }
        while let Some(task) = tasks.join_next().await {
            task.unwrap();
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_delivery_can_be_retried() {
        let key = uuid::Uuid::now_v7().to_string();
        assert!(once(None, key.clone(), || async {
            Err(AppError::github("failed"))
        })
        .await
        .is_err());
        let mut called = false;
        once(None, key, || async {
            called = true;
            Ok(())
        })
        .await
        .unwrap();
        assert!(called);
    }

    #[tokio::test]
    async fn redis_claim_is_atomic_and_only_owner_can_finish() {
        let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
        let client = redis::Client::open(url).unwrap();
        let Ok(mut connection) = client.get_multiplexed_async_connection().await else {
            eprintln!("skipping comment claim integration test: Redis unavailable");
            return;
        };
        let key = format!("toss-test-comment:{}", uuid::Uuid::now_v7());
        let claim = redis::Script::new(CLAIM);
        let first: i32 = claim
            .key(&key)
            .arg("owner")
            .invoke_async(&mut connection)
            .await
            .unwrap();
        let duplicate: i32 = claim
            .key(&key)
            .arg("other")
            .invoke_async(&mut connection)
            .await
            .unwrap();
        assert_eq!((first, duplicate), (1, 0));
        let wrong: i32 = redis::Script::new(FINISH)
            .key(&key)
            .arg("other")
            .arg("sent")
            .invoke_async(&mut connection)
            .await
            .unwrap();
        assert_eq!(wrong, 0);
        let _: i32 = redis::Script::new(FINISH)
            .key(&key)
            .arg("owner")
            .arg("sent")
            .invoke_async(&mut connection)
            .await
            .unwrap();
        let duplicate: i32 = claim
            .key(&key)
            .arg("other")
            .invoke_async(&mut connection)
            .await
            .unwrap();
        assert_eq!(duplicate, 2);
        let _: i32 = redis::cmd("DEL")
            .arg(&key)
            .query_async(&mut connection)
            .await
            .unwrap();
    }
}
