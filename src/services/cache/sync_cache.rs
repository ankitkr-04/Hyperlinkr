use std::sync::Arc;
use moka::sync::Cache;

#[derive(Clone)]
pub struct SyncCache {
  inner: Arc<Cache<String, String>>,
}

impl SyncCache {
  pub fn new(capacity: usize) -> Self {
   
  #[cfg(feature = "libnuma")]
  unsafe  {
    if libnuma_sys::numa_available() >= 0 {
      let _ = libnuma_sys::numa_alloc_onnode(0,0);
    }
  }

   Self {
      inner: Arc::new(Cache::builder()
        .max_capacity(capacity as u64)
        .time_to_idle(std::time::Duration::from_secs(60))
        .eviction_policy(moka::policy::EvictionPolicy::tiny_lfu())
        .build()),
    }

  }

  #[inline(always)]
  pub fn get(&self, key: &str) -> Option<String> {
    // Optional Prefetching
    // std::intrinsics::prefetch_read_data(key.as_ptr(), 3);
    self.inner.get(key)
  }

  #[inline]
  pub fn insert(&self, key: String, value: String) {
    self.inner.insert(key, value);
  }
}

  
