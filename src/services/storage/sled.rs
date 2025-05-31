use async_trait::async_trait;
use std::sync::Arc;
use sled::{Db, Batch, Transactional, IVec};
use bincode::{config, decode_from_slice, encode_to_vec, Decode, Encode};
use chrono::{DateTime, Utc};
use std::time::{Instant, Duration};
use crate::errors::AppError;
use crate::services::metrics;
use super::storage::Storage;
use crate::clock::{Clock, SystemClock};
use crate::config::settings::Settings;

use crate::types::{Paginate, UrlData, User};

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
            .cache_capacity(config.cache.sled_cache_bytes)
            .use_compression(config.cache.sled_compression)
            .flush_every_ms(Some(config.cache.sled_flush_ms));
        let db = sled_config.open().expect("Failed to open sled database");
        Self {
            db: Arc::new(db),
            clock,
            snapshot_ttl: Duration::from_secs(config.cache.sled_snapshot_ttl_secs),
            global_admins: config.security.global_admins.iter().cloned().collect(),
        }
    }

    fn name_prefix(name: &str) -> [u8; 24] {
        let mut buf = [0u8; 24];
        let bytes = name.as_bytes();
        let len = bytes.len().min(24);
        buf[..len].copy_from_slice(&bytes[..len]);
        buf
    }

    fn snapshot_key(name: &str) -> Vec<u8> {
        let mut v = b"snapshot:".to_vec();
        v.extend_from_slice(&Self::name_prefix(name));
        v
    }

    fn snapshot_meta_key(name: &str) -> Vec<u8> {
        let mut v = b"snapshot_meta:".to_vec();
        v.extend_from_slice(&Self::name_prefix(name));
        v
    }

    async fn maybe_fetch_snapshot(&self, name: &str) -> Result<Option<Vec<u8>>, AppError> {
        let meta_key = Self::snapshot_meta_key(name);
        if let Some(ivec) = self.db.get(&meta_key)? {
            let bytes = ivec.as_ref();
            if bytes.len() == 8 {
                let last_built_ts = i64::from_be_bytes(bytes[0..8].try_into().unwrap());
                let now_ts = self.clock.now().timestamp();
                if (now_ts - last_built_ts) as u64 <= self.snapshot_ttl.as_secs() {
                    let snap_key = Self::snapshot_key(name);
                    if let Some(blob) = self.db.get(&snap_key)? {
                        return Ok(Some(blob.to_vec()));
                    }
                }
            }
        }
        Ok(None)
    }

    async fn rebuild_snapshot(&self, name: &str) -> Result<(), AppError> {
        let start = Instant::now();
        let config = config::standard();
        let data = self.db
            .get(name.as_bytes())
            .map_err(|e| {
                metrics::record_db_error("snapshot_fetch");
                AppError::Sled(e)
            })?
            .map(|v| {
                decode_from_slice::<Vec<(u64, u64)>, _>(&v, config)
                    .map(|(data, _)| data)
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let serialized = encode_to_vec(&data, config).map_err(|e| AppError::Internal(e.to_string()))?;
        let snap_key = Self::snapshot_key(name);
        let meta_key = Self::snapshot_meta_key(name);
        let mut batch = Batch::default();
        batch.insert(snap_key, serialized);
        batch.insert(meta_key, self.clock.now().timestamp().to_be_bytes().to_vec());
        self.db.apply_batch(batch).map_err(|e| {
            metrics::record_db_error("snapshot_apply");
            AppError::Sled(e)
        })?;
        metrics::record_db_latency("rebuild_snapshot", start);
        Ok(())
    }
}

#[async_trait]
impl<C: Clock + Send + Sync> Storage for SledStorage<C> {
    async fn get(&self, key: &str) -> Result<String, AppError> {
        let start = Instant::now();
        let bytes = self
            .db
            .get(key.as_bytes())
            .map_err(|e| {
                metrics::record_db_error("get_sled");
                AppError::Sled(e)
            })?
            .ok_or_else(|| {
                metrics::record_db_error("get_sled");
                AppError::NotFound(key.into())
            })?;
        let result = String::from_utf8(bytes.to_vec()).map_err(|e| AppError::Internal(e.to_string()))?;
        metrics::record_db_latency("get_sled", start);
        Ok(result)
    }

    async fn set_ex(&self, key: &str, value: &str, ttl_seconds: u64) -> Result<(), AppError> {
        let start = Instant::now();
        let expiry = self.clock.now().timestamp() as u64 + ttl_seconds;
        let mut data = value.as_bytes().to_vec();
        data.extend_from_slice(expiry.to_le_bytes().as_ref());
        self.db.insert(key.as_bytes(), data).map_err(|e| {
            metrics::record_db_error("set_ex_sled");
            AppError::Sled(e)
        })?;
        metrics::record_db_latency("set_ex_sled", start);
        Ok(())
    }

    async fn zadd(&self, key: &str, score: u64, member: u64) -> Result<(), AppError> {
        let start = Instant::now();
        let config = config::standard();
        let result = self.db.transaction(|tx| {
            let data = tx
                .get(key.as_bytes())?
                .map(|v| {
                    decode_from_slice::<Vec<(u64, u64)>, _>(&v, config)
                        .map(|(data, _)| data)
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            let mut new_data = data.into_iter().filter(|&(_, m)| m != member).collect::<Vec<_>>();
            new_data.push((score, member));
            new_data.sort_by_key(|&(s, _)| s);
            tx.insert(
                key.as_bytes(),
                encode_to_vec(&new_data, config).map_err(|e| AppError::Internal(e.to_string()))?,
            )?;
            tx.remove(Self::snapshot_meta_key(key))?;
            Ok(())
        });
        result.map_err(|e| {
            metrics::record_db_error("zadd_sled");
            AppError::Sled(e.into())
        })?;
        metrics::record_db_latency("zadd_sled", start);
        Ok(())
    }

    async fn rate_limit(&self, key: &str, limit: u64, window_secs: i64) -> Result<bool, AppError> {
        let start = Instant::now();
        let now = self.clock.now().timestamp();
        let key_bytes = key.as_bytes();
        let result = self.db.transaction(|tx| {
            let state = tx.get(key_bytes)?;
            let (mut count, mut last_timestamp) = if let Some(bytes) = state {
                if bytes.len() == 16 {
                    let count_bytes: [u8; 8] = bytes[0..8].try_into().unwrap();
                    let timestamp_bytes: [u8; 8] = bytes[8..16].try_into().unwrap();
                    (u64::from_le_bytes(count_bytes), i64::from_le_bytes(timestamp_bytes))
                } else {
                    (0, 0)
                }
            } else {
                (0, 0)
            };
            let allowed = if now >= last_timestamp + window_secs {
                count = 1;
                last_timestamp = now;
                true
            } else if count < limit {
                count += 1;
                true
            } else {
                false
            };
            let mut new_bytes = Vec::with_capacity(16);
            new_bytes.extend_from_slice(&count.to_le_bytes());
            new_bytes.extend_from_slice(&last_timestamp.to_le_bytes().as_ref());
            tx.insert(key_bytes, new_bytes)?;
            Ok(allowed)
        });
        let allowed = result.map_err(|e| {
            metrics::record_db_error("rate_limit_sled");
            AppError::Sled(e.into())
        })?;
        metrics::record_db_latency("rate_limit_sled", start);
        Ok(allowed)
    }

    async fn zrange(&self, key: &str, start: i64, end: i64) -> Result<Vec<(u64, u64)>, AppError> {
        let start_time = Instant::now();
        let config = config::standard();
        let data = self
            .db
            .get(key.as_bytes())
            .map_err(|e| {
                metrics::record_db_error("zrange_sled");
                AppError::Sled(e)
            })?
            .map(|v| {
                decode_from_slice::<Vec<(u64, u64)>, _>(&v, config)
                    .map(|(data, _)| data)
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        if data.len() > 1000 {
            // Use snapshot for large sorted sets
            if let Some(blob) = self.maybe_fetch_snapshot(key).await? {
                let all_pairs: Vec<(u64, u64)> = decode_from_slice(&blob, config)
                    .map(|(data, _)| data)
                    .map_err(|e| AppError::Internal(e.to_string()))?;
                let len = all_pairs.len() as i64;
                let s = start.max(0) as usize;
                let e = if end < 0 { len as usize } else { (end + 1).min(len) as usize };
                let slice = if s < e {
                    all_pairs[s..e]
                        .iter()
                        .filter(|&(score, _)| {
                            let now = self.clock.now().timestamp() as u64;
                            score > &now.saturating_sub(90 * 24 * 3600)
                        })
                        .copied()
                        .collect()
                } else {
                    Vec::new()
                };
                metrics::record_db_latency("zrange_sled_snapshot", start_time);
                return Ok(slice);
            }
            let _ = self.rebuild_snapshot(key).await; // Rebuild async
        }
        let start_idx = start.max(0) as usize;
        let end_idx = if end < 0 { data.len() } else { (end + 1) as usize };
        let result = data
            .into_iter()
            .skip(start_idx)
            .take(end_idx.saturating_sub(start_idx))
            .filter(|&(score, _)| {
                let now = self.clock.now().timestamp() as u64;
                score > now.saturating_sub(90 * 24 * 3600)
            })
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
        let config = config::standard();
        let result = self.db.transaction(|tx| {
            use sled::transaction::ConflictableTransactionError;
            let mut batches = std::collections::HashMap::new();
            for (key, score, member) in &operations {
                let data = batches
                    .entry(key.clone())
                    .or_insert_with(|| {
                        tx.get(key.as_bytes())
                            .map_err(ConflictableTransactionError::Storage)?
                            .map(|v| {
                                decode_from_slice::<Vec<(u64, u64)>, _>(&v, config)
                                    .map(|(data, _)| data)
                                    .unwrap_or_default()
                            })
                            .unwrap_or_default()
                    });
                data.retain(|&(_, m)| m != *member);
                data.push((*score, *member));
                data.sort_by_key(|&(s, _)| s);
            }
            for (key, data) in batches {
                tx.insert(
                    key.as_bytes(),
                    encode_to_vec(&data, config)
                        .map_err(|e| ConflictableTransactionError::Abort(AppError::Internal(e.to_string())))?,
                )?;
                tx.remove(Self::snapshot_meta_key(&key))?;
            }
            Ok(())
        });
        result.map_err(|e| {
            metrics::record_db_error("zadd_batch_sled");
            AppError::Sled(e.into())
        })?;
        metrics::record_db_latency("zadd_batch_sled", start);
        Ok(())
    }

    async fn delete_url(&self, code: &str, user_id: Option<&str>, user_email: &str) -> Result<(), AppError> {
        let start = Instant::now();
        let key = format!("url:{}", code);
        let is_admin = self.global_admins.iter().any(|admin| admin == user_email);

        let data = self.db.get(&key).map_err(|e| {
            metrics::record_db_error("delete_url_sled");
            AppError::Sled(e)
        })?;

        if let Some(bytes) = data {
            let url_data: UrlData = decode_from_slice(&bytes, config::standard())
                .map(|(data, _)| data)
                .map_err(|e| AppError::Internal(e.to_string()))?;

            // Check ownership or admin status
            let is_owner = url_data.user_id.as_deref() == user_id || url_data.user_id.is_none();
            if !is_owner && !is_admin {
                return Err(AppError::Unauthorized("Not authorized to delete this URL".into()));
            }

            self.db.remove(&key).map_err(|e| {
                metrics::record_db_error("delete_url_sled");
                AppError::Sled(e)
            })?;
        } else {
            return Err(AppError::NotFound(format!("URL {} not found", code)));
        }

        metrics::record_db_latency("delete_url_sled", start);
        Ok(())
    }

    async fn list_urls(&self, user_id: Option<&str>, page: u64, per_page: u64) -> Result<Paginate<UrlData>, AppError> {
        let start = Instant::now();
        let is_admin = user_id.is_none(); // Admins pass None
        let per_page = per_page.clamp(1, 100); // Limit page size
        let offset = page.saturating_sub(1) * per_page;

        let mut items = Vec::new();
        let mut total_items = 0;

        for entry in self.db.scan_prefix("url:") {
            let (key, value) = entry.map_err(|e| {
                metrics::record_db_error("list_urls_sled");
                AppError::Sled(e)
            })?;

            let url_data: UrlData = decode_from_slice(&value, config::standard())
                .map(|(data, _)| data)
                .map_err(|e| AppError::Internal(e.to_string()))?;

            // Filter by user_id (unless admin)
            if is_admin || url_data.user_id.as_deref() == user_id || url_data.user_id.is_none() {
                total_items += 1;
                if total_items > offset && items.len() < per_page as usize {
                    items.push(url_data);
                }
            }
        }

        let total_pages = (total_items + per_page - 1) / per_page;
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
        let data = encode_to_vec(user, config::standard())
            .map_err(|e| AppError::Internal(e.to_string()))?;

        self.db.insert(key, data).map_err(|e| {
            metrics::record_db_error("set_user_sled");
            AppError::Sled(e)
        })?;

        // Index by email for lookup
        let email_key = format!("user_email:{}", user.email);
        self.db.insert(email_key, user.id.as_bytes()).map_err(|e| {
            metrics::record_db_error("set_user_sled");
            AppError::Sled(e)
        })?;

        metrics::record_db_latency("set_user_sled", start);
        Ok(())
    }

    async fn get_user(&self, id_or_email: &str) -> Result<Option<User>, AppError> {
        let start = Instant::now();
        let key = if id_or_email.contains('@') {
            // Lookup by email
            let email_key = format!("user_email:{}", id_or_email);
            match self.db.get(&email_key).map_err(|e| {
                metrics::record_db_error("get_user_sled");
                AppError::Sled(e)
            })? {
                Some(id_bytes) => format!(
                    "user:{}",
                    String::from_utf8(id_bytes.to_vec()).map_err(|e| AppError::Internal(e.to_string()))?
                ),
                None => return Ok(None),
            }
        } else {
            format!("user:{}", id_or_email)
        };

        let data = self.db.get(&key).map_err(|e| {
            metrics::record_db_error("get_user_sled");
            AppError::Sled(e)
        })?;

        let user = if let Some(bytes) = data {
            Some(
                decode_from_slice::<User, _>(&bytes, config::standard())
                    .map(|(data, _)| data)
                    .map_err(|e| AppError::Internal(e.to_string()))?,
            )
        } else {
            None
        };

        metrics::record_db_latency("get_user_sled", start);
        Ok(user)
    }

    async fn count_users(&self) -> Result<u64, AppError> {
        let start = Instant::now();
        let count = self
            .db
            .scan_prefix("user:")
            .count()
            .try_into()
            .map_err(|e| AppError::Internal(TryFromIntError::to_string()))?;
        metrics::record_db_latency("count_users_sled", start);
        Ok(count)
    }

    async fn count_urls(&self, user_id: Option<&str>) -> Result<u64, AppError> {
        let start = Instant::now();
        let is_admin = user_id.is_none();
        let mut count = 0;

        for entry in self.db.scan_prefix("url:") {
            let (_, value) = entry.map_err(|e| {
                metrics::record_db_error("count_urls_sled");
                AppError::Sled(e)
            })?;

            let url_data: UrlData = decode_from_slice(&value, config::standard())
                .map(|(data, _)| data)
                .map_err(|e| AppError::Internal(e.to_string()))?;

            if is_admin || url_data.user_id.as_deref() == user_id || url_data.user_id.is_none() {
                count += 1;
            }
        }

        metrics::record_db_latency("count_urls_sled", start);
        Ok(count)
    }

    async fn blacklist_token(&self, token: &str, expiry_secs: u64) -> Result<(), AppError> {
        let start = Instant::now();
        let key = format!("token:{}", token);
        let expiry = self.clock.now().timestamp() as u64 + expiry_secs;
        let mut data = vec![1u8]; // Minimal value
        data.extend_from_slice(expiry.to_le_bytes().as_ref());

        self.db.insert(key, data).map_err(|e| {
            metrics::record_db_error("blacklist_token_sled");
            AppError::Sled(e)
        })?;

        metrics::record_db_latency("blacklist_token_sled", start);
        Ok(())
    }

    async fn is_token_blacklisted(&self, token: &str) -> Result<bool, AppError> {
        let start = Instant::now();
        let key = format!("token:{}", token);
        let exists = self.db.get(&key).map_err(|e| {
            metrics::record_db_error("is_token_blacklisted_sled");
            AppError::Sled(e)
        })?.is_some();
        metrics::record_db_latency("is_token_blacklisted_sled", start);
        Ok(exists)
    }
}