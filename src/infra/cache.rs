use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use redis::AsyncCommands;
use serde::{de::DeserializeOwned, Serialize};
use tracing::{debug, error, warn};

use crate::infra::redis::{check_health, RedisClient, RedisHealthStatus};

const DEFAULT_TTL: u64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
pub enum CacheType {
    GhToken,
    Repo,
    Contrib,
    Other,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CacheTypeStats {
    pub hits: u64,
    pub misses: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct CacheTypeSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub hit_rate: Option<f64>,
}

#[derive(Clone)]
pub struct Cache {
    redis: Option<RedisClient>,
    stats: Arc<Mutex<BTreeMap<CacheType, CacheTypeStats>>>,
}

impl Cache {
    pub fn new(redis: Option<RedisClient>) -> Self {
        let stats = BTreeMap::from([
            (CacheType::GhToken, CacheTypeStats::default()),
            (CacheType::Repo, CacheTypeStats::default()),
            (CacheType::Contrib, CacheTypeStats::default()),
            (CacheType::Other, CacheTypeStats::default()),
        ]);

        Self {
            redis,
            stats: Arc::new(Mutex::new(stats)),
        }
    }

    pub async fn get<T>(&self, key: &str) -> Option<T>
    where
        T: DeserializeOwned,
    {
        let health = check_health(self.redis.as_ref()).await;
        if health.status != RedisHealthStatus::Ok {
            warn!(key, "Redis unavailable, cache miss");
            self.record_miss(key);
            return None;
        }

        let Some(client) = &self.redis else {
            self.record_miss(key);
            return None;
        };

        match client.get_multiplexed_async_connection().await {
            Ok(mut connection) => match connection.get::<_, Option<String>>(key).await {
                Ok(Some(value)) => match serde_json::from_str::<T>(&value) {
                    Ok(parsed) => {
                        debug!(key, "cache hit");
                        self.record_hit(key);
                        Some(parsed)
                    }
                    Err(err) => {
                        error!(key, error = %err, "error deserializing cache value");
                        self.record_miss(key);
                        None
                    }
                },
                Ok(None) => {
                    debug!(key, "cache miss");
                    self.record_miss(key);
                    None
                }
                Err(err) => {
                    error!(key, error = %err, "error getting cache key");
                    self.record_miss(key);
                    None
                }
            },
            Err(err) => {
                error!(key, error = %err, "error opening redis connection");
                self.record_miss(key);
                None
            }
        }
    }

    pub async fn set<T>(&self, key: &str, value: &T, ttl: Option<u64>)
    where
        T: Serialize,
    {
        let health = check_health(self.redis.as_ref()).await;
        if health.status != RedisHealthStatus::Ok {
            warn!(key, "Redis unavailable, cache set skipped");
            return;
        }

        let Some(client) = &self.redis else {
            return;
        };

        let ttl = ttl.filter(|ttl| *ttl > 0).unwrap_or(DEFAULT_TTL);
        let serialized = match serde_json::to_string(value) {
            Ok(serialized) => serialized,
            Err(err) => {
                error!(key, error = %err, "error serializing cache value");
                return;
            }
        };

        match client.get_multiplexed_async_connection().await {
            Ok(mut connection) => {
                let result = connection.set_ex::<_, _, ()>(key, serialized, ttl).await;
                if let Err(err) = result {
                    error!(key, error = %err, "error setting cache key");
                }
            }
            Err(err) => error!(key, error = %err, "error opening redis connection"),
        }
    }

    pub async fn invalidate(&self, key: &str) {
        let health = check_health(self.redis.as_ref()).await;
        if health.status != RedisHealthStatus::Ok {
            warn!(key, "Redis unavailable, cache invalidate skipped");
            return;
        }

        let Some(client) = &self.redis else {
            return;
        };

        match client.get_multiplexed_async_connection().await {
            Ok(mut connection) => {
                let result = connection.del::<_, ()>(key).await;
                if let Err(err) = result {
                    error!(key, error = %err, "error invalidating cache key");
                }
            }
            Err(err) => error!(key, error = %err, "error opening redis connection"),
        }
    }

    pub fn stats(&self) -> BTreeMap<CacheType, CacheTypeSnapshot> {
        let guard = self.stats.lock().expect("cache stats mutex poisoned");
        guard
            .iter()
            .map(|(cache_type, stats)| {
                let total = stats.hits + stats.misses;
                let hit_rate = if total > 0 {
                    Some(stats.hits as f64 / total as f64)
                } else {
                    None
                };

                (
                    *cache_type,
                    CacheTypeSnapshot {
                        hits: stats.hits,
                        misses: stats.misses,
                        hit_rate,
                    },
                )
            })
            .collect()
    }

    fn record_hit(&self, key: &str) {
        self.update_stats(key, true);
    }

    fn record_miss(&self, key: &str) {
        self.update_stats(key, false);
    }

    fn update_stats(&self, key: &str, hit: bool) {
        let cache_type = resolve_cache_type(key);
        if let Ok(mut guard) = self.stats.lock() {
            let entry = guard.entry(cache_type).or_default();
            if hit {
                entry.hits += 1;
            } else {
                entry.misses += 1;
            }
        }
    }
}

fn resolve_cache_type(key: &str) -> CacheType {
    if key.starts_with("gh-token:") {
        CacheType::GhToken
    } else if key.starts_with("repo:") {
        CacheType::Repo
    } else if key.starts_with("contrib:") {
        CacheType::Contrib
    } else {
        CacheType::Other
    }
}
