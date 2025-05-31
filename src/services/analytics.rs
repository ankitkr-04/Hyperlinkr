use crossbeam_queue::SegQueue;
use tokio::time::{interval, Duration};
use std::sync::Arc;
use std::time::Instant;
use crate::config::settings::Settings;
use crate::services::cache::circuit_breaker::CircuitBreaker;
use crate::services::metrics;
use crate::services::storage::dragonfly::DatabaseClient;
use tracing::{error, info};
use tokio::task::JoinHandle;

use super::storage::storage::Storage;

#[derive(Debug)]
enum AnalyticsMessage {
    Click(String, u64),
    Shutdown,
}

pub struct AnalyticsService {
    queue: Arc<SegQueue<AnalyticsMessage>>,
    flush_task: Arc<tokio::sync::Mutex<Option<JoinHandle<()>>>>,
    max_queue_size: usize,
    db: Arc<DatabaseClient>,
    is_shutdown: Arc<std::sync::atomic::AtomicBool>,
}

impl AnalyticsService {
    pub async fn new(config: &Settings, circuit_breaker: Arc<CircuitBreaker>) -> Self {
        let queue = Arc::new(SegQueue::new());
        let max_queue_size = config.analytics.max_queue_size.unwrap_or(100_000); // Default: 100K
        let db = Arc::new(DatabaseClient::new(config, Arc::clone(&circuit_breaker)).await.unwrap());
        let flush_task = Self::start_flush_task(Arc::clone(&queue), config, Arc::clone(&db)).await;

        Self {
            queue,
            flush_task: Arc::new(tokio::sync::Mutex::new(Some(flush_task))),
            max_queue_size,
            db,
            is_shutdown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub async fn record_click(&self, code: &str, timestamp: u64) {
        if self.queue.len() >= self.max_queue_size {
            error!("Dropped click for code {}: queue full", code);
            metrics::record_analytics_dropped();
            metrics::update_queue_length(self.queue.len() as u64);
            return;
        }
        self.queue.push(AnalyticsMessage::Click(code.to_string(), timestamp));
        metrics::record_click();
        metrics::update_queue_length(self.queue.len() as u64);
    }

    pub async fn shutdown(&self) {
        if self.is_shutdown.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return; // Already shut down
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
                                Self::flush_batch(&db, &mut batch).await;
                            }
                        }
                        AnalyticsMessage::Shutdown => {
                            if !batch.is_empty() {
                                Self::flush_batch(&db, &mut batch).await;
                            }
                            return;
                        }
                    }
                }
                if !batch.is_empty() {
                    Self::flush_batch(&db, &mut batch).await;
                }
            }
        })
    }

    async fn flush_batch(db: &Arc<DatabaseClient>, batch: &mut Vec<(String, u64)>) {
        if batch.is_empty() {
            return;
        }
        let start = Instant::now();
        let operations: Vec<(String, u64, u64)> = batch
            .iter()
            .map(|(code, ts)| (format!("stats:{}", code), *ts, *ts))
            .collect();
        match db.zadd_batch(operations, 90 * 24 * 3600).await {
            Ok(_) => {
                info!("Flushed {} analytics events in {:?}", batch.len(), start.elapsed());
                // DB_LATENCY recorded in zadd_batch
                metrics::record_batch_flush(batch.len());
                batch.clear();
            }
            Err(e) => {
                error!("Failed to flush analytics batch: {}", e);
                // DB_ERRORS recorded in zadd_batch
                metrics::record_analytics_error("flush");
                // Retain batch for next flush attempt
            }
        }
    }
}

impl Drop for AnalyticsService {
    fn drop(&mut self) {
        if self.is_shutdown.load(std::sync::atomic::Ordering::SeqCst) {
            return; // Skip if already shut down
        }
        let queue = Arc::clone(&self.queue);
        let flush_task = Arc::clone(&self.flush_task);
        let db = Arc::clone(&self.db);
        tokio::spawn(async move {
            let mut batch = Vec::with_capacity(1000);
            while let Some(msg) = queue.pop() {
                if let AnalyticsMessage::Click(code, ts) = msg {
                    batch.push((code, ts));
                    if batch.len() >= 1000 {
                        Self::flush_batch(&db, &mut batch).await;
                    }
                }
            }
            if !batch.is_empty() {
                Self::flush_batch(&db, &mut batch).await;
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
    use crate::config::{settings::Settings,analytics::AnalyticsConfig};
    use crate::services::cache::circuit_breaker::CircuitBreaker;
    use crate::services::storage::dragonfly::DatabaseClient;
    use std::sync::Arc;
    use prometheus::core::Collector;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_analytics_service() {
        let config = Arc::new(Settings {
            analytics: AnalyticsConfig {
                max_batch_size: 1000,
                max_batch_size_ms: 200,
                max_queue_size: Some(100_000),
                ..Default::default()
            },
            ..Default::default()
        });
        metrics::init_metrics();
        let circuit_breaker = Arc::new(CircuitBreaker::new(vec![], 5, Duration::from_secs(60)));
        let db = Arc::new(DatabaseClient::new(&config, Arc::clone(&circuit_breaker)).await.unwrap());
        let analytics = Arc::new(AnalyticsService::new(&config, Arc::clone(&circuit_breaker)).await);

        // Record a click
        analytics.record_click("test", 1234567890).await;

        // Wait for flush
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Check stats in DB
        let stats: Vec<(u64, u64)> = db.zrange(&"stats:test", 0, -1).await.unwrap();
        assert_eq!(stats, vec![(1234567890, 1234567890)]);

        // Verify metrics
        assert_eq!(metrics::CLICKS_RECORDED.get().unwrap().get(), 1);
        assert!(metrics::BATCHES_FLUSHED.get().unwrap().get() >= 1);
        let batch_size_metric = metrics::BATCH_SIZE.get().unwrap().with_label_values(&["analytics"]).collect();
        let metric_family = &batch_size_metric[0];
        let histogram = metric_family.get_metric().get(0).unwrap().get_histogram();
        assert!(histogram.get_sample_count() >= 1);
        let db_latency_metric = metrics::DB_LATENCY.get().unwrap().with_label_values(&["zadd_batch"]).collect();
        let db_latency_family = &db_latency_metric[0];
        let db_latency_histogram = db_latency_family.get_metric().get(0).unwrap().get_histogram();
        assert!(db_latency_histogram.get_sample_count() >= 1);
        assert!(metrics::QUEUE_LENGTH.get().unwrap().get() >= 0);

        // Test queue overflow
        for _ in 0..200_000 {
            analytics.record_click("test", 1234567891).await;
        }
        assert!(metrics::ANALYTICS_DROPPED.get().unwrap().get() > 0);

        analytics.shutdown().await;

        // Verify shutdown metrics
        assert!(metrics::QUEUE_LENGTH.get().unwrap().get() == 0);
    }
}