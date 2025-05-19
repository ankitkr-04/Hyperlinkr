// src/services/cache/mod.rs

pub mod bloom;
pub mod moka_cache;
pub mod metrics;
pub mod circuit_breaker;
pub mod sync_cache;

use crate::config::cache::CacheConfig;
use crate::services::storage::dragonfly::DatabaseClient;
use std::sync::Arc;
use parking_lot::RwLock;
use bb8::Pool;
use bb8_redis::RedisConnectionManager;
use redis::AsyncCommands;
use bloom::CacheBloom;
use moka_cache::MokaCache;
use circuit_breaker::CircuitBreaker;
use sync_cache::SyncCache;

pub struct CacheService {
    pub l1: Arc<SyncCache>,
    pub l2: Arc<MokaCache>,
    pub bloom: Arc<RwLock<CacheBloom>>,
    pub db_pool: Pool<RedisConnectionManager>,
    pub circuit_breaker: Arc<CircuitBreaker>,
    /// we keep the node identifier around for CB calls
    pub node_id: String,
}

impl CacheService {
    /// Initialize L1, L2, Bloom filter, Redis pool & circuit breaker
    pub async fn new(config: &CacheConfig, redis_url: &str) -> Self {
        // Bloom filter
        let bloom = Arc::new(RwLock::new(CacheBloom::new(
            config.bloom_bits,
            config.bloom_expected,
        )));

        // L1 & L2 caches
        let l1 = Arc::new(SyncCache::new(config.l1_capacity));
        let l2 = Arc::new(MokaCache::new(config.l2_capacity));

        // bb8 + RedisConnectionManager pool
        let manager = RedisConnectionManager::new(redis_url)
            .expect("Invalid Redis URL");
        let db_pool = Pool::builder()
            .max_size(config.redis_pool_size)
            .idle_timeout(Some(std::time::Duration::from_secs(300)))
            .build(manager)
            .await
            .expect("Failed to build Redis pool");

        // Circuit breaker seeded with the single node
        let circuit_breaker = Arc::new(CircuitBreaker::new(vec![redis_url.to_string()]));

        Self {
            l1,
            l2,
            bloom,
            db_pool,
            circuit_breaker,
            node_id: redis_url.to_string(),
        }
    }

    /// Get a URL by key, with L1→Bloom→L2→DB cascade, metrics & circuit-breaker
    pub async fn get(&self, key: &str) -> Result<String, &'static str> {
        let start = std::time::Instant::now();

        // --- L1 check ---
        if let Some(val) = self.l1.get(key) {
            metrics::CACHE_HITS.with_label_values(&["l1"]).inc();
            metrics::CACHE_LATENCY
                .with_label_values(&["l1"])
                .observe(start.elapsed().as_secs_f64());
            return Ok(val);
        }

        // --- Bloom filter ---
        if !self.bloom.read().contains(key) {
            metrics::CACHE_LATENCY
                .with_label_values(&["bloom"])
                .observe(start.elapsed().as_secs_f64());
            return Err("Not found in Bloom filter");
        }

        // --- L2 check ---
        if let Some(val) = self.l2.get(key).await {
            // populate L1
            self.l1.insert(key.to_string(), val.clone());
            metrics::CACHE_HITS.with_label_values(&["l2"]).inc();
            metrics::CACHE_LATENCY
                .with_label_values(&["l2"])
                .observe(start.elapsed().as_secs_f64());
            return Ok(val);
        }

        // --- Circuit breaker ---
        if !self.circuit_breaker.should_try(&self.node_id).await {
            return Err("Circuit breaker open");
        }

        // --- Redis fetch ---
        let mut conn = match self.db_pool.get().await {
            Ok(c) => c,
            Err(_) => {
                // record failure and bail
                self.circuit_breaker.record_failure(&self.node_id).await;
                return Err("Failed to get Redis connection");
            }
        };
        let result: Option<Vec<u8>> = match conn.get(key).await {
            Ok(r) => r,
            Err(_) => {
                // record failure on GET error
                self.circuit_breaker.record_failure(&self.node_id).await;
                return Err("Redis GET failed");
            }
        };

        let url = result
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .ok_or("Key not found")?;

        // populate caches & bloom
        self.l2.insert(key.to_string(), url.clone()).await;
        self.l1.insert(key.to_string(), url.clone());
        self.bloom.write().insert(key);

        metrics::CACHE_LATENCY
            .with_label_values(&["db"])
            .observe(start.elapsed().as_secs_f64());

        Ok(url)
    }

    /// Bulk-warm keys into L1 & L2 (skipping circuit-breaker & metrics)
    pub async fn warmup(&self, keys: Vec<String>) {
        let futures = keys.into_iter().map(|key| {
            let bloom = Arc::clone(&self.bloom);
            let l1 = Arc::clone(&self.l1);
            let l2 = Arc::clone(&self.l2);
            let pool = self.db_pool.clone();
            async move {
                if let Ok(mut conn) = pool.get().await {
                    if let Ok(Some(bytes)) = conn.get(&key).await {
                        let url = String::from_utf8_lossy(&bytes).into_owned();
                        l2.insert(key.clone(), url.clone()).await;
                        l1.insert(key.clone(), url.clone());
                        bloom.write().insert(&key);
                    }
                }
            }
        });
        futures::future::join_all(futures).await;
    }
}
