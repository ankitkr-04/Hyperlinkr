use async_trait::async_trait;
use bb8_redis::{bb8::Pool, RedisConnectionManager, redis::AsyncCommands};
use std::sync::Arc;
use std::time::{Duration, Instant};
use crate::config::settings::Settings;
use crate::errors::AppError;
use crate::services::cache::circuit_breaker::CircuitBreaker;
use crate::services::cache::metrics;
use super::storage::Storage;

pub struct DatabaseClient {
    pool: Pool<RedisConnectionManager>,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl DatabaseClient {
    pub async fn new(config: &Settings, circuit_breaker: Arc<CircuitBreaker>) -> Result<Self, AppError> {
        let manager = RedisConnectionManager::new(config.database_urls[0].clone())
            .map_err(|_| AppError::RedisConnection)?;
        let pool = Pool::builder()
            .max_size(config.cache.redis_pool_size)
            .connection_timeout(Duration::from_secs(5))
            .idle_timeout(Some(Duration::from_secs(300)))
            .build(manager)
            .await
            .map_err(|_| AppError::RedisConnection)?;
        Ok(Self { pool, circuit_breaker })
    }
}

#[async_trait]
impl Storage for DatabaseClient {
    async fn get(&self, key: &str) -> Result<String, AppError> {
        let start = Instant::now();

        let node = self
            .circuit_breaker
            .get_healthy_node()
            .await
            .ok_or_else(|| AppError::CircuitBreaker("No healthy nodes".to_string()))?;

        let mut conn = match self.pool.get().await {
            Ok(c) => c,
            Err(_) => {
                self.circuit_breaker.record_failure(&node).await;
                metrics::DB_ERRORS.get().unwrap().with_label_values(&["get"][..]).inc();
                return Err(AppError::RedisConnection);
            }
        };

        let data: Option<Vec<u8>> = match conn.get(key).await {
            Ok(d) => d,
            Err(_) => {
                self.circuit_breaker.record_failure(&node).await;
                metrics::DB_ERRORS.get().unwrap().with_label_values(&["get"][..]).inc();
                return Err(AppError::RedisOperation("Redis GET failed".into()));
            }
        };

        metrics::DB_LATENCY.get().unwrap()
            .with_label_values(&["get"][..])
            .observe(start.elapsed().as_secs_f64());

        data
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .ok_or_else(|| AppError::NotFound("Key not found".to_string()))
    }

    async fn set_ex(&self, key: &str, value: &str, ttl_seconds: u64) -> Result<(), AppError> {
        let start = Instant::now();

        let node = self
            .circuit_breaker
            .get_healthy_node()
            .await
            .ok_or_else(|| AppError::CircuitBreaker("No healthy nodes".to_string()))?;

        let mut conn = match self.pool.get().await {
            Ok(c) => c,
            Err(_) => {
                self.circuit_breaker.record_failure(&node).await;
                metrics::DB_ERRORS.get().unwrap().with_label_values(&["set"][..]).inc();
                return Err(AppError::RedisConnection);
            }
        };

        if let Err(_) = conn.set_ex(key, value, ttl_seconds).await {
            self.circuit_breaker.record_failure(&node).await;
            metrics::DB_ERRORS.get().unwrap().with_label_values(&["set"][..]).inc();
            return Err(AppError::RedisOperation("Redis SET failed".into()));
        }

        metrics::DB_LATENCY.get().unwrap()
            .with_label_values(&["set"][..])
            .observe(start.elapsed().as_secs_f64());

        Ok(())
    }
   async fn zadd(&self, key: &str, score: u64, member: u64) -> Result<(), AppError> {
        let start = Instant::now();
        let node = self.circuit_breaker.get_healthy_node().await
            .ok_or_else(|| AppError::CircuitBreaker("No healthy nodes".to_string()))?;
        let mut conn = match self.pool.get().await {
            Ok(c) => c,
            Err(_) => {
                self.circuit_breaker.record_failure(&node).await;
                metrics::DB_ERRORS.get().unwrap().with_label_values(&["zadd"][..]).inc();
                return Err(AppError::RedisConnection);
            }
        };
        if let Err(_) = conn.zadd(key, member, score).await {
            self.circuit_breaker.record_failure(&node).await;
            metrics::DB_ERRORS.get().unwrap().with_label_values(&["zadd"][..]).inc();
            return Err(AppError::RedisOperation("Redis ZADD failed".into()));
        }
        metrics::DB_LATENCY.get().unwrap()
            .with_label_values(&["zadd"][..])
            .observe(start.elapsed().as_secs_f64());
        Ok(())
    }
}