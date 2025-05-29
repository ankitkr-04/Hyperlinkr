use async_trait::async_trait;
use crate::errors::AppError;

#[async_trait]
pub trait Storage {
    async fn get(&self, key: &str) -> Result<String, AppError>;
    async fn set_ex(&self, key: &str, value: &str, ttl_seconds: u64) -> Result<(), AppError>;
    async fn zadd(&self, key: &str, score: u64, member: u64) -> Result<(), AppError>;
    async fn rate_limit(&self, key: &str, limit: u64, window_secs: i64) -> Result<bool, AppError>;
    
}