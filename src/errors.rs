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

    #[error("GeoIP lookup error: {0}")]
    GeoLookup(#[from] maxminddb::MaxMindDbError),

    #[error("Analytics error: {0}")]
    Analytics(String),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Rate limit exceeded with response")]
    RateLimitExceededWithResponse(Response),

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

    #[error("Conflict in associated resource")]
    Conflict(String),

    #[error("Forbidden access")]
    Forbidden(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::RateLimitExceededWithResponse(resp) => resp,
            AppError::Validation(err) => (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
            AppError::CodeGen(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
            AppError::Cache(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
            AppError::RedisConnection(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg).into_response(),
            AppError::RedisOperation(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
            AppError::CircuitBreaker(node) => (StatusCode::SERVICE_UNAVAILABLE, format!("Circuit breaker open for node: {}", node)).into_response(),
            AppError::Sled(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
            AppError::GeoLookup(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
            AppError::Analytics(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
            AppError::RateLimitExceeded => (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded".to_string()).into_response(),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg).into_response(),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
            AppError::Expired => (StatusCode::GONE, "URL expired".to_string()).into_response(),
            AppError::DuplicateAlias(alias) => (StatusCode::CONFLICT, format!("Duplicate alias: {}", alias)).into_response(),
            AppError::InvalidUrl(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg).into_response(),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg).into_response(),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg).into_response(),
        }
    }
}