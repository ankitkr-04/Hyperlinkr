use async_trait::async_trait;
use fred::{
    clients::ExclusivePool as FredPool,
    prelude::{Blocking::Block, Error, KeysInterface, SortedSetsInterface, TransactionInterface},
    types::{
        config::{Config, ConnectionConfig, PerformanceConfig, ReconnectPolicy, Server, ServerConfig}, scan::{ScanResult, ScanType, Scanner}, Expiration
    },
};
use futures::StreamExt; // For Stream::next()
use serde_json;
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
    types::{Paginate, UrlData, User},
};
use super::storage::Storage;

pub struct DatabaseClient {
    pools: Vec<(String, FredPool)>, // (URL, Pool) pairs
    circuit_breaker: Arc<CircuitBreaker>,
    global_admins: Vec<String>,
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
                    server: Server {
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
            global_admins: config.security.global_admins.clone(),
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

    async fn delete_url(&self, code: &str, user_id: Option<&str>, user_email: &str) -> Result<(), AppError> {
        let start = Instant::now();
        let key = format!("url:{}", code);
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;

        let data: Option<String> = match (*client).get(&key).await {
            Ok(data) => data,
            Err(e) => {
                self.circuit_breaker.record_failure(node).await;
                return Err(AppError::RedisConnection(e.to_string()));
            }
        };

        if let Some(json_str) = data {
            let url_data: UrlData = serde_json::from_str(&json_str)
                .map_err(|e| AppError::Internal(e.to_string()))?;

            let is_admin = self.global_admins.iter().any(|admin| admin == user_email);
            let is_owner = url_data.user_id.as_deref() == user_id || url_data.user_id.is_none();
            if !is_owner && !is_admin {
                return Err(AppError::Unauthorized("Not authorized to delete this URL".into()));
            }

            match (*client).del(&key).await {
                Ok(_) => (),
                Err(e) => {
                    self.circuit_breaker.record_failure(node).await;
                    return Err(AppError::RedisConnection(e.to_string()));
                }
            }
        } else {
            return Err(AppError::NotFound(format!("URL {} not found", code)));
        }

        metrics::record_db_latency("delete_url_dragonfly", start);
        Ok(())
    }

    async fn list_urls(
        &self,
        user_id: Option<&str>,
        page: u64,
        per_page: u64,
    ) -> Result<Paginate<UrlData>, AppError> {
        let start = Instant::now();
        let is_admin = user_id.is_none();
        let per_page = per_page.clamp(1, 100);
        let offset = page.saturating_sub(1) * per_page;

        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;

        let mut items = Vec::new();
        let mut total_items: u64 = 0;

        let pattern = "url:*".to_string();
        let scan_count = Some(1000u32);
        let mut scanner = (*client).scan(pattern.clone(), scan_count, Some(ScanType::String));

        while let Some(page_result) = scanner.next().await {
            let mut scan_page: ScanResult = page_result.map_err(|e| {
                futures::executor::block_on(self.circuit_breaker.record_failure(node));
                AppError::RedisConnection(e.to_string())
            })?;

            let keys = scan_page.take_results().unwrap_or_default();

            for key in keys {
                let json_str: String = match (*client).get(&key).await {
                    Ok(data) => data,
                    Err(e) => {
                        self.circuit_breaker.record_failure(node).await;
                        return Err(AppError::RedisConnection(e.to_string()));
                    }
                };

                let url_data: UrlData = serde_json::from_str(&json_str)
                    .map_err(|e| AppError::Internal(e.to_string()))?;

                let is_visible = is_admin
                    || url_data.user_id.as_deref() == user_id
                    || url_data.user_id.is_none();

                if is_visible {
                    total_items += 1;
                    if total_items > offset && items.len() < per_page as usize {
                        items.push(url_data);
                    }
                }
            }

            if !scan_page.has_more() {
                break;
            }

            if items.len() >= per_page as usize && total_items >= offset + per_page {
                scan_page.cancel();
                break;
            }

            scan_page.next();
        }

        let total_pages = if total_items == 0 {
            1
        } else {
            (total_items + per_page - 1) / per_page
        };

        metrics::record_db_latency("list_urls_dragonfly", start);
        Ok(Paginate {
            items,
            page,
            per_page,
            total_items,
            total_pages,
        })
    }

    async fn set_user(&self, user: &User) -> Result<(), AppError> {
        let start = Instant::now();
        let key = format!("user:{}", user.id);
        let email_key = format!("user_email:{}", user.email);
        let data = serde_json::to_string(user)
            .map_err(|e| AppError::Internal(e.to_string()))?;

        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;

        let tx = (*client).multi();
        let _ = tx.set(&key, &data, None, None, false).await;
        let _ = tx.set(&email_key, &user.id, None, None, false).await;

        match tx.exec(true).await {
            Ok(_) => (),
            Err(e) => {
                self.circuit_breaker.record_failure(node).await;
                return Err(AppError::RedisConnection(e.to_string()));
            }
        }

        metrics::record_db_latency("set_user_dragonfly", start);
        Ok(())
    }

    async fn get_user(&self, id_or_email: &str) -> Result<Option<User>, AppError> {
        let start = Instant::now();
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;

        let key = if id_or_email.contains('@') {
            let email_key = format!("user_email:{}", id_or_email);
            match (*client).get::<Option<String>, _>(&email_key).await {
                Ok(Some(id)) => format!("user:{}", id),
                Ok(None) => return Ok(None),
                Err(e) => {
                    self.circuit_breaker.record_failure(node).await;
                    return Err(AppError::RedisConnection(e.to_string()));
                }
            }
        } else {
            format!("user:{}", id_or_email)
        };

        let data: Option<String> = match (*client).get(&key).await {
            Ok(data) => data,
            Err(e) => {
                self.circuit_breaker.record_failure(node).await;
                return Err(AppError::RedisConnection(e.to_string()));
            }
        };

        let user = if let Some(json_str) = data {
            Some(
                serde_json::from_str(&json_str)
                    .map_err(|e| AppError::Internal(e.to_string()))?,
            )
        } else {
            None
        };

        metrics::record_db_latency("get_user_dragonfly", start);
        Ok(user)
    }

    async fn count_users(&self) -> Result<u64, AppError> {
        let start = Instant::now();
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;

        let mut count: u64 = 0;
        let pattern = "user:*".to_string();
        let scan_count = Some(1000u32);
        let mut scanner = (*client).scan(pattern.clone(), scan_count, Some(ScanType::String));

        while let Some(page_result) = scanner.next().await {
            let mut scan_page: ScanResult = page_result.map_err(|e| {
                futures::executor::block_on(self.circuit_breaker.record_failure(node));
                AppError::RedisConnection(e.to_string())
            })?;

            let keys = scan_page.take_results().unwrap_or_default();
            count += keys.len() as u64;

            if !scan_page.has_more() {
                break;
            }

            scan_page.next();
        }

        metrics::record_db_latency("count_users_dragonfly", start);
        Ok(count)
    }

    async fn count_urls(&self, user_id: Option<&str>) -> Result<u64, AppError> {
        let start = Instant::now();
        let is_admin = user_id.is_none();
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;

        let mut count: u64 = 0;
        let pattern = "url:*".to_string();
        let scan_count = Some(1000u32);
        let mut scanner = (*client).scan(pattern.clone(), scan_count, Some(ScanType::String));

        while let Some(page_result) = scanner.next().await {
            let mut scan_page: ScanResult = page_result.map_err(|e| {
                futures::executor::block_on(self.circuit_breaker.record_failure(node));
                AppError::RedisConnection(e.to_string())
            })?;

            let keys = scan_page.take_results().unwrap_or_default();

            for key in keys {
                let json_str: String = match (*client).get(&key).await {
                    Ok(data) => data,
                    Err(e) => {
                        self.circuit_breaker.record_failure(node).await;
                        return Err(AppError::RedisConnection(e.to_string()));
                    }
                };

                let url_data: UrlData = serde_json::from_str(&json_str)
                    .map_err(|e| AppError::Internal(e.to_string()))?;

                if is_admin || url_data.user_id.as_deref() == user_id || url_data.user_id.is_none() {
                    count += 1;
                }
            }

            if !scan_page.has_more() {
                break;
            }

            scan_page.next();
        }

        metrics::record_db_latency("count_urls_dragonfly", start);
        Ok(count)
    }

    async fn blacklist_token(&self, token: &str, expiry_secs: u64) -> Result<(), AppError> {
        let start = Instant::now();
        let key = format!("token:{}", token);
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;

        match (*client)
            .set(&key, "1", Some(Expiration::EX(expiry_secs as i64)), None, false)
            .await
        {
            Ok(_) => (),
            Err(e) => {
                self.circuit_breaker.record_failure(node).await;
                return Err(AppError::RedisConnection(e.to_string()));
            }
        }

        metrics::record_db_latency("blacklist_token_dragonfly", start);
        Ok(())
    }

    async fn is_token_blacklisted(&self, token: &str) -> Result<bool, AppError> {
        let start = Instant::now();
        let key = format!("token:{}", token);
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;

        let exists: bool = match (*client).exists(&key).await {
            Ok(result) => result,
            Err(e) => {
                self.circuit_breaker.record_failure(node).await;
                return Err(AppError::RedisConnection(e.to_string()));
            }
        };

        metrics::record_db_latency("is_token_blacklisted_dragonfly", start);
        Ok(exists)
    }
}