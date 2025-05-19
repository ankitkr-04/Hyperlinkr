use crossbeam_queue::SegQueue;
use tokio::sync::Mutex;
use std::sync::Arc;

pub struct AnalyticsService {
    queue: Arc<SegQueue<(String, u64)>>,
    flush_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl AnalyticsService {
    pub fn new(db_pool: bb8::Pool<mini_redis::client::ClientManager>) -> Self {
        let queue = Arc::new(SegQueue::new());
        let flush_task = Self::start_flush_task(Arc::clone(&queue), db_pool);
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
        db_pool: bb8::Pool<mini_redis::client::ClientManager>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(200));
            loop {
                interval.tick().await;
                let mut batch = Vec::new();
                while let Some((code, ts)) = queue.pop() {
                    batch.push((code, ts));
                    if batch.len() >= 10_000 {
                        break;
                    }
                }
                if !batch.is_empty() {
                    if let Ok(mut db) = db_pool.get().await {
                        let mut pipe = mini_redis::client::Pipeline::new();
                        for (code, ts) in batch {
                            pipe.zadd(format!("stats:{}", code), ts, ts);
                        }
                        let _ = pipe.query_async(&mut db).await;
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