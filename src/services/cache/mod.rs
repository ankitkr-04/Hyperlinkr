pub mod bloom;
pub mod moka_cache;
pub mod metrics;
pub mod circuit_breaker;
pub mod sync_cache;
use crate::config::settings::Settings;
use std::sync::Arc;
use parking_lot::RwLock;
use bb8::Pool;
use bb8_redis::RedisConnectionManager;
use redis::AsyncCommands;
use bloom::CacheBloom;
use moka_cache::MokaCache;
use circuit_breaker::CircuitBreaker;
use sync_cache::SyncCache;
use crate::errors::AppError;
use tracing::info;

pub struct CacheService {
    pub l1: Arc<SyncCache>,
    pub l2: Arc<MokaCache>,
    pub bloom: Arc<RwLock<CacheBloom>>,
    pub db_pool: Pool<RedisConnectionManager>,
    pub circuit_breaker: Arc<CircuitBreaker>,
    pub node_id: String,
    pub ttl_seconds: u64,
}

impl CacheService {
    pub async fn new(config: &Settings) -> Self {
        let bloom = Arc::new(RwLock::new(CacheBloom::new(
            config.cache.bloom_bits,
            config.cache.bloom_expected,
            config.cache.bloom_block_size,
        )));
        let l1 = Arc::new(SyncCache::new(config.cache.l1_capacity, config.cache.ttl_seconds));
        let l2 = Arc::new(MokaCache::new(config.cache.l2_capacity, config.cache.ttl_seconds));
        let manager = RedisConnectionManager::new(&config.database_url)
            .expect("Invalid Redis URL");
        let db_pool = Pool::builder()
            .max_size(config.cache.redis_pool_size)
            .idle_timeout(Some(std::time::Duration::from_secs(300)))
            .build(manager)
            .await
            .expect("Failed to build Redis pool");
        let circuit_breaker = Arc::new(CircuitBreaker::new(vec![config.database_url.clone()]));
        Self {
            l1,
            l2,
            bloom,
            db_pool,
            circuit_breaker,
            node_id: config.database_url.clone(),
            ttl_seconds: config.cache.ttl_seconds,
        }
    }

    pub async fn get(&self, key: &str) -> Result<String, AppError> {
        let start = std::time::Instant::now();
        if let Some(val) = self.l1.get(key) {
            metrics::CACHE_HITS.with_label_values(&["l1"]).inc();
            metrics::CACHE_LATENCY
                .with_label_values(&["l1"])
                .observe(start.elapsed().as_secs_f64());
            return Ok(val);
        }
        if !self.bloom.read().contains(key) {
            metrics::CACHE_LATENCY
                .with_label_values(&["bloom"])
                .observe(start.elapsed().as_secs_f64());
            return Err(AppError::NotFound("Not found in Bloom filter".to_string()));
        }
        if let Some(val) = self.l2.get(key).await {
            self.l1.insert(key.to_string(), val.clone());
            metrics::CACHE_HITS.with_label_values(&["l2"]).inc();
            metrics::CACHE_LATENCY
                .with_label_values(&["l2"])
                .observe(start.elapsed().as_secs_f64());
            return Ok(val);
        }
        if !self.circuit_breaker.should_try(&self.node_id).await {
            return Err(AppError::CircuitBreaker(self.node_id.clone()));
        }
        let mut conn = match self.db_pool.get().await {
            Ok(c) => c,
            Err(_) => {
                self.circuit_breaker.record_failure(&self.node_id).await;
                metrics::DB_ERRORS.with_label_values(&["get"]).inc();
                return Err(AppError::RedisConnection);
            }
        };
        let start_db = std::time::Instant::now();
        let result: Option<(Vec<u8>, u64)> = match conn.get_ex(key, self.ttl_seconds).await {
            Ok(r) => r,
            Err(_) => {
                self.circuit_breaker.record_failure(&self.node_id).await;
                metrics::DB_ERRORS.with_label_values(&["get"]).inc();
                return Err(AppError::RedisOperation("Redis GET failed".to_string()));
            }
        };
        let url = result
            .map(|(bytes, _)| String::from_utf8_lossy(&bytes).into_owned())
            .ok_or_else(|| AppError::NotFound("Key not found".to_string()))?;
        metrics::DB_LATENCY
            .with_label_values(&["get"])
            .observe(start_db.elapsed().as_secs_f64());
        self.l2.insert(key.to_string(), url.clone()).await;
        self.l1.insert(key.to_string(), url.clone());
        self.bloom.write().insert(key);
        info!("Cache miss for key {}, fetched from DB", key);
        Ok(url)
    }

    pub async fn warmup(&self, keys: Vec<String>) {
        let start = std::time::Instant::now();
        let futures = keys.into_iter().map(|key| {
            let bloom = Arc::clone(&self.bloom);
            let l1 = Arc::clone(&self.l1);
            let l2 = Arc::clone(&self.l2);
            let pool = self.db_pool.clone();
            async move {
                let start_db = std::time::Instant::now();
                if let Ok(mut conn) = pool.get().await {
                    if let Ok(Some(bytes)) = conn.get(&key).await {
                        let url = String::from_utf8_lossy(&bytes).into_owned();
                        l2.insert(key.clone(), url.clone()).await;
                        l1.insert(key.clone(), url.clone());
                        bloom.write().insert(&key);
                        metrics::CACHE_HITS.with_label_values(&["db"]).inc();
                        metrics::DB_LATENCY
                            .with_label_values(&["get"])
                            .observe(start_db.elapsed().as_secs_f64());
                    }
                }
            }
        });
        futures::future::join_all(futures).await;
        metrics::CACHE_LATENCY
            .with_label_values(&["warmup"])
            .observe(start.elapsed().as_secs_f64());
    }
}