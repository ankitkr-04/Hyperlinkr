use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;
use validator::ValidationErrors;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Validation failed: {0}")]
    Validation(#[from] ValidationErrors),

    #[error("Code generation failed: {0}")]
    CodeGen(#[from] crate::services::codegen::generator::CodeGenError),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("Redis connection failed")]
    RedisConnection(String),

    #[error("Redis operation failed: {0}")]
    RedisOperation(String),

    #[error("Circuit breaker open for node: {0}")]
    CircuitBreaker(String),

    #[error("Sled storage error: {0}")]
    Sled(#[from] sled::Error),

    #[error("Analytics error: {0}")]
    Analytics(String),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Internal server error: {0}")]
    Internal(String),

    #[error("URL expired")]
    Expired,

    #[error("Duplicate alias: {0}")]
    DuplicateAlias(String),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Unauthorized access")]
    Unauthorized(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Validation(err) => (StatusCode::BAD_REQUEST, err.to_string()),
            AppError::CodeGen(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
            AppError::Cache(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.to_string()),
            AppError::RedisConnection(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg.to_string()),
            AppError::RedisOperation(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.to_string()),
            AppError::CircuitBreaker(node) => (StatusCode::SERVICE_UNAVAILABLE, format!("Circuit breaker open for node: {}", node)),
            AppError::Sled(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
            AppError::Analytics(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.to_string()),
            AppError::RateLimitExceeded => (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded".to_string()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.to_string()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.to_string()),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.to_string()),
            AppError::Expired => (StatusCode::GONE, "URL expired".to_string()),
            AppError::DuplicateAlias(alias) => (StatusCode::CONFLICT, format!("Duplicate alias: {}", alias)),
            AppError::InvalidUrl(msg) => (StatusCode::BAD_REQUEST, msg.to_string()),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.to_string()),
        };
        (status, message).into_response()
    }
}