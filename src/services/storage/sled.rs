use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use sled::Db;
use crate::errors::AppError;
use super::storage::Storage;

// Note: SledStorage is for testing only; use DatabaseClient for production
pub struct SledStorage {
    db: Arc<Mutex<Db>>,
}

impl SledStorage {
    pub fn new(path: &str) -> Self {
        let db = sled::open(path).expect("Failed to open sled database");
        Self { db: Arc::new(Mutex::new(db)) }
    }
}

#[async_trait]
impl Storage for SledStorage {
    async fn get(&self, key: &str) -> Result<String, AppError> {
        let db = self.db.lock().await;
        db.get(key)
            .map_err(|e| AppError::Sled(e))?
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .ok_or(AppError::NotFound("Key not found".to_string()))
    }

    async fn set_ex(&self, key: &str, value: &str, _ttl_seconds: u64) -> Result<(), AppError> {
        let db = self.db.lock().await;
        db.insert(key, value.as_bytes())
            .map_err(|e| AppError::Sled(e))?;
        // Note: sled does not support TTL natively; consider cleanup task for production
        Ok(())
    }

    async fn zadd(&self, key: &str, score: u64, member: u64) -> Result<(), AppError> {
        let db = self.db.lock().await;
        db.insert(format!("{}:{}", key, member), score.to_le_bytes().to_vec())
            .map_err(|e| AppError::Sled(e))?;
        Ok(())
    }

    async fn rate_limit(&self, key: &str, limit: u64, window_secs: i64) -> Result<bool, AppError> {
        let db = self.db.lock().await;
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| AppError::Internal(e.to_string()))?
            .as_secs() as i64;

        let count_key = format!("{}:count", key);
        let timestamp_key = format!("{}:timestamp", key);

        // Get current count and timestamp
        let count: u64 = db.get(&count_key)
            .map_err(|e| AppError::Sled(e))?
            .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let last_timestamp: i64 = db.get(&timestamp_key)
            .map_err(|e| AppError::Sled(e))?
            .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // Check if we are still within the window
        if current_time - last_timestamp > window_secs {
            // Reset count and timestamp
            db.insert(&count_key, "1".as_bytes())
                .map_err(|e| AppError::Sled(e))?;
            db.insert(&timestamp_key, current_time.to_string().as_bytes())
                .map_err(|e| AppError::Sled(e))?;
            return Ok(true);
        }

        // Check if we can increment the count
        if count < limit {
            db.insert(&count_key, (count + 1).to_string().as_bytes())
                .map_err(|e| AppError::Sled(e))?;
            return Ok(true);
        }

        Ok(false) // Rate limit exceeded
    }

    async fn zrange(&self, key: &str, start: i64, end: i64) -> Result<Vec<(u64, u64)>, AppError> {
        let db = self.db.lock().await;
        let mut results = Vec::new();
        for entry in db.scan_prefix(key).skip(start as usize).take((end - start + 1) as usize) {
            match entry {
                Ok((k, v)) => {
                    if let Ok(key_str) = std::str::from_utf8(&k) {
                        if let Some(member_str) = key_str.split(':').last() {
                            if let Ok(member) = member_str.parse::<u64>() {
                                if let Ok(bytes) = v.as_ref().try_into() {
                                    let score = u64::from_le_bytes(bytes);
                                    results.push((score, member));
                                }
                            }
                        }
                    }
                }
                Err(e) => return Err(AppError::Sled(e)),
            }
        }
        Ok(results)
    }

    async fn zadd_batch(&self, operations: Vec<(String, u64, u64)>, _expire_secs: i64) -> Result<(), AppError> {
        let db = self.db.lock().await;
        for (key, score, member) in operations {
            db.insert(format!("{}:{}", key, member), score.to_le_bytes().to_vec())
                .map_err(|e| AppError::Sled(e))?;
        }
        Ok(())
    }
}