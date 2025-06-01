use async_trait::async_trait;
use sled::{Db, Batch, IVec};
use bincode::{config, decode_from_slice, encode_to_vec};
use std::sync::Arc;
use std::time::{Instant, Duration};
use crate::{
    config::settings::Settings,
    errors::AppError,
    services::metrics,
    types::{Paginate, UrlData, User},
    clock::{Clock, SystemClock},
};
use super::storage::Storage;

pub struct SledStorage<C: Clock = SystemClock> {
    db: Arc<Db>,
    clock: C,
    snapshot_ttl: Duration,
    global_admins: Vec<String>,
}

impl SledStorage {
    pub fn new(config: &Settings) -> Self {
        Self::with_clock(config, SystemClock)
    }
}

impl<C: Clock> SledStorage<C> {
    pub fn with_clock(config: &Settings, clock: C) -> Self {
        let sled_config = sled::Config::new()
            .path(&config.cache.sled_path)
            .cache_capacity(config.cache.sled_cache_bytes * 2) // Double cache for hot keys
            .use_compression(true)
            .compression_factor(8)
            .flush_every_ms(Some(10)); // Aggressive flush
        let db = sled_config.open().expect("Failed to open sled database");
        Self {
            db: Arc::new(db),
            clock,
            snapshot_ttl: Duration::from_secs(config.cache.sled_snapshot_ttl_secs),
            global_admins: config.security.global_admins.clone(),
        }
    }

    fn url_index_key(user_id: &str, code: &str) -> Vec<u8> {
        format!("index:user_urls:{}:{}", user_id, code).into_bytes()
    }

    fn url_index_prefix(user_id: &str) -> Vec<u8> {
        format!("index:user_urls:{}:", user_id).into_bytes()
    }
}

#[async_trait]
impl<C: Clock + Send + Sync> Storage for SledStorage<C> {
    async fn get(&self, key: &str) -> Result<String, AppError> {
        let start = Instant::now();
        let bytes = self.db.get(key.as_bytes()).map_err(|e| AppError::Sled(e))?
            .ok_or_else(|| AppError::NotFound(key.into()))?;
        let result = String::from_utf8(bytes.to_vec())
            .map_err(|e| AppError::Internal(e.to_string()))?;
        metrics::record_db_latency("get_sled", start);
        Ok(result)
    }

    async fn set_ex(&self, key: &str, value: &str, ttl_seconds: u64) -> Result<(), AppError> {
        let start = Instant::now();
        let expiry = self.clock.now().timestamp() as u64 + ttl_seconds;
        let mut data = value.as_bytes().to_vec();
        data.extend_from_slice(expiry.to_le_bytes().as_ref());
        self.db.insert(key.as_bytes(), data).map_err(|e| AppError::Sled(e))?;
        metrics::record_db_latency("set_ex_sled", start);
        Ok(())
    }

    async fn zadd(&self, key: &str, score: u64, member: u64) -> Result<(), AppError> {
        let start = Instant::now();
        let config = config::standard().with_variable_int_encoding();
        let mut batch = Batch::default();
        let data = self.db.get(key.as_bytes()).map_err(|e| AppError::Sled(e))?
            .map(|v| decode_from_slice::<Vec<(u64, u64)>, _>(&v, config)
                .map(|(data, _)| data)
                .unwrap_or_default())
            .unwrap_or_default();
        let mut new_data = data.into_iter().filter(|&(_, m)| m != member).collect::<Vec<_>>();
        new_data.push((score, member));
        new_data.sort_by_key(|&(s, _)| s);
        batch.insert(key.as_bytes(), encode_to_vec(&new_data, config)
            .map_err(|e| AppError::Internal(e.to_string()))?);
        self.db.apply_batch(batch).map_err(|e| AppError::Sled(e))?;
        metrics::record_db_latency("zadd_sled", start);
        Ok(())
    }

    async fn rate_limit(&self, key: &str, limit: u64, window_secs: i64) -> Result<bool, AppError> {
        let start = Instant::now();
        let now = self.clock.now().timestamp();
        let key_bytes = key.as_bytes();
        let mut batch = Batch::default();
        let (count, last_timestamp) = self.db.get(key_bytes).map_err(|e| AppError::Sled(e))?
            .map(|bytes| {
                if bytes.len() == 16 {
                    let count_bytes: [u8; 8] = bytes[0..8].try_into().unwrap();
                    let timestamp_bytes: [u8; 8] = bytes[8..16].try_into().unwrap();
                    (u64::from_le_bytes(count_bytes), i64::from_le_bytes(timestamp_bytes))
                } else {
                    (0, 0)
                }
            })
            .unwrap_or((0, 0));
        let allowed = if now >= last_timestamp + window_secs {
            batch.insert(key_bytes, {
                let mut b = Vec::with_capacity(16);
                b.extend_from_slice(&1u64.to_le_bytes());
                b.extend_from_slice(&now.to_le_bytes().as_ref());
                b
            });
            true
        } else if count < limit {
            batch.insert(key_bytes, {
                let mut b = Vec::with_capacity(16);
                b.extend_from_slice(&(count + 1).to_le_bytes());
                b.extend_from_slice(&last_timestamp.to_le_bytes().as_ref());
                b
            });
            true
        } else {
            false
        };
        self.db.apply_batch(batch).map_err(|e| AppError::Sled(e))?;
        metrics::record_db_latency("rate_limit_sled", start);
        Ok(allowed)
    }

