use bb8::Pool;
use bb8_redis::RedisConnectionManager;
use redis::AsyncCommands;
use std::time::Duration;
use crate::services::cache::metrics;
use crate::errors::AppError;

pub struct DatabaseClient {
    pool: Pool<RedisConnectionManager>,
}

impl DatabaseClient {
    pub async fn new(redis_url: &str, pool_size: u32) -> Result<Self, AppError> {
        let manager = RedisConnectionManager::new(redis_url)
            .map_err(|_| AppError::RedisConnection)?;
        let pool = Pool::builder()
            .max_size(pool_size)
            .connection_timeout(Duration::from_secs(5))
            .idle_timeout(Some(Duration::from_secs(300)))
            .build(manager)
            .await
            .map_err(|_| AppError::RedisConnection)?;
        Ok(Self { pool })
    }

    pub async fn get(&self, key: &str) -> Result<String, AppError> {
        let start = std::time::Instant::now();
        let mut conn = self.pool
            .get()
            .await
            .map_err(|_| AppError::RedisConnection)?;
        let data: Option<Vec<u8>> = conn
            .get(key)
            .await
            .map_err(|_| {
                metrics::DB_ERRORS.with_label_values(&["get"]).inc();
                AppError::RedisOperation("Redis GET failed".to_string())
            })?;
        metrics::DB_LATENCY
            .with_label_values(&["get"])
            .observe(start.elapsed().as_secs_f64());
        data
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .ok_or(AppError::NotFound("Key not found".to_string()))
    }

    pub async fn set_ex(&self, key: &str, value: &str, ttl_seconds: u64) -> Result<(), AppError> {
        let start = std::time::Instant::now();
        let mut conn = self.pool
            .get()
            .await
            .map_err(|_| AppError::RedisConnection)?;
        conn.set_ex(key, value, ttl_seconds)
            .await
            .map_err(|_| {
                metrics::DB_ERRORS.with_label_values(&["set"]).inc();
                AppError::RedisOperation("Redis SET failed".to_string())
            })?;
        metrics::DB_LATENCY
            .with_label_values(&["set"])
            .observe(start.elapsed().as_secs_f64());
        Ok(())
    }
}