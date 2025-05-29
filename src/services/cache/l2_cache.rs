use moka::future::Cache;
use std::time::Duration;
use crate::services::metrics;

pub struct L2Cache {
    pub inner: Cache<String, String>,
}

impl L2Cache {
    pub fn new(capacity: usize, ttl_seconds: u64) -> Self {
        let inner = Cache::builder()
            .max_capacity(capacity as u64)
            .time_to_live(Duration::from_secs(ttl_seconds))
            .eviction_policy(moka::policy::EvictionPolicy::tiny_lfu())
            .build();
        Self { inner }
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        let start = std::time::Instant::now();
        let val = self.inner.get(key).await;
        if val.is_some() {
            metrics::CACHE_HITS.get().unwrap().with_label_values(&["l2"]).inc();
            metrics::CACHE_LATENCY
                .get().unwrap().with_label_values(&["l2"])
                .observe(start.elapsed().as_secs_f64());
        }
        val
    }

    pub async fn insert(&self, key: String, value: String) {
        self.inner.insert(key, value).await;
    }

    pub async fn remove(&self, key: &str) {
        self.inner.invalidate(key).await;
    }
}