    async fn zrange(&self, key: &str, start: i64, end: i64) -> Result<Vec<(u64, u64)>, AppError> {
        let start_time = Instant::now();
        let config = config::standard().with_variable_int_encoding();
        let data = self.db.get(key.as_bytes()).map_err(|e| AppError::Sled(e))?
            .map(|v| decode_from_slice::<Vec<(u64, u64)>, _>(&v, config)
                .map(|(data, _)| data)
                .unwrap_or_default())
            .unwrap_or_default();
        let start_idx = start.max(0) as usize;
        let end_idx = if end < 0 { data.len() } else { (end + 1) as usize };
        let result = data
            .into_iter()
            .skip(start_idx)
            .take(end_idx.saturating_sub(start_idx))
            .collect();
        metrics::record_db_latency("zrange_sled", start_time);
        Ok(result)
    }

    async fn zadd_batch(
        &self,
        operations: Vec<(String, u64, u64)>,
        _expire_secs: i64,
    ) -> Result<(), AppError> {
        let start = Instant::now();
        let config = config::standard().with_variable_int_encoding();
        let mut batch = Batch::default();
        let mut grouped = std::collections::HashMap::new();
        for (key, score, member) in operations {
            grouped.entry(key).or_insert_with(Vec::new).push((score, member));
        }
        for (key, ops) in grouped {
            let data = self.db.get(key.as_bytes()).map_err(|e| AppError::Sled(e))?
                .map(|v| decode_from_slice::<Vec<(u64, u64)>, _>(&v, config)
                    .map(|(data, _)| data)
                    .unwrap_or_default())
                .unwrap_or_default();
            let mut new_data = data;
            for (score, member) in ops {
                new_data.retain(|&(_, m)| m != member);
                new_data.push((score, member));
            }
            new_data.sort_by_key(|&(s, _)| s);
            batch.insert(key.as_bytes(), encode_to_vec(&new_data, config)
                .map_err(|e| AppError::Internal(e.to_string()))?);
        }
        self.db.apply_batch(batch).map_err(|e| AppError::Sled(e))?;
        metrics::record_db_latency("zadd_batch_sled", start);
        Ok(())
    }

    async fn delete_url(&self, code: &str, user_id: Option<&str>, user_email: &str) -> Result<(), AppError> {
        let start = Instant::now();
        let key = format!("url:{}", code);
        let is_admin = self.global_admins.iter().any(|admin| admin == user_email);
        let mut batch = Batch::default();

        let data = self.db.get(&key).map_err(|e| AppError::Sled(e))?;
        if let Some(bytes) = data {
            let url_data: UrlData = decode_from_slice(&bytes, config::standard())
                .map(|(data, _)| data)
                .map_err(|e| AppError::Internal(e.to_string()))?;

            let is_owner = url_data.user_id.as_deref() == user_id || url_data.user_id.is_none();
            if !is_owner && !is_admin {
                return Err(AppError::Unauthorized("Not authorized to delete this URL".into()));
            }

            batch.remove(&key);
            if let Some(uid) = user_id {
                batch.remove(Self::url_index_key(uid, code));
            }
            self.db.apply_batch(batch).map_err(|e| AppError::Sled(e))?;
        } else {
            return Err(AppError::NotFound(format!("URL {} not found", code)));
        }

        metrics::record_db_latency("delete_url_sled", start);
        Ok(())
    }

    async fn set_url(&self, code: &str, url_data: &UrlData) -> Result<(), AppError> {
        let start = Instant::now();
        let key = format!("url:{}", code);
        let data = encode_to_vec(url_data, config::standard().with_variable_int_encoding())
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let mut batch = Batch::default();
        batch.insert(&key, data);
        if let Some(user_id) = &url_data.user_id {
            batch.insert(Self::url_index_key(user_id, code), vec![1u8]);
        }
        self.db.apply_batch(batch).map_err(|e| AppError::Sled(e))?;
        metrics::record_db_latency("set_url_sled", start);
        Ok(())
    }

