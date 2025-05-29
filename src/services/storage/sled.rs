use std::sync::Arc;
use tokio::sync::Mutex;
use sled::Db;
use crate::errors::AppError;
use async_trait::async_trait;
use crate::services::storage::Storage;

pub struct SledStorage {
    db: Arc<Mutex<Db>>,
}

impl SledStorage {
    pub fn new(path: &str) -> Self {
        let db = sled::open(path).unwrap();
        Self {
            db: Arc::new(Mutex::new(db)),
        }
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
        Ok(())
    }
}