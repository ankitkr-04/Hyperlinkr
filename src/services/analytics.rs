use crossbeam_queue::SegQueue;
use tokio::time::{interval, Duration};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Instant;
use crate::config::settings::Settings;
use crate::services::cache::circuit_breaker::CircuitBreaker;
use crate::services::metrics;
use crate::services::storage::dragonfly::DatabaseClient;
use crate::services::storage::sled::SledStorage;
use crate::services::storage::storage::Storage;
use crate::errors::AppError;
use crate::clock::{Clock, SystemClock};
use tracing::{error, info};
use tokio::task::JoinHandle;

#[derive(Debug)]
enum AnalyticsMessage {
    Click(String, u64),
    Shutdown,
}

pub struct AnalyticsService<C: Clock = SystemClock> {
    queue: Arc<SegQueue<AnalyticsMessage>>,
    flush_task: Arc<tokio::sync::Mutex<Option<JoinHandle<()>>>>,
    max_queue_size: usize,
    db: Arc<DatabaseClient>,
    sled: Arc<SledStorage<C>>,
    is_shutdown: Arc<AtomicBool>,
    clock: C,
}

impl<C: Clock + Send + Sync> AnalyticsService<C> {
    pub async fn new(config: &Settings, circuit_breaker: Arc<CircuitBreaker>, clock: C) -> Self {
        let queue = Arc::new(SegQueue::new());
        let max_queue_size = config.analytics.max_queue_size.unwrap_or(100_000);
        let db = Arc::new(DatabaseClient::new(config, Arc::clone(&circuit_breaker)).await.unwrap());
        let sled = Arc::new(SledStorage::with_clock(config, clock.clone()));
        let flush_task = Self::start_flush_task(Arc::clone(&queue), config, Arc::clone(&db), Arc::clone(&sled)).await;

        Self {
            queue,
            flush_task: Arc::new(tokio::sync::Mutex::new(Some(flush_task))),
            max_queue_size,
            db,
            sled,
            is_shutdown: Arc::new(AtomicBool::new(false)),
            clock,
        }
    }

    pub async fn record_click(&self, code: &str) {
        if self.queue.len() >= self.max_queue_size {
            error!("Dropped click for code {}: queue full", code);
            metrics::record_analytics_dropped();
            metrics::update_queue_length(self.queue.len() as u64);
            return;
        }
        let timestamp = self.clock.now().timestamp() as u64;
        self.queue.push(AnalyticsMessage::Click(code.to_string(), timestamp));
        metrics::record_click();
        metrics::update_queue_length(self.queue.len() as u64);
    }

