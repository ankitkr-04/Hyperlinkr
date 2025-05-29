use async_trait::async_trait;
use bb8_redis::{bb8::Pool, RedisConnectionManager, redis::{AsyncCommands, pipe}};
use std::{sync::Arc, time::{Duration, Instant}};
use crate::{config::settings::Settings, errors::AppError};
use crate::services::cache::circuit_breaker::CircuitBreaker;
use crate::services::metrics;
use super::storage::Storage;

pub struct DatabaseClient {
    pools: Vec<Pool<RedisConnectionManager>>,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl DatabaseClient {
    pub async fn new(
        config: &Settings,
        circuit_breaker: Arc<CircuitBreaker>,
    ) -> Result<Self, AppError> {
        let mut pools = Vec::with_capacity(config.database_urls.len());
        for url in &config.database_urls {
            let mgr = RedisConnectionManager::new(url.clone())
                .map_err(|_| AppError::RedisConnection)?;
            let pool = Pool::builder()
                .max_size(config.cache.redis_pool_size)
                .connection_timeout(Duration::from_secs(5))
                .idle_timeout(Some(Duration::from_secs(300)))
                .build(mgr)
                .await
                .map_err(|_| AppError::RedisConnection)?;
            pools.push(pool);
        }
        if pools.is_empty() {
            return Err(AppError::RedisConnection);
        }
        Ok(Self { pools, circuit_breaker })
    }

    /// Attempt `f(pool, node)` up to one try per pool, recording failures.
    async fn try_with_node<F, T>(&self, mut f: F) -> Result<T, AppError>
    where
        F: FnMut(Pool<RedisConnectionManager>, String) -> futures::future::BoxFuture<'_, Result<T, ()>>,
    {
        let max = self.pools.len();
        let mut attempts = 0;
        let start = Instant::now();

        while attempts < max {
            // pick a healthy node
            let node = self.circuit_breaker.get_healthy_node()
                .await
                .ok_or_else(|| AppError::CircuitBreaker("No healthy nodes".into()))?;
            let idx = self.circuit_breaker.get_node_index(&node).unwrap_or(0);
            if idx >= self.pools.len() {
                self.circuit_breaker.record_failure(&node).await;
                attempts += 1;
                continue;
            }

            // run the operation
            match f(self.pools[idx].clone(), node.clone()).await {
                Ok(result) => {
                    // record overall latency once
                    metrics::record_db_latency("op", start);
                    return Ok(result);
                }
                Err(_) => {
                    self.circuit_breaker.record_failure(&node).await;
                    metrics::record_db_error("op");
                    attempts += 1;
                }
            }
        }
        Err(AppError::RedisConnection)
    }
}

#[async_trait]
impl Storage for DatabaseClient {
    async fn get(&self, key: &str) -> Result<String, AppError> {
        self.try_with_node(|pool, _node| {
            let key = key.to_string();
            Box::pin(async move {
                let mut conn = pool.get().await.map_err(|_| ())?;
                let data: Option<Vec<u8>> = conn.get(&key).await.map_err(|_| ())?;
                data
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .ok_or(())
            })
        })
        .await
        .map_err(|_| AppError::NotFound("Key not found".into()))
    }

    async fn set_ex(&self, key: &str, value: &str, ttl: u64) -> Result<(), AppError> {
        self.try_with_node(|pool, _node| {
            let key = key.to_string();
            let value = value.to_string();
            Box::pin(async move {
                let mut conn = pool.get().await.map_err(|_| ())?;
                conn.set_ex(&key, &value, ttl).await.map_err(|_| ())?;
                Ok(())
            })
        })
        .await
    }

    async fn zadd(&self, key: &str, score: u64, member: u64) -> Result<(), AppError> {
        self.try_with_node(|pool, _node| {
            let key = key.to_string();
            Box::pin(async move {
                let mut conn = pool.get().await.map_err(|_| ())?;
                conn.zadd(&key, member, score).await.map_err(|_| ())?;
                Ok(())
            })
        })
        .await
    }

    async fn rate_limit(&self, key: &str, limit: u64, window_secs: i64) -> Result<bool, AppError> {
        let key = key.to_string();
        self.try_with_node(|pool, _node| {
            let key = key.clone();
            Box::pin(async move {
                let mut conn = pool.get().await.map_err(|_| ())?;
                let now = chrono::Utc::now().timestamp();
                let mut pipeline = pipe();
                pipeline
                    .atomic()
                    .cmd("ZREMRANGEBYSCORE").arg(&key).arg(0).arg(now - window_secs).ignore()
                    .cmd("ZCARD").arg(&key)
                    .cmd("ZADD").arg(&key).arg(now).arg(now).ignore()
                    .cmd("EXPIRE").arg(&key).arg(window_secs).ignore();
                let (count,): (i64,) = pipeline.query_async(&mut conn).await.map_err(|_| ())?;
                Ok(count < limit as i64)
            })
        })
        .await
    }
}
