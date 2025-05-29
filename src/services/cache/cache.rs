use crate::config::settings::Settings;
use crate::errors::AppError;
use std::sync::Arc;
use bb8::Pool;
use bb8_redis::RedisConnectionManager;
use redis::{AsyncCommands, Expiry};
use super::bloom::CacheBloom;
use super::l2_cache::L2Cache;
use super::circuit_breaker::CircuitBreaker;
use super::l1_cache::L1Cache;
use tracing::{info, error};
use futures::future;
use std::time::Instant;

pub struct CacheService {
    pub l1: Arc<L1Cache>,
    pub l2: Arc<L2Cache>,
    pub bloom: Arc<CacheBloom>,
    pub db_pool: Pool<RedisConnectionManager>,
    pub circuit_breaker: Arc<CircuitBreaker>,
    pub ttl_seconds: u64,
}

impl CacheService {
    pub async fn new(config: &Settings) -> Self {
        metrics::init_metrics();
        let bloom = Arc::new(CacheBloom::new(
            config.cache.bloom_bits,
            config.cache.bloom_expected,
            config.cache.bloom_shards,
        ));
        let l1 = Arc::new(L1Cache::new(config.cache.l1_capacity, config.cache.ttl_seconds));
        let l2 = Arc::new(L2Cache::new(config.cache.l2_capacity, config.cache.ttl_seconds));
        let manager = RedisConnectionManager::new(config.database_urls[0].clone())
            .expect("Invalid Redis URL");
        let db_pool = Pool::builder()
            .max_size(config.cache.redis_pool_size)
            .idle_timeout(Some(std::time::Duration::from_secs(300)))
            .build(manager)
            .await
            .expect("Failed to build Redis pool");
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            config.database_urls.clone(),
            config.cache.max_failures,
            std::time::Duration::from_secs(config.cache.retry_interval_secs),
        ));
        // Spawn background task for circuit breaker reset
        let cb = Arc::clone(&circuit_breaker);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                cb.reset_unhealthy().await;
            }
        });
        Self {
            l1,
            l2,
            bloom,
            db_pool,
            circuit_breaker,
            ttl_seconds: config.cache.ttl_seconds,
        }
    }

    pub async fn get(&self, key: &str) -> Result<String, AppError> {
        let start = Instant::now();

        // L1 Cache Check
        if let Some(val) = self.l1.get(key).await {
            metrics::CACHE_HITS.get().unwrap().with_label_values(&["l1"]).inc();
            metrics::CACHE_LATENCY.get().unwrap()
                .with_label_values(&["l1"])
                .observe(start.elapsed().as_secs_f64());
            return Ok(val);
        }

        // Bloom Filter Check
        if !self.bloom.contains(key.as_bytes()) {
            metrics::CACHE_LATENCY.get().unwrap()
                .with_label_values(&["bloom"])
                .observe(start.elapsed().as_secs_f64());
            return Err(AppError::NotFound("Not found in Bloom filter".to_string()));
        }

        // L2 Cache Check
        if let Some(val) = self.l2.get(key).await {
            self.l1.insert(key.to_string(), val.clone()).await;
            return Ok(val); // Metrics handled in L2Cache::get
        }

        // Database Fallback
        let node = self.circuit_breaker.get_healthy_node().await
            .ok_or_else(|| AppError::CircuitBreaker("No healthy nodes".to_string()))?;
        let mut conn = match self.db_pool.get().await {
            Ok(c) => c,
            Err(e) => {
                self.circuit_breaker.record_failure(&node).await;
                metrics::DB_ERRORS.get().unwrap().with_label_values(&["get"]).inc();
                error!("Redis connection error: {:?}", e);
                return Err(AppError::RedisConnection);
            }
        };

        let start_db = Instant::now();
        let result: Option<Vec<u8>> = match conn.get_ex(key, Expiry::EX(self.ttl_seconds)).await {
            Ok(r) => r,
            Err(e) => {
                self.circuit_breaker.record_failure(&node).await;
                metrics::DB_ERRORS.get().unwrap().with_label_values(&["get"]).inc();
                error!("Redis GET error: {:?}", e);
                return Err(AppError::RedisOperation("Redis GET failed".to_string()));
            }
        };

        let url = result
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .ok_or_else(|| AppError::NotFound("Key not found".to_string()))?;

        metrics::DB_LATENCY.get().unwrap()
            .with_label_values(&["get"])
            .observe(start_db.elapsed().as_secs_f64());

        self.l2.insert(key.to_string(), url.clone()).await;
        self.l1.insert(key.to_string(), url.clone()).await;
        self.bloom.insert(key.as_bytes());
        info!("Cache miss for key {}, fetched from DB", key);
        Ok(url)
    }

    pub async fn warmup(&self, keys: Vec<String>) {
        let start = Instant::now();
        let futures = keys.into_iter().map(|key| {
            let bloom = Arc::clone(&self.bloom);
            let l1 = Arc::clone(&self.l1);
            let l2 = Arc::clone(&self.l2);
            let pool = self.db_pool.clone();
            async move {
                let start_db = Instant::now();
                if let Ok(mut conn) = pool.get().await {
                    if let Ok(Some(bytes)) = conn.get(&key).await {
                        let url = String::from_utf8_lossy(&bytes).into_owned();
                        l2.insert(key.clone(), url.clone()).await;
                        l1.insert(key.clone(), url.clone()).await;
                        bloom.insert(key.as_bytes());
                        metrics::CACHE_HITS.get().unwrap().with_label_values(&["db"]).inc();
                        metrics::DB_LATENCY.get().unwrap()
                            .with_label_values(&["get"])
                            .observe(start_db.elapsed().as_secs_f64());
                    }
                }
            }
        });
        future::join_all(futures).await;
        metrics::CACHE_LATENCY.get().unwrap()
            .with_label_values(&["warmup"])
            .observe(start.elapsed().as_secs_f64());
    }
}