    pub async fn get_analytics(&self, code: &str, start: i64, end: i64) -> Result<Vec<(u64, u64)>, AppError> {
        let key = format!("stats:{}", code);
        match self.db.zrange(&key, start, end).await {
            Ok(data) if !data.is_empty() => Ok(data),
            _ => {
                match self.sled.zrange(&key, start, end).await {
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
        sled: Arc<SledStorage<C>>,
    ) -> JoinHandle<()> {
        let batch_size = config.analytics.max_batch_size;
        let batch_time_ms = config.analytics.max_batch_size_ms;

        tokio::spawn(async move {
            let mut batch = Vec::with_capacity(batch_size);
            let mut interval = interval(Duration::from_millis(batch_time_ms));
            loop {
                interval.tick().await;
                while let Some(msg) = queue.pop() {
                    match msg {
                        AnalyticsMessage::Click(code, ts) => {
                            batch.push((code, ts));
                            if batch.len() >= batch_size {
                                Self::flush_batch(&db, &sled, &mut batch).await;
                            }
                        }
                        AnalyticsMessage::Shutdown => {
                            if !batch.is_empty() {
                                Self::flush_batch(&db, &sled, &mut batch).await;
                            }
                            return;
                        }
                    }
                }
                if !batch.is_empty() {
                    Self::flush_batch(&db, &sled, &mut batch).await;
                }
            }
        })
    }

    async fn flush_batch(db: &Arc<DatabaseClient>, sled: &Arc<SledStorage<C>>, batch: &mut Vec<(String, u64)>) {
        if batch.is_empty() {
            return;
        }
        let start = Instant::now();
        let operations: Vec<(String, u64, u64)> = batch
            .iter()
            .map(|(code, ts)| (format!("stats:{}", code), *ts, *ts))
            .collect();

        let dragonfly_result = db.zadd_batch(operations.clone(), 90 * 24 * 3600).await;
        if let Err(e) = dragonfly_result {
            error!("Failed to flush analytics to DragonflyDB: {}", e);
            metrics::record_analytics_error("flush_dragonfly");
        }

        let sled_result = sled.zadd_batch(operations, 0).await; // No expiry in Sled
        if let Err(e) = sled_result {
            error!("Failed to flush analytics to Sled: {}", e);
            metrics::record_analytics_error("flush_sled");
        }

        if dragonfly_result.is_ok() || sled_result.is_ok() {
            info!("Flushed {} analytics events in {:?}", batch.len(), start.elapsed());
            metrics::record_batch_flush(batch.len());
            batch.clear();
        } else {
            metrics::record_analytics_error("flush_failed");
        }
    }
}

impl<C: Clock + Send + Sync> Drop for AnalyticsService<C> {
    fn drop(&mut self) {
        if self.is_shutdown.load(Ordering::SeqCst) {
            return;
        }
        let queue = Arc::clone(&self.queue);
        let flush_task = Arc::clone(&self.flush_task);
        let db = Arc::clone(&self.db);
        let sled = Arc::clone(&self.sled);
        tokio::spawn(async move {
            let mut batch = Vec::with_capacity(1000);
            while let Some(msg) = queue.pop() {
                if let AnalyticsMessage::Click(code, ts) = msg {
                    batch.push((code, ts));
                    if batch.len() >= 1000 {
                        Self::flush_batch(&db, &sled, &mut batch).await;
                    }
                }
            }
            if !batch.is_empty() {
                Self::flush_batch(&db, &sled, &mut batch).await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{settings::Settings, analytics::AnalyticsConfig, cache::CacheConfig};
    use crate::services::cache::circuit_breaker::CircuitBreaker;
    use crate::clock::MockClock;
    use prometheus::core::Collector;
    use tokio::time::Duration;
    use std::sync::Arc;
    use chrono::{DateTime, Utc};

    #[tokio::test]
    async fn test_analytics_service() {
        let fixed_time = DateTime::parse_from_rfc3339("2025-05-31T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let clock = MockClock::new(fixed_time);
        let config = Arc::new(Settings {
            analytics: AnalyticsConfig {
                max_batch_size: 1000,
                max_batch_size_ms: 200,
                max_queue_size: Some(100_000),
                ..Default::default()
            },
            cache: CacheConfig {
                sled_path: "/tmp/test_sled".to_string(),
                ..Default::default()
            },
            ..Default::default()
        });
        metrics::init_metrics();
        let circuit_breaker = Arc::new(CircuitBreaker::new(vec![], 5, Duration::from_secs(60)));
        let analytics = Arc::new(AnalyticsService::new(&config, Arc::clone(&circuit_breaker), clock).await);

        // Record a click
        analytics.record_click("test").await;

        // Wait for flush
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Check stats in DB or Sled
        let stats = analytics.get_analytics("test", 0, -1).await.unwrap();
        let expected_ts = fixed_time.timestamp() as u64;
        assert_eq!(stats, vec![(expected_ts, expected_ts)]);

        // Verify metrics
        assert_eq!(metrics::CLICKS_RECORDED.get().unwrap().get(), 1);
        assert!(metrics::BATCHES_FLUSHED.get().unwrap().get() >= 1);
        let batch_size_metric = metrics::BATCH_SIZE.get().unwrap().with_label_values(&["analytics"]).collect();
        assert!(batch_size_metric[0].get_metric()[0].get_histogram().get_sample_count() >= 1);
        let db_latency_metric = metrics::DB_LATENCY.get().unwrap().with_label_values(&["zadd_batch"]).collect();
        assert!(db_latency_metric[0].get_metric()[0].get_histogram().get_sample_count() >= 1);
        assert!(metrics::QUEUE_LENGTH.get().unwrap().get() >= 0);

        // Test queue overflow
        for _ in 0..200_000 {
            analytics.record_click("test").await;
        }
        assert!(metrics::ANALYTICS_DROPPED.get().unwrap().get() > 0);

        analytics.shutdown().await;

        // Verify shutdown metrics
        assert_eq!(metrics::QUEUE_LENGTH.get().unwrap().get(), 0);
    }
}