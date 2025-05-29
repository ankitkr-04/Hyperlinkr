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

    async fn set_ex(&self, key: &str, value: &str, ttl_seconds: u64) -> Result<(), AppError> {
        let db = self.db.lock().await;
        db.insert(key, value.as_bytes())
            .map_err(|e| AppError::Sled(e))?;
        // Note: sled does not support TTL natively; consider cleanup task for production
        Ok(())
    }

    async fn zadd(&self, key: &str, score: u64, member: u64) -> Result<(), AppError> {
        let db = self.db.lock().await;
        db.insert(format!("{}:{}", key, member), score.to_le_bytes())
            .map_err(|e| AppError::Sled(e))?;
        Ok(())
    }
}