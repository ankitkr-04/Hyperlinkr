use tokio::sync::{mpsc, Mutex, Semaphore};
use std::sync::Arc;
use std::time::{Duration, Instant};
use crate::config::settings::Settings;
use crate::services::cache::circuit_breaker::CircuitBreaker;
use crate::services::metrics;
use crate::services::storage::dragonfly::DatabaseClient;
use crate::services::storage::storage::Storage;
use tracing::{error, info};
use tokio::task::JoinHandle;

#[derive(Debug)]
enum AnalyticsMessage {
    Click(String, u64),
    Shutdown,
}

pub struct AnalyticsService {
    sender: mpsc::Sender<AnalyticsMessage>,
    semaphore: Arc<Semaphore>,
    flush_task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl AnalyticsService {
    pub async fn new(config: &Settings, circuit_breaker: Arc<CircuitBreaker>) -> Self {
        // Use direct values; no unwrap_or needed because fields are usize/u64
        let batch_size = config.analytics.max_batch_size;
        let (sender, receiver) = mpsc::channel(batch_size);
        let semaphore = Arc::new(Semaphore::new(batch_size));
        let db = Arc::new(
            DatabaseClient::new(config, Arc::clone(&circuit_breaker))
                .await
                .expect("Failed to create DatabaseClient"),
        );
        let flush_task = Self::start_flush_task(
            receiver,
            config,
            db,
            Arc::clone(&semaphore),
        )
        .await;

        Self {
            sender,
            semaphore,
            flush_task: Arc::new(Mutex::new(Some(flush_task))),
        }
    }

    pub async fn record_click(&self, code: &str, timestamp: u64) {
        let _permit = match self.semaphore.acquire().await {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to acquire semaphore: {}", e);
                return;
            }
        };
        if let Err(e) = self
            .sender
            .send(AnalyticsMessage::Click(code.to_string(), timestamp))
            .await
        {
            error!("Failed to queue analytics click: {}", e);
        }
    }

    pub async fn shutdown(&self) {
        let _ = self.sender.send(AnalyticsMessage::Shutdown).await;
        if let Some(task) = self.flush_task.lock().await.take() {
            if let Err(e) = task.await {
                error!("Flush task failed: {}", e);
            }
        }
    }

    async fn start_flush_task(
        mut receiver: mpsc::Receiver<AnalyticsMessage>,
        config: &Settings,
        db: Arc<DatabaseClient>,
        semaphore: Arc<Semaphore>,
    ) -> JoinHandle<()> {
        let batch_size = config.analytics.max_batch_size;
        let batch_time_ms = config.analytics.max_batch_size_ms;

        tokio::spawn(async move {
            let mut batch = Vec::with_capacity(batch_size);
            let mut batch_deadline = Instant::now() + Duration::from_millis(batch_time_ms);
            loop {
                tokio::select! {
                    _ = tokio::time::sleep_until(batch_deadline.into()) => {
                        if !batch.is_empty() {
                            let start = Instant::now();
                            for (code, ts) in &batch {
                                if let Err(e) = db.zadd(&format!("stats:{}", code), *ts, *ts).await {
                                    error!("Failed to flush analytics event: {}", e);
                                    metrics::DB_ERRORS.get().unwrap().with_label_values(&["zadd"]).inc();
                                }
                            }
                            info!("Flushed {} analytics events in {:?}", batch.len(), start.elapsed());
                            metrics::DB_LATENCY.get().unwrap()
                                .with_label_values(&["flush"])
                                .observe(start.elapsed().as_secs_f64());
                            batch.iter().for_each(|_| semaphore.add_permits(1));
                            batch.clear();
                            batch_deadline = Instant::now() + Duration::from_millis(batch_time_ms);
                        }
                    }
                    msg = receiver.recv() => match msg {
                        Some(AnalyticsMessage::Click(code, ts)) => {
                            batch.push((code, ts));
                            if batch.len() >= batch_size {
                                let start = Instant::now();
                                for (code, ts) in &batch {
                                    if let Err(e) = db.zadd(&format!("stats:{}", code), *ts, *ts).await {
                                        error!("Failed to flush analytics event: {}", e);
                                        metrics::DB_ERRORS.get().unwrap().with_label_values(&["zadd"]).inc();
                                    }
                                }
                                info!("Flushed {} analytics events in {:?}", batch.len(), start.elapsed());
                                metrics::DB_LATENCY.get().unwrap()
                                    .with_label_values(&["flush"])
                                    .observe(start.elapsed().as_secs_f64());
                                batch.iter().for_each(|_| semaphore.add_permits(1));
                                batch.clear();
                                batch_deadline = Instant::now() + Duration::from_millis(batch_time_ms);
                            }
                        }
                        Some(AnalyticsMessage::Shutdown) | None => {
                            if !batch.is_empty() {
                                let start = Instant::now();
                                for (code, ts) in &batch {
                                    if let Err(e) = db.zadd(&format!("stats:{}", code), *ts, *ts).await {
                                        error!("Failed to flush analytics event: {}", e);
                                        metrics::DB_ERRORS.get().unwrap().with_label_values(&["zadd"]).inc();
                                    }
                                }
                                info!("Flushed {} analytics events in {:?}", batch.len(), start.elapsed());
                                metrics::DB_LATENCY.get().unwrap()
                                    .with_label_values(&["flush"])
                                    .observe(start.elapsed().as_secs_f64());
                                batch.iter().for_each(|_| semaphore.add_permits(1));
                            }
                            break;
                        }
                    }
                }
            }
        })
    }
}

impl Drop for AnalyticsService {
    fn drop(&mut self) {
        let sender = self.sender.clone();
        let flush_task = Arc::clone(&self.flush_task);
        tokio::spawn(async move {
            let _ = sender.send(AnalyticsMessage::Shutdown).await;
            if let Some(task) = flush_task.lock().await.take() {
                if let Err(e) = task.await {
                    error!("Flush task failed on drop: {}", e);
                }
            }
        });
    }
}
