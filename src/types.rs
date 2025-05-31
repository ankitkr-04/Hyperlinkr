use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use validator::{Validate, ValidationError};
use crate::validator::{validate_url, validate_custom_alias, validate_rfc3339_date};

#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct ShortenRequest {
    #[validate(url, custom(function = "validate_url"))]
    pub url: String,
    #[validate(length(min = 1, max = 20), custom(function = "validate_custom_alias"))]
    pub custom_alias: Option<String>,
    #[validate(custom(function = "validate_rfc3339_date"))]
    pub expiration_date: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ShortenResponse {
    pub short_url: String, // e.g., "https://api.hyperlinkr.com/abc123"
    pub code: String, // e.g., "abc123"
    pub expiration_date: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UrlData {
    pub long_url: String,
    pub user_id: Option<String>, // CUID, None for anonymous
    pub created_at: String, // ISO 8601
    pub expires_at: Option<String>, // ISO 8601
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthAction {
    Register,
    Login,
}

#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct AuthRequest {
    #[validate(length(min = 1, max = 100))]
    pub username: String,
    #[validate(length(min = 8, max = 100))]
    pub password: String,
    #[validate(email)]
    pub email: Option<String>,
    pub action: AuthAction,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub message: String,
    pub token: Option<String>, // JWT
}

#[derive(Debug, Serialize)]
pub struct AnalyticsRequest {
    pub code: String, // URL code
    pub start_date: Option<String>, // ISO 8601
    pub end_date: Option<String>, // ISO 8601
}

#[derive(Debug, Serialize)]
pub struct AnalyticsResponse {
    pub code: String, // URL code
    pub total_clicks: u64,
    pub daily_clicks: HashMap<String, u64>,
    pub total_urls: Option<u64>, // Admin-only
    pub total_users: Option<u64>, // Admin-only
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: String, // CUID
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub created_at: String, // ISO 8601
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthToken {
    pub user_id: Option<String>, // CUID, None for anonymous
    pub expires_at: String, // ISO 8601
    pub is_admin: bool, // True if email in global_admins
}

#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct DeleteRequest {
    #[validate(length(min = 1, max = 20))]
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct DeleteResponse {
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub message: String,
    pub details: Option<String>, // e.g., validation errors
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<ErrorResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Paginate<T> {
    pub items: Vec<T>,
    pub page: u64,
    pub per_page: u64,
    pub total_items: u64,
    pub total_pages: u64,
}
