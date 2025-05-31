use async_trait::async_trait;
use crate::errors::AppError;
use crate::types::{Paginate, UrlData, User};

#[async_trait]
pub trait Storage {
    // Existing methods (already implemented)
    async fn get(&self, key: &str) -> Result<String, AppError>;
    async fn set_ex(&self, key: &str, value: &str, ttl_seconds: u64) -> Result<(), AppError>;
    async fn zadd(&self, key: &str, score: u64, member: u64) -> Result<(), AppError>;
    async fn rate_limit(&self, key: &str, limit: u64, window_secs: i64) -> Result<bool, AppError>;
    async fn zrange(&self, key: &str, start: i64, end: i64) -> Result<Vec<(u64, u64)>, AppError>;
    async fn zadd_batch(&self, operations: Vec<(String, u64, u64)>, expire_secs: i64) -> Result<(), AppError>;
    async fn scan_keys(&self, pattern: &str, count: u32) -> Result<Vec<String>, AppError>;

   
    async fn delete_url(&self, code: &str, user_id: Option<&str>, user_email: &str) -> Result<(), AppError>;
    async fn list_urls(&self, user_id: Option<&str>, page: u64, per_page: u64) -> Result<Paginate<UrlData>, AppError>;
    async fn set_url(&self, code: &str, url_data: &UrlData) -> Result<(), AppError>;
    async fn set_user(&self, user: &User) -> Result<(), AppError>;
    async fn get_user(&self, id_or_email: &str) -> Result<Option<User>, AppError>;
    async fn count_users(&self) -> Result<u64, AppError>;
    async fn count_urls(&self, user_id: Option<&str>) -> Result<u64, AppError>;
    async fn blacklist_token(&self, token: &str, expiry_secs: u64) -> Result<(), AppError>;
    async fn is_token_blacklisted(&self, token: &str) -> Result<bool, AppError>;
}