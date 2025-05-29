pub mod dragonfly;
pub mod sled;

use async_trait::async_trait;
use crate::errors::AppError;

#[async_trait]
pub trait Storage {
    async fn get(&self, key: &str) -> Result<String, AppError>;
    async fn set_ex(&self, key: &str, value: &str, ttl_seconds: u64) -> Result<(), AppError>;
}