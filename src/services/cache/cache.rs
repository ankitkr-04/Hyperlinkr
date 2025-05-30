use std::{sync::Arc, time::Instant};
use futures::future;
use tracing::info;
use tokio::time::Duration;
use crate::{
    config::settings::Settings,
    errors::AppError,
    services::{
        storage::{storage::Storage, dragonfly::DatabaseClient},
        cache::{
            bloom_filter::bloom::CacheBloom,
            l1_cache::L1Cache,
            l2_cache::L2Cache,
            circuit_breaker::CircuitBreaker,
           
        },
        metrics,
    },
};

pub struct CacheService {
    l1: Arc<L1Cache>,
    l2: Arc<L2Cache>,
    bloom: Arc<CacheBloom>,
    db: Arc<DatabaseClient>,
    ttl_seconds: u64,
}

impl CacheService {
    pub async fn new(config: &Settings) -> Self {
        metrics::init_metrics();
        let bloom = Arc::new(CacheBloom::new(
            config.cache.bloom_bits,
            config.cache.bloom_expected,
            config.cache.bloom_shards,
        ));
        let l1 = Arc::new(L1Cache::new(
            config.cache.l1_capacity,
            config.cache.ttl_seconds,
        ));
        let l2 = Arc::new(L2Cache::new(
            config.cache.l2_capacity,
            config.cache.ttl_seconds,
        ));
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            config.database_urls.clone(),
            config.cache.max_failures,
            Duration::from_secs(config.cache.retry_interval_secs),
        ));
        let db = Arc::new(
            DatabaseClient::new(config, Arc::clone(&circuit_breaker))
                .await
                .expect("Failed to create DatabaseClient"),
        );
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

    pub async fn get(&self, key: &str) -> Result<String, AppError> {
        let start = Instant::now();
        if let Some(val) = self.l1.get(key).await {
            metrics::record_cache_hit("l1", start);
            return Ok(val);
        }

       if !self.bloom.contains(key.as_bytes()) {
            metrics::record_cache_latency("bloom", start);
            return Err(AppError::NotFound("Bloom says no".into()));
        }

        if let Some(val) = self.l2.get(key).await {
            metrics::record_cache_hit("l2", start);
            self.l1.insert(key.to_string(), val.clone()).await;
            return Ok(val);
        }
        let db_start = Instant::now();
        let url = self.db.get(key).await?;
        metrics::record_cache_latency("db", db_start);
        self.l2.insert(key.to_string(), url.clone()).await;
        self.l1.insert(key.to_string(), url.clone()).await;
        self.bloom.insert(key.as_bytes());

        info!("Cache miss for key {}: fetched from DB", key);
        Ok(url)
    }

    pub async fn insert(&self, key: String, value: String) -> Result<(), AppError> {
        self.l1.insert(key.clone(), value.clone()).await;
        self.l2.insert(key.clone(), value.clone()).await;
        self.bloom.insert(key.as_bytes());
        self.db.set_ex(&key, &value, self.ttl_seconds).await?;
        Ok(())
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.bloom.contains(key.as_bytes())
    }

    pub async fn warmup(&self, keys: Vec<String>) {
        let start = Instant::now();
        let tasks = keys.into_iter().map(|key| {
            
            let l1 = Arc::clone(&self.l1);
            let l2 = Arc::clone(&self.l2);
            let bloom = Arc::clone(&self.bloom);
            let db = Arc::clone(&self.db);

            async move {
                let op_start = Instant::now();
                if let Ok(url) = db.get(&key).await {
                    l2.insert(key.clone(), url.clone()).await;
                    l1.insert(key.clone(), url.clone()).await;
                    bloom.insert(key.as_bytes());
                   metrics::record_cache_hit("warmup", op_start);
                }
            }
        });
        future::join_all(tasks).await;
       metrics::record_cache_latency("warmup", start);
        info!("Cache warmup completed in {:?}", start.elapsed());
    }
}
