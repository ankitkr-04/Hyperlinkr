use tokio::sync::{mpsc, Mutex};
use std::sync::Arc;
use crate::config::settings::{AnalyticsConfig, Settings};
use tracing::error;
use bb8_redis::{bb8::Pool, RedisConnectionManager, redis::AsyncCommands};
use tokio::task::JoinHandle;

pub struct AnalyticsService {
    sender: mpsc::Sender<(String, u64)>,
    flush_task: Mutex<Option<JoinHandle<()>>>,
}

/// Service for handling analytics events, such as recording link clicks and batching them for storage.
///
/// # Methods
///
/// - `new(config: &Settings) -> Self`  
///   Asynchronously creates a new `AnalyticsService` instance, initializing the batching channel and
///   starting the background flush task that periodically writes analytics data to the database.
///
/// - `record_click(&self, code: String, timestamp: u64)`  
///   Asynchronously records a click event by sending it to the batching channel. If the channel is full or closed,
///   logs an error.
///
/// - `start_flush_task(receiver: mpsc::Receiver<(String, u64)>, config: &Settings) -> JoinHandle<()>`  
///   Internal method that spawns a background task. This task collects click events from the channel,
///   batches them, and periodically flushes them to a Redis-compatible database using a connection pool.
///   Each batch is written as a series of sorted set (`zadd`) operations, keyed by the link code.
///
/// # Fields
///
/// - `sender`: Channel sender for queuing analytics events.
/// - `flush_task`: Mutex-protected handle to the background flush task.
///
/// # Errors
///
/// Errors during sending to the channel or flushing to the database are logged but do not panic the service.
///
/// # Dependencies
///
/// - `tokio` for async runtime and task spawning.
/// - `redis` for database operations.
/// - `bb8` for connection pooling.
/// - `mpsc` for batching channel.
/// - `Settings` for configuration.
/// Implementation of the `AnalyticsService` responsible for recording and batching analytics events,
/// such as link clicks, and periodically flushing them to a Redis-compatible backend (e.g., Dragonfly).
///
/// # Methods
///
/// - `new(config: &Settings) -> Self`  
///   Asynchronously creates a new `AnalyticsService` instance, initializing the batching channel and
///   spawning a background task to flush analytics data at regular intervals.
///
/// - `record_click(&self, code: String, timestamp: u64)`  
///   Asynchronously queues a click event, identified by a code and timestamp, for later batch processing.
///   Logs an error if the event cannot be queued.
///
/// - `start_flush_task(receiver: mpsc::Receiver<(String, u64)>, config: &Settings) -> JoinHandle<()>`  
///   Internal async function that spawns a background task. This task collects queued analytics events
///   into batches and periodically writes them to the Redis backend using pipelined commands. Handles
///   connection pooling and error logging.
///
/// # Details
///
/// - Uses Tokio's async channels and tasks for concurrency.
/// - Batches events according to the configured batch size and flush interval.
/// - Stores analytics data in Redis sorted sets, using the event timestamp as both the score and value.
/// - Handles connection pooling via `bb8` and `RedisConnectionManager`.
/// - Errors during queuing or flushing are logged but do not panic the service.
impl AnalyticsService {
    pub async fn new(config: &Settings) -> Self {
        let (sender, receiver) = mpsc::channel(config.analytics.batch_size);
        let flush_task = Self::start_flush_task(receiver, config).await;
        Self {
            sender,
            flush_task: Mutex::new(Some(flush_task)),
        }
    }

    pub async fn record_click(&self, code: String, timestamp: u64) {
        if let Err(e) = self.sender.send((code, timestamp)).await {
            error!("Failed to queue analytics click: {}", e);
        }
    }


    async fn start_flush_task(
        mut receiver: mpsc::Receiver<(String, u64)>,
        config: &Settings,
    ) -> JoinHandle<()> {
        let manager = RedisConnectionManager::new(config.database_url.clone())
            .expect("Failed to create Dragonfly connection manager");
        let pool = Pool::builder()
            .max_size(config.cache.redis_pool_size)
            .build(manager)
            .await
            .expect("Failed to create Dragonfly pool");

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(
                config.analytics.flush_interval_ms,
            ));
            loop {
                interval.tick().await;
                let mut batch = Vec::with_capacity(config.analytics.batch_size);
                while let Some(item) = receiver.recv().await {
                    batch.push(item);
                    if batch.len() >= config.analytics.batch_size {
                        break;
                    }
                }
                if !batch.is_empty() {
                    if let Ok(mut conn) = pool.get().await {
                        let mut pipe = redis::pipe();
                        for (code, ts) in &batch {
                            pipe.zadd(format!("stats:{}", code), ts, ts);
                        }
                        if let Err(e) = pipe.query_async(&mut conn).await {
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