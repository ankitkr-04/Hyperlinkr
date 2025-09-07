use crossbeam_queue::SegQueue;
use tokio::time::{interval, Duration};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Instant;
use crate::config::settings::Settings;
use crate::services::cache::circuit_breaker::CircuitBreaker;
use crate::services::metrics;
use crate::services::storage::dragonfly::DatabaseClient;
use crate::services::sled::SledStorage;
use crate::services::storage::storage::Storage;
use crate::errors::AppError;
use crate::clock::{Clock, SystemClock};
use tracing::{error, info};
use tokio::task::JoinHandle;

#[derive(Debug)]
#[allow(dead_code)]
pub enum AnalyticsMessage {
    Click {
        code: String,
        timestamp: u64,
        ip: String,
        referrer: Option<String>,
        country: Option<String>,
        device_type: Option<String>,
        browser: Option<String>,
    },
    Shutdown,
}

pub struct AnalyticsService<C: Clock + Send + Sync + 'static = SystemClock> {
    queue: Arc<SegQueue<AnalyticsMessage>>,
    flush_task: Arc<tokio::sync::Mutex<Option<JoinHandle<()>>>>,
    max_queue_size: usize,
    db: Arc<DatabaseClient>,
    sled: Option<Arc<SledStorage<C>>>, // Generic Sled service with specific path
    is_shutdown: Arc<AtomicBool>,
    clock: C,
    use_sled: bool,
    #[allow(dead_code)]
    sled_flush_ms: u64,
}

