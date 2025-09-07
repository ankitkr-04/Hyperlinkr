use std::{sync::Arc, time::Instant};
use futures::future;
use tracing::info;
use tokio::time::Duration;
use once_cell::sync::Lazy;
use prometheus::IntCounter;
use crate::{
    config::settings::Settings,
    errors::AppError,
    services::{
        cache::{
            bloom_filter::bloom::CacheBloom,
            circuit_breaker::CircuitBreaker,
            l1_cache::L1Cache,
            l2_cache::L2Cache,
        },
        metrics,
        storage::{dragonfly::DatabaseClient, storage::Storage},
        sled::SledStorage,
    },
    types::{Paginate, UrlData},
};

use std::pin::Pin;
use std::future::Future;

#[derive(Clone)]
pub struct CacheService {
    l1: Arc<L1Cache>,
    l2: Arc<L2Cache>,
    bloom: Arc<CacheBloom>,
    dragonfly: Arc<DatabaseClient>,
    sled: Option<Arc<SledStorage>>, // Optional Sled
    ttl_seconds: u64,
    use_sled: bool,
    sled_flush_ms: u64,
}

static FLUSH_COUNT: Lazy<IntCounter> = Lazy::new(|| {
    prometheus::register_int_counter!("flush_count_total", "Total Sled flushes").unwrap()
});

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
        let dragonfly = Arc::new(
            DatabaseClient::new(config, Arc::clone(&circuit_breaker))
                .await
                .expect("Failed to create DatabaseClient"),
        );
        let sled = if config.cache.use_sled {
            Some(Arc::new(SledStorage::new(&config.cache.sled_path, config)))
        } else {
            None
        };
        let cache = Self {
            l1,
            l2,
            bloom,
            dragonfly,
            sled,
            ttl_seconds: config.cache.ttl_seconds,
            use_sled: config.cache.use_sled,
            sled_flush_ms: config.cache.sled_flush_ms,
        };

        // Start flush task if Sled is enabled
        if cache.use_sled {
            let cache = cache.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_millis(cache.sled_flush_ms));
                loop {
                    interval.tick().await;
                    if let Err(e) = cache.flush_to_sled().await {
                        tracing::error!("Flush to Sled failed: {}", e);
                    }
                    FLUSH_COUNT.inc();
                }
            });
        }

        cache
    }

    pub async fn get(&self, key: &str) -> Result<String, AppError> {
        let start = Instant::now();
        if let Some(val) = self.l1.get(key).await {
            metrics::record_cache_hit("l1", start);
            return Ok(val);
        }

        if !self.bloom.contains(key.as_bytes()) {
            metrics::record_cache_latency("bloom", start);
            return Err(AppError::NotFound("Key not found".into()));
        }

        if let Some(val) = self.l2.get(key).await {
            metrics::record_cache_hit("l2", start);
            self.l1.insert(key.to_string(), val.clone()).await;
            return Ok(val);
        }

        if let Ok(val) = self.dragonfly.get(key).await {
            metrics::record_cache_hit("dragonfly", start);
            let key = key.to_string();
            let val_clone = val.clone();
            let l1_task = {
                let key = key.clone();
                let val_clone = val_clone.clone();
                async move {
                    self.l1.insert(key, val_clone).await;
                    Ok::<(), AppError>(())
                }
            };
            let l2_task = {
                let key = key.clone();
                let val_clone = val_clone.clone();
                async move {
                    self.l2.insert(key, val_clone).await;
                    Ok::<(), AppError>(())
                }
            };
            

            let tasks: Vec<Pin<Box<dyn Future<Output = Result<(), AppError>> + Send>>> = vec![
                Box::pin(l1_task),
                Box::pin(l2_task),
            ];
            future::try_join_all(tasks).await?;
            return Ok(val);
        }

        if self.use_sled {
            if let Some(sled) = &self.sled {
                let sled_start = Instant::now();
                let url = sled.get(key).await?;
                metrics::record_cache_latency("sled", sled_start);
                let key = key.to_string();
                let url_clone = url.clone();
                let dragonfly_task = self.dragonfly.set_ex(&key, &url_clone, self.ttl_seconds);
                let l1_task = {
                    let key = key.clone();
                    let url_clone = url_clone.clone();
                    async move {
                        self.l1.insert(key, url_clone).await;
                        Ok::<(), AppError>(())
                    }
                };
                let l2_task = {
                    let key = key.clone();
                    let url_clone = url_clone.clone();
                    async move {
                        self.l2.insert(key, url_clone).await;
                        Ok::<(), AppError>(())
                    }
                };
                let bloom_task = {
                    let key = key.clone();
                    async move {
                        self.bloom.insert(key.as_bytes());
                        Ok::<(), AppError>(())
                    }
                };

                let tasks: Vec<Pin<Box<dyn Future<Output = Result<(), AppError>> + Send>>> = vec![
                    Box::pin(dragonfly_task),
                    Box::pin(l1_task),
                    Box::pin(l2_task),
                    Box::pin(bloom_task),
                ];
                future::try_join_all(tasks).await?;
                metrics::record_cache_latency("total", start);
                return Ok(url);
            }
        }

        Err(AppError::NotFound("Key not found".into()))
    }

    pub async fn insert(&self, key: String, value: String) -> Result<(), AppError> {
        let start = Instant::now();
        self.dragonfly.set_ex(&key, &value, self.ttl_seconds).await?;
        let value_clone = value.clone();
        let l1_task = {
            let key = key.clone();
            let value_clone = value_clone.clone();
            async move {
                self.l1.insert(key, value_clone).await;
                Ok::<(), AppError>(())
            }
        };
        let l2_task = {
            let key = key.clone();
            let value_clone = value_clone.clone();
            async move {
                self.l2.insert(key, value_clone).await;
                Ok::<(), AppError>(())
            }
        };
        let bloom_task = {
            let key = key.clone();
            async move {
                self.bloom.insert(key.as_bytes());
                Ok::<(), AppError>(())
            }
        };

        let mut tasks: Vec<Pin<Box<dyn Future<Output = Result<(), AppError>> + Send>>> = vec![
            Box::pin(l1_task),
            Box::pin(l2_task),
            Box::pin(bloom_task),
        ];
        if self.use_sled {
            if let Some(sled) = &self.sled {
                let sled_task: Pin<Box<dyn Future<Output = Result<(), AppError>> + Send>> = Box::pin(sled.set_ex(&key, &value, self.ttl_seconds));
                tasks.push(sled_task);
            }
        }
        future::try_join_all(tasks).await?;
        metrics::record_cache_latency("insert", start);
        Ok(())
    }

    pub async fn delete(&self, key: &str) -> Result<(), AppError> {
        let start = Instant::now();
        let dragonfly_task = self.dragonfly.delete_url(key, None, "");
        let l1_task = async move {
            self.l1.remove(key).await;
            Ok::<(), AppError>(())
        };
        let l2_task = async move {
            self.l2.remove(key).await;
            Ok::<(), AppError>(())
        };

        let mut tasks: Vec<Pin<Box<dyn Future<Output = Result<(), AppError>> + Send>>> = vec![
            Box::pin(dragonfly_task),
            Box::pin(l1_task),
            Box::pin(l2_task),
        ];
        if self.use_sled {
            if let Some(sled) = &self.sled {
                let sled_task: Pin<Box<dyn Future<Output = Result<(), AppError>> + Send>> = Box::pin(sled.delete_url(key, None, ""));
                tasks.push(sled_task);
            }
        }
        future::try_join_all(tasks).await?;
        metrics::record_cache_latency("delete", start);
        Ok(())
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.bloom.contains(key.as_bytes())
    }

    async fn flush_to_sled(&self) -> Result<(), AppError> {
        if !self.use_sled || self.sled.is_none() {
            return Ok(());
        }
        let sled = self.sled.as_ref().unwrap();
        let start = Instant::now();
        let count = 1000;
        let keys = self.dragonfly.scan_keys("url:*", count).await?;
        let tasks = keys.iter().map(|key| {
            let dragonfly = Arc::clone(&self.dragonfly);
            let sled = Arc::clone(sled);
            let ttl = self.ttl_seconds;
            async move {
                if let Ok(value) = dragonfly.get(key).await {
                    sled.set_ex(key, &value, ttl).await?;
                }
                Ok::<(), AppError>(())
            }
        });
        future::try_join_all(tasks).await?;
        metrics::record_cache_latency("flush", start);
        info!("Flushed keys to Sled in {:?}", start.elapsed());
        Ok(())
    }

    pub async fn list_urls_cache(&self, user_id: Option<&str>, page: u64, per_page: u64) -> Result<Option<Paginate<UrlData>>, AppError> {
        let cache_key = format!("urls:{}:{}:{}", user_id.unwrap_or("all"), page, per_page);
        if let Some(cached) = self.l2.get(&cache_key).await {
            let result: Paginate<UrlData> = serde_json::from_str(&cached)
                .map_err(|e| AppError::Internal(e.to_string()))?;
            return Ok(Some(result));
        }
        Ok(None)
    }

    pub async fn cache_list_urls(&self, user_id: Option<&str>, page: u64, per_page: u64, result: &Paginate<UrlData>) -> Result<(), AppError> {
        let cache_key = format!("urls:{}:{}:{}", user_id.unwrap_or("all"), page, per_page);
        let serialized = serde_json::to_string(result)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        self.l2.insert(cache_key, serialized).await;
        Ok(())
    }

    pub async fn warmup(&self, keys: Vec<String>) {
        let start = Instant::now();
        let chunks: Vec<_> = keys.chunks(1000).collect();
        let tasks = chunks.into_iter().map(|chunk| {
            let l1 = Arc::clone(&self.l1);
            let l2 = Arc::clone(&self.l2);
            let bloom = Arc::clone(&self.bloom);
            let dragonfly = Arc::clone(&self.dragonfly);
            let sled = self.sled.clone();
            let ttl = self.ttl_seconds;
            let use_sled = self.use_sled;
            async move {
                let tasks = chunk.iter().map(|key| {
                    let l1 = Arc::clone(&l1);
                    let l2 = Arc::clone(&l2);
                    let bloom = Arc::clone(&bloom);
                    let dragonfly = Arc::clone(&dragonfly);
                    let sled = sled.clone();
                    let key = key.clone();
                    async move {
                        let op_start = Instant::now();
                        if let Ok(url) = dragonfly.get(&key).await {
                            l2.insert(key.clone(), url.clone()).await;
                            l1.insert(key.clone(), url.clone()).await;
                            bloom.insert(key.as_bytes());
                            metrics::record_cache_hit("warmup", op_start);
                        } else if use_sled {
                            if let Some(sled) = sled.as_ref() {
                                if let Ok(url) = sled.get(&key).await {
                                    dragonfly.set_ex(&key, &url, ttl).await.ok();
                                    l2.insert(key.clone(), url.clone()).await;
                                    l1.insert(key.clone(), url.clone()).await;
                                    bloom.insert(key.as_bytes());
                                    metrics::record_cache_hit("warmup", op_start);
                                }
                            }
                        }
                    }
                });
                future::join_all(tasks).await;
            }
        });
        future::join_all(tasks).await;
        metrics::record_cache_latency("warmup", start);
        info!("Cache warmup completed in {:?}", start.elapsed());
    }
}