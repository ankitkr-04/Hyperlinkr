use moka::future::Cache;
use std::time::Duration;
use crate::services::cache::metrics;

pub struct MokaCache {
    pub inner: Cache<String, String>,
}

impl MokaCache {
    pub fn new(capacity: usize) -> Self {
        let inner = Cache::builder()
            .max_capacity(capacity as u64)
            .time_to_idle(Duration::from_secs(60))
            .eviction_policy(moka::policy::EvictionPolicy::tiny_lfu())
            .build();
        Self { inner }
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        let start = std::time::Instant::now();
        let val = self.inner.get(key).await;
        if val.is_some() {
            metrics::CACHE_HITS.with_label_values(&["l1"]).inc();
            metrics::CACHE_LATENCY
                .with_label_values(&["l1"])
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