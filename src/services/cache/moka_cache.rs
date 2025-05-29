use moka::future::Cache;
use std::time::Duration;
use crate::services::cache::metrics;

pub struct MokaCache {
    pub inner: Cache<String, String>,
}

impl MokaCache {
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
            metrics::CACHE_HITS.with_label_values(&["l2"]).inc();
            metrics::CACHE_LATENCY
                .with_label_values(&["l2"])
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

// use dashmap::DashMap;
// use std::sync::Arc;
// use crate::config::settings::Settings;
// use prometheus::{IntCounter, Histogram};

// lazy_static! {
//     static ref CACHE_HITS: IntCounter = prometheus::register_int_counter!(
//         "cache_hits_total",
//         "Total number of cache hits"
//     ).unwrap();
//     static ref CACHE_LATENCY: Histogram = prometheus::register_histogram!(
//         "cache_latency_seconds",
//         "Cache operation latency in seconds"
//     ).unwrap();
// }

// // #[derive(Clone)]
// pub struct MokaCache {
//     inner: Arc<DashMap<String, String>>,
// }

// impl MokaCache {
//     pub fn new(config: &Settings) -> Self {
//         let inner = Arc::new(DashMap::with_capacity(config.cache.l2_capacity));
//         Self { inner }
//     }

//     pub fn get(&self, key: &str) -> Option<String> {
//         let timer = CACHE_LATENCY.start_timer();
//         let result = self.inner.get(key).map(|v| v.clone());
//         if result.is_some() {
//             CACHE_HITS.inc();
//         }
//         timer.stop_and_record();
//         result
//     }

//     pub async fn insert(&self, key: String, value: String) {
//         let timer = CACHE_LATENCY.start_timer();
//         self.inner.insert(key, value);
//         timer.stop_and_record();
//     }
// }