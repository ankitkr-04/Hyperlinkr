use crossbeam_queue::SegQueue;
use tokio::sync::Mutex;
use std::sync::Arc;
use crate::config::settings::AnalyticsConfig;
use tracing::error;

pub struct AnalyticsService {
    queue: Arc<SegQueue<(String, u64)>>,
    flush_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl AnalyticsService {
    pub fn new(config: AnalyticsConfig) -> Self {
        let queue = Arc::new(SegQueue::new());
        let flush_task = Self::start_flush_task(Arc::clone(&queue), config);
        Self {
            queue,
            flush_task: Mutex::new(Some(flush_task)),
        }
    }

    pub fn record_click(&self, code: String, timestamp: u64) {
        self.queue.push((code, timestamp));
    }

    fn start_flush_task(
        queue: Arc<SegQueue<(String, u64)>>,
        config: AnalyticsConfig,
    ) -> tokio::task::JoinHandle<()> {
        let db_pool = bb8::Pool::builder()
            .max_size(config.redis_pool_size)
            .build(RedisConnectionManager::new(&config.database_url).unwrap())
            .await
            .unwrap();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(config.flush_interval_ms));
            loop {
                interval.tick().await;
                let mut batch = Vec::new();
                while let Some((code, ts)) = queue.pop() {
                    batch.push((code, ts));
                    if batch.len() >= config.batch_size {
                        break;
                    }
                }
                if !batch.is_empty() {
                    if let Ok(mut db) = db_pool.get().await {
                        let mut pipe = mini_redis::client::Pipeline::new();
                        for (code, ts) in batch {
                            pipe.zadd(format!("stats:{}", code), ts, ts);
                        }
                        if let Err(e) = pipe.query_async(&mut db).await {
                            error!("Failed to flush analytics batch: {}", e);
                        }
                    }
                }
            }
        })
    }
}

impl Drop for AnalyticsService {
    fn drop(&mut self) {
        if let Some(task) = self.flush_task.blocking_lock().take() {
            task.abort();
        }
    }
}