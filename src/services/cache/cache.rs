use std::{sync::Arc, time::Instant};
use futures::future;
use tracing::{info, error};

use tokio::time::Duration;
use crate::{
    config::settings::Settings,
    errors::AppError,
    services::{
        storage::{storage::Storage, dragonfly::DatabaseClient},
        cache::{
            bloom::CacheBloom,
            l1_cache::L1Cache,
            l2_cache::L2Cache,
            circuit_breaker::CircuitBreaker,
            metrics,
        },
    },
};

/// High-level cache service combining L1, L2, bloom filter, and DB with circuit breaker
pub struct CacheService {
    l1: Arc<L1Cache>,
    l2: Arc<L2Cache>,
    bloom: Arc<CacheBloom>,
    db: Arc<DatabaseClient>,
    ttl_seconds: u64,
}

impl CacheService {
    /// Initialize caches, bloom filter, DB client, and metrics
    pub async fn new(config: &Settings) -> Self {
        // Initialize Prometheus metrics
        metrics::init_metrics();

        // Bloom filter layer
        let bloom = Arc::new(CacheBloom::new(
            config.cache.bloom_bits,
            config.cache.bloom_expected,
            config.cache.bloom_shards,
        ));

        // In-memory L1 cache
        let l1 = Arc::new(L1Cache::new(
            config.cache.l1_capacity,
            config.cache.ttl_seconds,
        ));

        // Shared L2 cache
        let l2 = Arc::new(L2Cache::new(
            config.cache.l2_capacity,
            config.cache.ttl_seconds,
        ));

        // Circuit breaker for DB
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            config.database_urls.clone(),
            config.cache.max_failures,
            Duration::from_secs(config.cache.retry_interval_secs),
        ));

        // Database client
        let db = Arc::new(
            DatabaseClient::new(config, Arc::clone(&circuit_breaker))
                .await
                .expect("Failed to create DatabaseClient"),
        );

        // Background task to reset circuit breaker periodically
        {
            let cb = Arc::clone(&circuit_breaker);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    cb.reset_unhealthy().await;
                }
            });
        }

        Self {
            l1,
            l2,
            bloom,
            db,
            ttl_seconds: config.cache.ttl_seconds,
        }
    }

    /// Retrieve a value by key using multi-layer cache pattern
    pub async fn get(&self, key: &str) -> Result<String, AppError> {
        let start = Instant::now();

        // 1. Try L1 cache
        if let Some(val) = self.l1.get(key).await {
            metrics::record_cache_hit("l1", start);
            return Ok(val);
        }

        // 2. Check bloom filter to avoid DB miss
        if !self.bloom.contains(key.as_bytes()) {
            metrics::record_cache_latency("bloom", start);
            return Err(AppError::NotFound("Not found in Bloom filter".into()));
        }

        // 3. Try L2 cache (metrics inside L2Cache)
        if let Some(val) = self.l2.get(key).await {
            // Populate L1 on L2 hit
            self.l1.insert(key.to_string(), val.clone()).await;
            return Ok(val);
        }

        // 4. Fallback to DB
        let db_start = Instant::now();
        let url = self.db.get(key).await?;
        metrics::record_db_latency("get", db_start);

        // Populate caches and bloom
        self.l2.insert(key.to_string(), url.clone()).await;
        self.l1.insert(key.to_string(), url.clone()).await;
        self.bloom.insert(key.as_bytes());

        info!("Cache miss for key {}: fetched from DB", key);
        Ok(url)
    }

    /// Pre-warm caches by loading keys from DB concurrently
    pub async fn warmup(&self, keys: Vec<String>) {
        let start = Instant::now();

        let tasks = keys.into_iter().map(|key| {
            let bloom = Arc::clone(&self.bloom);
            let l1 = Arc::clone(&self.l1);
            let l2 = Arc::clone(&self.l2);
            let db = Arc::clone(&self.db);

            async move {
                let op_start = Instant::now();
                if let Ok(url) = db.get(&key).await {
                    l2.insert(key.clone(), url.clone()).await;
                    l1.insert(key.clone(), url.clone()).await;
                    bloom.insert(key.as_bytes());
                    metrics::record_cache_hit("db", op_start);
                }
            }
        });

        future::join_all(tasks).await;
        metrics::record_cache_latency("warmup", start);
    }
}
