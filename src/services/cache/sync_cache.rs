use std::sync::Arc;
use moka::sync::Cache;

#[derive(Clone)]
pub struct SyncCache {
    inner: Arc<Cache<String, String>>,
}

impl SyncCache {
    pub fn new(capacity: usize, ttl_seconds: u64) -> Self {
        #[cfg(feature = "libnuma")]
        unsafe {
            if libnuma_sys::numa_available() >= 0 {
                let _ = libnuma_sys::numa_alloc_onnode(0, 0);
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
    pub fn get(&self, key: &str) -> Option<String> {
        self.inner.get(key)
    }

    #[inline]
    pub fn insert(&self, key: String, value: String) {
        self.inner.insert(key, value);
    }
}