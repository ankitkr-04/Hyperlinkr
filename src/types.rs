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
    pub username: String,
    pub email: String,
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

#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct DeleteAccountRequest {
    #[validate(length(min = 8, max = 100))]
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Paginate<T> {
    pub items: Vec<T>,
    pub page: u64,
    pub per_page: u64,
    pub total_items: u64,
    pub total_pages: u64,
}


#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct AnalyticsRequest {
    #[validate(length(min = 1, max = 20))]
    pub code: Option<String>, // URL code, None for user-wide analytics
    #[serde(default)]
    #[validate(range(min = 1))]
    pub page: Option<u64>, // Pagination page
    #[serde(default)]
    #[validate(range(min = 1, max = 100))]
    pub per_page: Option<u64>, // Items per page
    pub filters: Option<AnalyticsFilters>, // Filtering options
}

#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct AnalyticsFilters {
    #[validate(custom(function = "validate_rfc3339_date"))]
    pub start_date: Option<String>, // ISO 8601
    #[validate(custom(function = "validate_rfc3339_date"))]
    pub end_date: Option<String>, // ISO 8601
    #[validate(length(min = 1))]
    pub country: Option<String>, // e.g., "US", "IN"
    #[validate(length(min = 1))]
    pub referrer: Option<String>, // e.g., "example.com"
    #[validate(length(min = 1))]
    pub device_type: Option<String>, // e.g., "desktop", "mobile", "tablet"
    #[validate(length(min = 1))]
    pub browser: Option<String>, // e.g., "Chrome", "Firefox", "Safari"
}

#[derive(Debug, Serialize)]
pub struct AnalyticsResponse {
    pub code: Option<String>, // URL code, None for user-wide analytics
    pub total_clicks: u64, // Total clicks for the URL or all user URLs
    pub unique_visitors: u64, // Unique IPs
    pub daily_clicks: HashMap<String, u64>, // Date (YYYY-MM-DD) -> clicks
    pub referrers: HashMap<String, u64>, // Referrer -> clicks
    pub countries: HashMap<String, u64>, // Country -> clicks
    pub device_types: HashMap<String, u64>, // Device type -> clicks
    pub browsers: HashMap<String, u64>, // Browser -> clicks
    pub total_urls: u64, // Total URLs created by the user
    pub total_system_urls: Option<u64>, // Admin-only: total URLs in system
    pub total_users: Option<u64>, // Admin-only: total registered users
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Paginate<T> {
    pub items: Vec<T>,
    pub page: u64,
    pub per_page: u64,
    pub total_items: u64,
    pub total_pages: u64,
}