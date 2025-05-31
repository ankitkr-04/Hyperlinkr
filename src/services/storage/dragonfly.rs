use async_trait::async_trait;
use fred::{
    clients::ExclusivePool as FredPool,
    prelude::{ KeysInterface, SortedSetsInterface, TransactionInterface, Blocking::Block, Error},
    types::{config::{Config, ConnectionConfig, PerformanceConfig, ReconnectPolicy, ServerConfig, Server}, Expiration},
    
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use url::Url;
use crate::{
    config::settings::Settings,
    errors::AppError,
    services::{
        cache::circuit_breaker::CircuitBreaker,
        metrics,
    },
};
use super::storage::Storage;

pub struct DatabaseClient {
    pools: Vec<(String, FredPool)>, // (URL, Pool) pairs
    circuit_breaker: Arc<CircuitBreaker>,
}

impl DatabaseClient {
    pub async fn new(config: &Settings, circuit_breaker: Arc<CircuitBreaker>) -> Result<Self, AppError> {
        let mut pools = Vec::new();

        for url in &config.database_urls {
            let parsed_url = Url::parse(url)
                .map_err(|e| AppError::RedisConnection(format!("Invalid URL {}: {}", url, e)))?;
            let host = parsed_url
                .host_str()
                .ok_or_else(|| AppError::RedisConnection(format!("No host in URL {}", url)))?
                .to_string();
            let port = parsed_url.port().unwrap_or(6379);

            let redis_config = Config {
                server: ServerConfig::Centralized {
                    server:  Server { 
                        host: host.into(),
                        port,
                     },
                },
                blocking: Block,
                ..Default::default()
            };

            let perf_config = PerformanceConfig {
                default_command_timeout: Duration::from_secs(config.cache.redis_command_timeout_secs),
                max_feed_count: config.cache.redis_max_feed_count,
                broadcast_channel_capacity: config.cache.redis_broadcast_channel_capacity,
                ..Default::default()
            };

            let connection_config = ConnectionConfig {
                connection_timeout: Duration::from_millis(config.cache.redis_connection_timeout_ms),
                max_command_attempts: config.cache.redis_max_command_attempts,
                ..Default::default()
            };

            let policy = ReconnectPolicy::new_linear(
                config.cache.redis_reconnect_max_attempts,
                config.cache.redis_reconnect_delay_ms as u32,
                config.cache.redis_reconnect_max_delay_ms as u32,
            );

            let pool = FredPool::new(
                redis_config,
                Some(perf_config),
                Some(connection_config),
                Some(policy),
                config.cache.redis_pool_size as usize,
            )
            .map_err(|e| AppError::RedisConnection(e.to_string()))?;

            pool.connect().await;
            pool.wait_for_connect()
                .await
                .map_err(|e| AppError::RedisConnection(e.to_string()))?;

            pools.push((url.clone(), pool));
        }

        if pools.is_empty() {
            return Err(AppError::RedisConnection("No database URLs provided".into()));
        }

        Ok(Self {
            pools,
            circuit_breaker,
        })
    }

    async fn get_pool(&self) -> Result<(&str, &FredPool), AppError> {
        let node = self
            .circuit_breaker
            .get_healthy_node()
            .await
            .ok_or_else(|| AppError::RedisConnection("No healthy nodes available".into()))?;
        self
            .pools
            .iter()
            .find(|(url, _)| url == &node)
            .map(|(url, pool)| (url.as_str(), pool))
            .ok_or_else(|| AppError::RedisConnection(format!("Pool for node {} not found", node)))
    }
}

#[async_trait]
impl Storage for DatabaseClient {
    async fn get(&self, key: &str) -> Result<String, AppError> {
        let start = Instant::now();
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;
        let data: Option<String> = match (*client).get(key).await {
            Ok(data) => data,
            Err(e) => {
                self.circuit_breaker.record_failure(node).await;
                return Err(AppError::RedisConnection(e.to_string()));
            }
        };
        metrics::record_db_latency("get", start);
        data.ok_or_else(|| AppError::NotFound("Key not found".into()))
    }

    async fn set_ex(&self, key: &str, value: &str, ttl: u64) -> Result<(), AppError> {
        let start = Instant::now();
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;
        let result: Result<i64, Error> = (*client)
            .set(key, value, Some(Expiration::EX(ttl as i64)), None, false)
            .await;
        if let Err(e) = result {
            self.circuit_breaker.record_failure(node).await;
            return Err(AppError::RedisConnection(e.to_string()));
        }
        metrics::record_db_latency("set_ex", start);
        Ok(())
    }

    async fn zadd(&self, key: &str, score: u64, member: u64) -> Result<(), AppError> {
        let start = Instant::now();
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;
        let res: Result<i64, Error> = (*client)
            .zadd(key, None, None, false, false, (score as f64, member))
            .await;
        if let Err(e) = res {
            self.circuit_breaker.record_failure(node).await;
            return Err(AppError::RedisConnection(e.to_string()));
        }
        metrics::record_db_latency("zadd", start);
        Ok(())
    }

    async fn rate_limit(&self, key: &str, limit: u64, window_secs: i64) -> Result<bool, AppError> {
        let start = Instant::now();
        let now_ts = chrono::Utc::now().timestamp();
        let now_u64 = now_ts as u64;
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;
        let tx = (*client).multi();
        let _ = tx.zremrangebyscore::<i64, &str, i64, i64>(key, 0, now_ts - window_secs);
        let _ = tx.zcard::<i64, &str>(key);
        let _ = tx.zadd::<i64, &str, _>(key, None, None, false, false, (now_ts as f64, now_u64));
        let _ = tx.expire::<i64, &str>(key, window_secs as i64, Some(fred::types::ExpireOptions::LT));

        let results: Vec<i64> = match tx.exec(false).await {
            Ok(res) => res,
            Err(e) => {
                self.circuit_breaker.record_failure(node).await;
                return Err(AppError::RedisConnection(e.to_string()));
            }
        };
        let count = results.get(1).copied().unwrap_or(0);
        metrics::record_db_latency("rate_limit", start);
        Ok(count < limit as i64)
    }

    async fn zrange(&self, key: &str, start: i64, stop: i64) -> Result<Vec<u64>, AppError> {
        let start_time = Instant::now();
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;
        let result: Result<Vec<u64>, Error> = (*client).zrange(key, start, stop, None, false, None, false).await;
        match result {
            Ok(data) => {
                metrics::record_db_latency("zrange", start_time);
                Ok(data)
            }
            Err(e) => {
                self.circuit_breaker.record_failure(node).await;
                metrics::record_db_error("zrange");
                Err(AppError::RedisConnection(e.to_string()))
            }
        }
    }

    async fn zadd_batch(&self, operations: Vec<(String, u64, u64)>, expire_secs: i64) -> Result<(), AppError> {
        let start = Instant::now();
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;
        let tx = (*client).multi();
        for (key, score, member) in operations.iter() {
            let _ = tx.zadd(key, None, None, false, false, (*score as f64, *member)).await;
            let _ = tx.expire(key, expire_secs, None).await;
        }
        match tx.exec(true).await {
            Ok(_) => {
                metrics::record_db_latency("zadd_batch", start);
                Ok(())
            }
            Err(e) => {
                self.circuit_breaker.record_failure(node).await;
                metrics::record_db_error("zadd_batch");
                Err(AppError::RedisConnection(e.to_string()))
            }
        }
    }
}