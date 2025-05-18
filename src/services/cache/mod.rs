pub mod bloom;
pub mod moka_cache;
pub mod metrics;
pub mod circuit_breaker;

use crate::config::cache::CacheConfig;
use crate::services::storage::dragonfly::DatabaseClient;
use std::sync::Arc;
use parking_lot::RwLock;
use bloom::CacheBloom;
use moka_cache::MokaCache;
use circuit_breaker::CircuitBreaker;
use tokio::sync::Mutex;

pub struct CacheService {
    pub l1: Arc<MokaCache>,
    pub bloom: Arc<RwLock<CacheBloom>>,
    pub db: Arc<Mutex<DatabaseClient>>,
    pub circuit_breaker: Arc<CircuitBreaker>,
}

impl CacheService {
    pub async fn new(config: &CacheConfig, db: DatabaseClient) -> Self {
        let bloom = Arc::new(RwLock::new(CacheBloom::new(
            config.bloom_bits,
            config.bloom_expected,
        )));

        Self {
            l1: Arc::new(MokaCache::new(config.l1_capacity)),
            bloom,
            db: Arc::new(Mutex::new(db)),
            circuit_breaker: Arc::new(CircuitBreaker::new()),
        }
    }

    pub async fn get(&self, key: &str) -> Result<String, &'static str> {
        let start = std::time::Instant::now();

        if let Some(url) = self.l1.get(key).await {
            return Ok(url);
        }

        if !self.bloom.read().contains(key) {
            metrics::CACHE_LATENCY
                .with_label_values(&["bloom"])
                .observe(start.elapsed().as_secs_f64());
            return Err("Not found in Bloom filter");
        }

        if !self.circuit_breaker.should_try().await {
            return Err("Circuit breaker open");
        }

        let mut db = self.db.lock().await;
        match db.get(key).await {
            Ok(url) => {
                self.l1.insert(key.to_string(), url.clone()).await;
                self.bloom.write().insert(key);
                metrics::CACHE_LATENCY
                    .with_label_values(&["db"])
                    .observe(start.elapsed().as_secs_f64());
                Ok(url)
            }
            Err(e) => {
                self.circuit_breaker.record_failure().await;
                Err(e)
            }
        }
    }

    pub async fn warmup(&self, keys: Vec<String>) {
        let futures = keys.into_iter().map(|key| {
            let db = Arc::clone(&self.db);
            let bloom = Arc::clone(&self.bloom);
            let l1 = Arc::clone(&self.l1);
            async move {
                let mut db = db.lock().await;
                if let Ok(url) = db.get(&key).await {
                    l1.insert(key.clone(), url.clone()).await;
                    bloom.write().insert(&key);
                }
            }
        });
        futures::future::join_all(futures).await;
    }
}