    async fn list_urls(&self, user_id: Option<&str>, page: u64, per_page: u64) -> Result<Paginate<UrlData>, AppError> {
        let start = Instant::now();
        let is_admin = user_id.is_none();
        let per_page = per_page.clamp(1, 100);
        let offset = page.saturating_sub(1) * per_page;

        let mut items = Vec::new();
        let mut total_items = 0;

        if is_admin {
            for entry in self.db.scan_prefix("url:") {
                let (key, value) = entry.map_err(|e| AppError::Sled(e))?;
                let url_data: UrlData = decode_from_slice(&value, config::standard())
                    .map(|(data, _)| data)
                    .map_err(|e| AppError::Internal(e.to_string()))?;
                total_items += 1;
                if total_items > offset && items.len() < per_page as usize {
                    items.push(url_data);
                }
            }
        } else if let Some(uid) = user_id {
            let prefix = Self::url_index_prefix(uid);
            let codes: Vec<String> = self.db.scan_prefix(&prefix)
                .filter_map(|entry| {
                    entry.ok().map(|(key, _)| {
                        String::from_utf8(key.to_vec())
                            .map(|k| k.split(':').last().unwrap_or("").to_string())
                            .unwrap_or_default()
                    })
                })
                .collect();
            total_items = codes.len() as u64;
            let start_idx = offset.min(total_items) as usize;
            let end_idx = (offset + per_page).min(total_items) as usize;

            for code in codes.into_iter().skip(start_idx).take(end_idx - start_idx) {
                let key = format!("url:{}", code);
                if let Some(value) = self.db.get(&key).map_err(|e| AppError::Sled(e))? {
                    let url_data: UrlData = decode_from_slice(&value, config::standard())
                        .map(|(data, _)| data)
                        .map_err(|e| AppError::Internal(e.to_string()))?;
                    items.push(url_data);
                }
            }
        }

        let total_pages = if total_items == 0 { 1 } else { (total_items + per_page - 1) / per_page };
        metrics::record_db_latency("list_urls_sled", start);
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
        let mut batch = Batch::default();
        batch.insert(&key, encode_to_vec(user, config::standard().with_variable_int_encoding())
            .map_err(|e| AppError::Internal(e.to_string()))?);
        batch.insert(&email_key, user.id.as_bytes());
        self.db.apply_batch(batch).map_err(|e| AppError::Sled(e))?;
        metrics::record_db_latency("set_user_sled", start);
        Ok(())
    }

    async fn get_user(&self, id_or_email: &str) -> Result<Option<User>, AppError> {
        let start = Instant::now();
        let key = if id_or_email.contains('@') {
            let email_key = format!("user_email:{}", id_or_email);
            self.db.get(&email_key).map_err(|e| AppError::Sled(e))?
                .map(|id_bytes| format!("user:{}", String::from_utf8(id_bytes.to_vec())
                    .map_err(|e| AppError::Internal(e.to_string()))?))
                .unwrap_or_default()
        } else {
            format!("user:{}", id_or_email)
        };

        let user = self.db.get(&key).map_err(|e| AppError::Sled(e))?
            .map(|bytes| decode_from_slice::<User, _>(&bytes, config::standard())
                .map(|(data, _)| data)
                .map_err(|e| AppError::Internal(e.to_string())))
            .transpose()?;

        metrics::record_db_latency("get_user_sled", start);
        Ok(user)
    }

    async fn count_users(&self) -> Result<u64, AppError> {
        let start = Instant::now();
        let count = self.db.scan_prefix("user:").count() as u64;
        metrics::record_db_latency("count_users_sled", start);
        Ok(count)
    }

    async fn count_urls(&self, user_id: Option<&str>) -> Result<u64, AppError> {
        let start = Instant::now();
        let count = if let Some(uid) = user_id {
            self.db.scan_prefix(Self::url_index_prefix(uid)).count() as u64
        } else {
            self.db.scan_prefix("url:").count() as u64
        };
        metrics::record_db_latency("count_urls_sled", start);
        Ok(count)
    }

    async fn blacklist_token(&self, token: &str, expiry_secs: u64) -> Result<(), AppError> {
        let start = Instant::now();
        let key = format!("token:{}", token);
        let expiry = self.clock.now().timestamp() as u64 + expiry_secs;
        let mut data = vec![1u8];
        data.extend_from_slice(&expiry.to_le_bytes().as_ref());
        self.db.insert(&key, data).map_err(|e| AppError::Sled(e))?;
        metrics::record_db_latency("blacklist_token_sled", start);
        Ok(())
    }

    async fn is_token_blacklisted(&self, token: &str) -> Result<bool, AppError> {
        let start = Instant::now();
        let key = format!("token:{}", token);
        let exists = self.db.get(&key).map_err(|e| AppError::Sled(e))?.is_some();
        metrics::record_db_latency("is_token_blacklisted_sled", start);
        Ok(exists)
    }

    async fn scan_keys(&self, pattern: &str, count: u32) -> Result<Vec<String>, AppError> {
        let start = Instant::now();
        let prefix = pattern.trim_end_matches('*');
        let keys: Vec<String> = self.db.scan_prefix(prefix)
            .take(count as usize)
            .filter_map(|entry| {
                entry.ok().map(|(key, _)| String::from_utf8(key.to_vec()).ok()).flatten()
            })
            .collect();
        metrics::record_db_latency("scan_keys_sled", start);
        Ok(keys)
    }

    async fn eval_lua(
        &self,
        _script: &str,
        _keys: Vec<String>,
        _args: Vec<String>,
    ) -> Result<i64, AppError> {
        Err(AppError::Internal("Lua scripting not supported in Sled".into()))
    }


    async fn is_global_admin(&self, user_email: &str) -> bool {
        self.global_admins.iter().any(|admin| admin == user_email)
    }
}