impl<C: Clock + Send + Sync + 'static> AnalyticsService<C> {
    pub async fn new(config: &Settings, circuit_breaker: Arc<CircuitBreaker>, clock: C) -> Self {
        let queue = Arc::new(SegQueue::new());
        let max_queue_size = config.analytics.max_queue_size.unwrap_or(100_000);
        let db = Arc::new(DatabaseClient::new(config, Arc::clone(&circuit_breaker)).await.unwrap());
        let sled = if config.cache.use_sled {
            // Create analytics-specific sled with the analytics path
            Some(Arc::new(SledStorage::with_clock(&config.analytics.sled_path, config, clock.clone())))
        } else {
            None
        };
        let flush_task = Self::start_flush_task(
            Arc::clone(&queue),
            config,
            Arc::clone(&db),
            sled.clone(),
        ).await;

        Self {
            queue,
            flush_task: Arc::new(tokio::sync::Mutex::new(Some(flush_task))),
            max_queue_size,
            db,
            sled,
            is_shutdown: Arc::new(AtomicBool::new(false)),
            clock,
            use_sled: config.cache.use_sled,
            sled_flush_ms: config.cache.sled_flush_ms,
        }
    }

    pub async fn record_click(
        &self,
        code: &str,
        ip: &str,
        referrer: Option<&str>,
        country: Option<&str>,
        device_type: Option<&str>,
        browser: Option<&str>,
    ) {
        if self.queue.len() >= self.max_queue_size {
            error!("Dropped click for code {}: queue full", code);
            metrics::record_analytics_dropped();
            metrics::update_queue_length(self.queue.len() as u64);
            return;
        }
        let timestamp = self.clock.now().timestamp() as u64;
        self.queue.push(AnalyticsMessage::Click {
            code: code.to_string(),
            timestamp,
            ip: ip.to_string(),
            referrer: referrer.map(String::from),
            country: country.map(String::from),
            device_type: device_type.map(String::from),
            browser: browser.map(String::from),
        });
        metrics::record_click();
        metrics::update_queue_length(self.queue.len() as u64);
    }

    pub async fn get_analytics(&self, code: &str, start: i64, end: i64) -> Result<Vec<(u64, u64)>, AppError> {
        let key = format!("stats:{}", code);
        match self.db.zrange(&key, start, end).await {
            Ok(data) if !data.is_empty() => Ok(data),
            _ => {
                if self.use_sled {
                    if let Some(sled) = &self.sled {
                        match sled.zrange(&key, start, end).await {
                            Ok(data) if !data.is_empty() => {
                                let operations = data.iter().map(|(score, member)| (key.clone(), *score, *member)).collect();
                                if let Err(e) = self.db.zadd_batch(operations, 90 * 24 * 3600).await {
                                    error!("Failed to restore analytics to DragonflyDB: {}", e);
                                    metrics::record_analytics_error("restore");
                                }
                                Ok(data)
                            }
                            Ok(_) => Ok(vec![]),
                            Err(e) => {
                                metrics::record_analytics_error("zrange_sled");
                                Err(e)
                            }
                        }
                    } else {
                        Ok(vec![])
                    }
                } else {
                    Ok(vec![])
                }
            }
        }
    }

    pub async fn shutdown(&self) {
        if self.is_shutdown.swap(true, Ordering::SeqCst) {
            return;
        }
        self.queue.push(AnalyticsMessage::Shutdown);
        if let Some(task) = self.flush_task.lock().await.take() {
            if let Err(e) = task.await {
                error!("Flush task failed: {}", e);
                metrics::record_analytics_error("shutdown");
            }
        }
    }

    async fn start_flush_task(
        queue: Arc<SegQueue<AnalyticsMessage>>,
        config: &Settings,
        db: Arc<DatabaseClient>,
        sled: Option<Arc<SledStorage<C>>>,
    ) -> JoinHandle<()> {
        let batch_size = config.analytics.max_batch_size;
        let batch_time_ms = config.cache.sled_flush_ms; // Use sled_flush_ms for consistency
        let use_sled = config.cache.use_sled;

        tokio::spawn(async move {
            let mut batch = Vec::with_capacity(batch_size);
            let mut interval = interval(Duration::from_millis(batch_time_ms));
            loop {
                interval.tick().await;
                while let Some(msg) = queue.pop() {
                    match msg {
                        AnalyticsMessage::Click { code, timestamp, ip: _, referrer: _, country: _, device_type: _, browser: _ } => {
                            batch.push((code, timestamp));
                            if batch.len() >= batch_size {
                                Self::flush_batch(&db, &sled, &mut batch, use_sled).await;
                            }
                        }
                        AnalyticsMessage::Shutdown => {
                            if !batch.is_empty() {
                                Self::flush_batch(&db, &sled, &mut batch, use_sled).await;
                            }
                            return;
                        }
                    }
                }
                if !batch.is_empty() {
                    Self::flush_batch(&db, &sled, &mut batch, use_sled).await;
                }
            }
        })
    }

    async fn flush_batch(db: &Arc<DatabaseClient>, sled: &Option<Arc<SledStorage<C>>>, batch: &mut Vec<(String, u64)>, use_sled: bool) {
        if batch.is_empty() {
            return;
        }
        let start = Instant::now();
        let operations: Vec<(String, u64, u64)> = batch
            .iter()
            .map(|(code, ts)| (format!("stats:{}", code), *ts, *ts))
            .collect();

        let dragonfly_result = db.zadd_batch(operations.clone(), 90 * 24 * 3600).await;
        if let Err(ref e) = dragonfly_result {
            error!("Failed to flush analytics to DragonflyDB: {}", e);
            metrics::record_analytics_error("flush_dragonfly");
        }

        let mut sled_success = false;
        if use_sled {
            if let Some(sled) = sled {
                let sled_result = sled.zadd_batch(operations, 0).await; // No expiry in Sled
                if let Err(e) = sled_result {
                    error!("Failed to flush analytics to Sled: {}", e);
                    metrics::record_analytics_error("flush_sled");
                } else {
                    sled_success = true;
                }
            }
        } else {
            sled_success = true; // No Sled operation needed
        }

        if dragonfly_result.is_ok() || sled_success {
            info!("Flushed {} analytics events in {:?}", batch.len(), start.elapsed());
            metrics::record_batch_flush(batch.len());
            batch.clear();
        } else {
            metrics::record_analytics_error("flush_failed");
        }
    }
}

impl<C: Clock + Send + Sync + 'static> Drop for AnalyticsService<C> {
    fn drop(&mut self) {
        if self.is_shutdown.load(Ordering::SeqCst) {
            return;
        }
        let queue = Arc::clone(&self.queue);
        let flush_task = Arc::clone(&self.flush_task);
        let db = Arc::clone(&self.db);
        let sled = self.sled.clone();
        let use_sled = self.use_sled;
        tokio::spawn(async move {
            let mut batch = Vec::with_capacity(1000);
            while let Some(msg) = queue.pop() {
                if let AnalyticsMessage::Click { code, timestamp, .. } = msg {
                    batch.push((code, timestamp));
                    if batch.len() >= 1000 {
                        Self::flush_batch(&db, &sled, &mut batch, use_sled).await;
                    }
                }
            }
            if !batch.is_empty() {
                Self::flush_batch(&db, &sled, &mut batch, use_sled).await;
            }
            if let Some(task) = flush_task.lock().await.take() {
                if let Err(e) = task.await {
                    error!("Flush task failed on drop: {}", e);
                    metrics::record_analytics_error("drop");
                }
            }
        });
    }
}