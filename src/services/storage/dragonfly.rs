use async_trait::async_trait;
use fred::{
    clients::ExclusivePool as FredPool,
    prelude::{Blocking::Block, KeysInterface, LuaInterface, SetsInterface, SortedSetsInterface, TransactionInterface},
    types::{
        config::{Config, ConnectionConfig, PerformanceConfig, ReconnectPolicy, Server, ServerConfig},
        scan::{ScanResult, ScanType, Scanner}, Expiration
    },
};
use futures::StreamExt;
use serde_json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use url::Url;
use xxhash_rust::xxh3::xxh3_64;
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
                    server: Server { host: host.into(), port },

                },

                blocking: Block,
                ..Default::default()
            };

            let perf_config = PerformanceConfig {
                default_command_timeout: Duration::from_millis(50), // Tighten for sub-ms
                max_feed_count: config.cache.redis_max_feed_count,
                broadcast_channel_capacity: config.cache.redis_broadcast_channel_capacity,

                ..Default::default()
            };

            let connection_config = ConnectionConfig {
                connection_timeout: Duration::from_millis(10),
                max_command_attempts: 2,
                
                
                ..Default::default()
            };

            let policy = ReconnectPolicy::new_linear(3, 10, 50);

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

    fn get_pool_for_key(&self, key: &str) -> Result<(&str, &FredPool), AppError> {
        if self.pools.is_empty() {
            return Err(AppError::RedisConnection("No pools available".into()));
        }
        let hash = xxh3_64(key.as_bytes());
        let index = (hash % self.pools.len() as u64) as usize;
        let (url, pool) = &self.pools[index];
        Ok((url.as_str(), pool))
    }

    async fn get_pool(&self) -> Result<(&str, &FredPool), AppError> {
        let node = self
            .circuit_breaker
            .get_healthy_node()
            .await
            .ok_or_else(|| AppError::RedisConnection("No healthy nodes available".into()))?;
        self.pools
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
        let (node, pool) = self.get_pool_for_key(key)?;
        let client = pool.acquire().await;
        let data: Option<String> = (*client).get(key).await.map_err(|e| {
            futures::executor::block_on(self.circuit_breaker.record_failure(node));
            AppError::RedisConnection(e.to_string())
        })?;
        metrics::record_db_latency("get_dragonfly", start);
        data.ok_or_else(|| AppError::NotFound("Key not found".into()))
    }

    async fn set_ex(&self, key: &str, value: &str, ttl: u64) -> Result<(), AppError> {
        let start = Instant::now();
        let (node, pool) = self.get_pool_for_key(key)?;
        let client = pool.acquire().await;
        let _: () = (*client)
            .set(key, value, Some(Expiration::EX(ttl as i64)), None, false)
            .await
            .map_err(|e| {
                futures::executor::block_on(self.circuit_breaker.record_failure(node));
                AppError::RedisConnection(e.to_string())
            })?;
        metrics::record_db_latency("set_ex_dragonfly", start);
        Ok(())
    }

    async fn zadd(&self, key: &str, score: u64, member: u64) -> Result<(), AppError> {
        let start = Instant::now();
        let (node, pool) = self.get_pool_for_key(key)?;
        let client = pool.acquire().await;
        let _: () = (*client)
            .zadd(key, None, None, false, false, (score as f64, member))
            .await
            .map_err(|e| {
                futures::executor::block_on(self.circuit_breaker.record_failure(node));
                AppError::RedisConnection(e.to_string())
            })?;
        metrics::record_db_latency("zadd_dragonfly", start);
        Ok(())
    }

    async fn rate_limit(&self, key: &str, limit: u64, window_secs: i64) -> Result<bool, AppError> {
        let start = Instant::now();
        let (node, pool) = self.get_pool_for_key(key)?;
        let client = pool.acquire().await;
        let now_ts = chrono::Utc::now().timestamp();
        let now_u64 = now_ts as u64;
        let tx = (*client).multi();
        let _ = tx.zremrangebyscore::<i64, &str, i64, i64>(key, 0, now_ts - window_secs).await;
        let _ = tx.zcard::<i64, &str>(key).await;
        let _ = tx.zadd::<i64, &str, _>(key, None, None, false, false, (now_ts as f64, now_u64)).await;
        let _ = tx.expire::<i64, &str>(key, window_secs as i64, Some(fred::types::ExpireOptions::LT)).await;

        let results: Vec<i64> = tx.exec(false).await.map_err(|e| {
            futures::executor::block_on(self.circuit_breaker.record_failure(node));
            AppError::RedisConnection(e.to_string())
        })?;
        let count = results.get(1).copied().unwrap_or(0);
        metrics::record_db_latency("rate_limit_dragonfly", start);
        Ok(count < limit as i64)
    }

    async fn zrange(&self, key: &str, start: i64, stop: i64) -> Result<Vec<(u64, u64)>, AppError> {
        let start_time = Instant::now();
        let (node, pool) = self.get_pool_for_key(key)?;
        let client = pool.acquire().await;
        let result: Vec<(u64, u64)> = (*client)
            .zrange(key, start, stop, None, false, None, true)
            .await
            .map_err(|e| {
                futures::executor::block_on(self.circuit_breaker.record_failure(node));
                AppError::RedisConnection(e.to_string())
            })?;
        metrics::record_db_latency("zrange_dragonfly", start_time);
        Ok(result)
    }

    async fn zadd_batch(&self, operations: Vec<(String, u64, u64)>, expire_secs: i64) -> Result<(), AppError> {
        let start = Instant::now();
        let mut grouped = std::collections::HashMap::new();
        for (key, score, member) in operations {
            grouped
                .entry(key)
                .or_insert_with(Vec::new)
                .push((score, member));
        }

        for (key, ops) in grouped {
            let (node, pool) = self.get_pool_for_key(&key)?;
            let client = pool.acquire().await;
            let tx = (*client).multi();
            for (score, member) in ops {
                let _ = tx.zadd::<(), _, _>(&key, None, None, false, false, (score as f64, member)).await;
            }
            let _ = tx.expire::<(), _>(&key, expire_secs, None).await;
            let _: () = tx.exec(true).await.map_err(|e| {
                 futures::executor::block_on(self.circuit_breaker.record_failure(node));
                AppError::RedisConnection(e.to_string())
            })?;
        }
        metrics::record_db_latency("zadd_batch_dragonfly", start);
        Ok(())
    }

    async fn delete_url(&self, code: &str, user_id: Option<&str>, user_email: &str) -> Result<(), AppError> {
        let start = Instant::now();
        let key = format!("url:{}", code);
        let index_key = user_id.map(|uid| format!("user_urls:{}", uid));
        let (node, pool) = self.get_pool_for_key(&key)?;
        let client = pool.acquire().await;

        let data: Option<String> = (*client).get(&key).await.map_err(|e| {
             futures::executor::block_on(self.circuit_breaker.record_failure(node));
            AppError::RedisConnection(e.to_string())
        })?;

        if let Some(json_str) = data {
            let url_data: UrlData = serde_json::from_str(&json_str)
                .map_err(|e| AppError::Internal(e.to_string()))?;

            let is_admin = self.global_admins.iter().any(|admin| admin == user_email);
            let is_owner = url_data.user_id.as_deref() == user_id || url_data.user_id.is_none();
            if !is_owner && !is_admin {
                return Err(AppError::Unauthorized("Not authorized to delete this URL".into()));
            }

            let tx = (*client).multi();
            let _ = tx.del::<(), _>(&key).await;
            if let Some(ref ikey) = index_key {
                let _ = tx.srem::<(), _, _>(ikey, code).await;
            }
            let _: () = tx.exec(true).await.map_err(|e| {
                 futures::executor::block_on(self.circuit_breaker.record_failure(node));
                AppError::RedisConnection(e.to_string())
            })?;
        } else {
            return Err(AppError::NotFound(format!("URL {} not found", code)));
        }

        metrics::record_db_latency("delete_url_dragonfly", start);
        Ok(())
    }

    async fn set_url(&self, code: &str, url_data: &UrlData) -> Result<(), AppError> {
        let start = Instant::now();
        let key = format!("url:{}", code);
        let data = serde_json::to_string(url_data)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let index_key = url_data.user_id.as_deref().map(|uid| format!("user_urls:{}", uid));

        let (node, pool) = self.get_pool_for_key(&key)?;
        let client = pool.acquire().await;
        let tx = (*client).multi();
        let _ = tx.set::<(), _, _>(&key, &data, None, None, false).await;
        if let Some(ref ikey) = index_key {
            let _ = tx.sadd::<(), _, _>(ikey, code).await;
        }
        let _: () = tx.exec(true).await.map_err(|e| {
             futures::executor::block_on(self.circuit_breaker.record_failure(node));
            AppError::RedisConnection(e.to_string())
        })?;

        metrics::record_db_latency("set_url_dragonfly", start);
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

        if is_admin {
            let pattern = "url:*".to_string();
            let scan_count = Some(1000u32);
            let mut scanner = (*client).scan(pattern, scan_count, Some(ScanType::String));
            let pipeline = (*client).pipeline();

            while let Some(page_result) = scanner.next().await {
                let scan_page: ScanResult = page_result.map_err(|e| {
                    futures::executor::block_on(self.circuit_breaker.record_failure(node));
                    AppError::RedisConnection(e.to_string())
                })?;
                let keys = scan_page.results().as_ref().map(|v| v.clone()).unwrap_or_default();

                for key in keys {
                    let _ = pipeline.get::<String, _>(&key).await;
                }

                let results: Vec<Option<String>> = pipeline.all().await.map_err(|e| {
                     futures::executor::block_on(self.circuit_breaker.record_failure(node));
                    AppError::RedisConnection(e.to_string())
                })?;

                for json_str in results.into_iter().flatten() {
                    let url_data: UrlData = serde_json::from_str(&json_str)
                        .map_err(|e| AppError::Internal(e.to_string()))?;
                    total_items += 1;
                    if total_items > offset && items.len() < per_page as usize {
                        items.push(url_data);
                    }
                }

                if !scan_page.has_more() {
                    break;
                }
                if items.len() >= per_page as usize && total_items >= offset + per_page {
                    break;
                }
            }
        } else if let Some(uid) = user_id {
            let index_key = format!("user_urls:{}", uid);
            let codes: Vec<String> = (*client)
                .smembers(&index_key)
                .await
                .map_err(|e| {
                     futures::executor::block_on(self.circuit_breaker.record_failure(node));
                    AppError::RedisConnection(e.to_string())
                })?;
            total_items = codes.len() as u64;

            let start_idx = offset.min(total_items) as usize;
            let end_idx = (offset + per_page).min(total_items) as usize;
            let pipeline = (*client).pipeline();

            for code in codes.iter().skip(start_idx).take(end_idx - start_idx) {
                let key = format!("url:{}", code);
                let _ = pipeline.get::<String, _>(&key).await;
            }

            let results: Vec<Option<String>> = pipeline.all().await.map_err(|e| {
                 futures::executor::block_on(self.circuit_breaker.record_failure(node));
                AppError::RedisConnection(e.to_string())
            })?;

            for json_str in results.into_iter().flatten() {
                let url_data: UrlData = serde_json::from_str(&json_str)
                    .map_err(|e| AppError::Internal(e.to_string()))?;
                items.push(url_data);
            }
        }

        let total_pages = if total_items == 0 { 1 } else { (total_items + per_page - 1) / per_page };
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

        let (node, pool) = self.get_pool_for_key(&key)?;
        let client = pool.acquire().await;
        let tx = (*client).multi();
        let _ = tx.set::<(), _, _>(&key, &data, None, None, false).await;
        let _ = tx.set::<(), _, _>(&email_key, &user.id, None, None, false).await;
        let _: () = tx.exec(true).await.map_err(|e| {
             futures::executor::block_on(self.circuit_breaker.record_failure(node));
            AppError::RedisConnection(e.to_string())
        })?;

        metrics::record_db_latency("set_user_dragonfly", start);
        Ok(())
    }

    async fn get_user(&self, id_or_email: &str) -> Result<Option<User>, AppError> {
        let start = Instant::now();
        let (node, pool) = self.get_pool_for_key(id_or_email)?;
        let client = pool.acquire().await;

        let key = if id_or_email.contains('@') {
            let email_key = format!("user_email:{}", id_or_email);
            match (*client).get::<Option<String>, _>(&email_key).await {
                Ok(Some(id)) => format!("user:{}", id),
                Ok(None) => return Ok(None),
                Err(e) => {
                     futures::executor::block_on(self.circuit_breaker.record_failure(node));
                    return Err(AppError::RedisConnection(e.to_string()));
                }
            }
        } else {
            format!("user:{}", id_or_email)
        };

        let data: Option<String> = (*client).get(&key).await.map_err(|e| {
             futures::executor::block_on(self.circuit_breaker.record_failure(node));
            AppError::RedisConnection(e.to_string())
        })?;

        let user = data
            .map(|json_str| serde_json::from_str(&json_str))
            .transpose()
            .map_err(|e| AppError::Internal(e.to_string()))?;

        metrics::record_db_latency("get_user_dragonfly", start);
        Ok(user)
    }

    async fn count_users(&self) -> Result<u64, AppError> {
        let start = Instant::now();
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;
        let pattern = "user:*".to_string();
        let scan_count = Some(1000u32);
        let mut scanner = (*client).scan(pattern, scan_count, Some(ScanType::String));
        let mut count: u64 = 0;

        while let Some(page_result) = scanner.next().await {
            let scan_page: ScanResult = page_result.map_err(|e| {
                futures::executor::block_on(self.circuit_breaker.record_failure(node));
                AppError::RedisConnection(e.to_string())
            })?;
            count += scan_page.results().as_ref().map(|v| v.len()).unwrap_or(0) as u64;
            if !scan_page.has_more() {
                break;
            }
        }

        metrics::record_db_latency("count_users_dragonfly", start);
        Ok(count)
    }

    async fn count_urls(&self, user_id: Option<&str>) -> Result<u64, AppError> {
        let start = Instant::now();
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;

        let count = if let Some(uid) = user_id {
            let index_key = format!("user_urls:{}", uid);
            (*client)
                .scard(&index_key)
                .await
                .map_err(|e| {
                     futures::executor::block_on(self.circuit_breaker.record_failure(node));
                    AppError::RedisConnection(e.to_string())
                })?
        } else {
            let pattern = "url:*".to_string();
            let scan_count = Some(1000u32);
            let mut scanner = (*client).scan(pattern, scan_count, Some(ScanType::String));
            let mut total: u64 = 0;
            while let Some(page_result) = scanner.next().await {
                let scan_page: ScanResult = page_result.map_err(|e| {
                    futures::executor::block_on(self.circuit_breaker.record_failure(node));
                    AppError::RedisConnection(e.to_string())
                })?;
                total += scan_page.results().as_ref().map(|v| v.len()).unwrap_or(0) as u64;
                if !scan_page.has_more() {
                    break;
                }
            }
            total
        };

        metrics::record_db_latency("count_urls_dragonfly", start);
        Ok(count)
    }

    async fn blacklist_token(&self, token: &str, expiry_secs: u64) -> Result<(), AppError> {
        let start = Instant::now();
        let key = format!("token:{}", token);
        let (node, pool) = self.get_pool_for_key(&key)?;
        let client = pool.acquire().await;
        let _: () = (*client)
            .set(&key, "1", Some(Expiration::EX(expiry_secs as i64)), None, false)
            .await
            .map_err(|e| {
                 futures::executor::block_on(self.circuit_breaker.record_failure(node));
                AppError::RedisConnection(e.to_string())
            })?;
        metrics::record_db_latency("blacklist_token_dragonfly", start);
        Ok(())
    }

    async fn is_token_blacklisted(&self, token: &str) -> Result<bool, AppError> {
        let start = Instant::now();
        let key = format!("token:{}", token);
        let (node, pool) = self.get_pool_for_key(&key)?;
        let client = pool.acquire().await;
        let exists: bool = (*client).exists(&key).await.map_err(|e| {
             futures::executor::block_on(self.circuit_breaker.record_failure(node));
            AppError::RedisConnection(e.to_string())
        })?;
        metrics::record_db_latency("is_token_blacklisted_dragonfly", start);
        Ok(exists)
    }

    async fn scan_keys(&self, pattern: &str, count: u32) -> Result<Vec<String>, AppError> {
        let start = Instant::now();
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;
        let mut scanner = (*client).scan(pattern.to_string(), Some(count), Some(ScanType::String));
        let mut keys = Vec::new();

        while let Some(page_result) = scanner.next().await {
            let scan_page: ScanResult = page_result.map_err(|e| {
                futures::executor::block_on(self.circuit_breaker.record_failure(node));
                AppError::RedisConnection(e.to_string())
            })?;
            keys.extend(
                scan_page
                    .results()
                    .as_ref()
                    .map(|v| v.iter().map(|k| k.clone().into_string()).collect::<Vec<_>>())
                    .unwrap_or_default()
                    .into_iter()
            );
            if !scan_page.has_more() {
                break;
            }
        }

        metrics::record_db_latency("scan_keys_dragonfly", start);
        Ok(keys.into_iter().flatten().collect())
    }

    async fn eval_lua(
        &self,
        script: &str,
        keys: Vec<String>,
        args: Vec<String>,
    ) -> Result<i64, AppError> {
        let start = Instant::now();
        let (node, pool) = self.get_pool().await?;
        let client = pool.acquire().await;
        let result: i64 = (*client)
            .eval(script, keys, args)
            .await
            .map_err(|e| {
                futures::executor::block_on(self.circuit_breaker.record_failure(node));
                AppError::RedisConnection(e.to_string())
            })?;
        metrics::record_db_latency("eval_lua_dragonfly", start);
        Ok(result)
    }

    async fn is_global_admin(&self, email: &str) -> Result<bool, AppError> {
        let start = Instant::now();
        let is_admin = self.global_admins.iter().any(|admin| admin == email);
        metrics::record_db_latency("is_global_admin_dragonfly", start);
        Ok(is_admin)
    }
}