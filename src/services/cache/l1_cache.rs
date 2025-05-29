use std::sync::Arc;
use moka::future::Cache;
use crate::services::cache::metrics;

#[derive(Clone)]
pub struct L1Cache {
    inner: Arc<Cache<String, String>>,
}

impl L1Cache {
    pub fn new(capacity: usize, ttl_seconds: u64) -> Self {
        #[cfg(feature = "libnuma")]
        unsafe {
            if lib_numa::numa_available() >= 0 {
                let _ = libnuma_sys::numa_preferred();
            }
        }

        Self {
            inner: Arc::new(Cache::builder()
                .max_capacity(capacity as u64)
                .time_to_live(std::time::Duration::from_secs(ttl_seconds))
                .eviction_policy(moka::policy::EvictionPolicy::tiny_lfu())
                .build()),
        }
    }

    #[inline(always)]
    pub async fn get(&self, key: &str) -> Option<String> {
        let start = std::time::Instant::now();
        let val = self.inner.get(key).await;
        if val.is_some() {
            metrics::CACHE_HITS.get().unwrap().with_label_values(&["l1"]).inc();
            metrics::CACHE_LATENCY
                .get().unwrap().with_label_values(&["l1"])
                .observe(start.elapsed().as_secs_f64());
        }
        val
    }

    #[inline]
    pub async fn insert(&self, key: String, value: String) {
        self.inner.insert(key, value).await;
    }

    pub async fn remove(&self, key: &str) {
        self.inner.invalidate(key).await;
    }
}