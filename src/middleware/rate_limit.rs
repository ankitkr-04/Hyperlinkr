use axum::{middleware::{from_fn, Next}, response::{IntoResponse, Response}, http::{Request, StatusCode}, Extension, routing::get};
use std::sync::Arc;
use bb8_redis::{redis::{AsyncCommands, pipe}, RedisConnectionManager, bb8::Pool};
use crate::{config::settings::Settings, errors::AppError};
use prometheus::IntCounter;
use tracing::warn;
use lazy_static::lazy_static;

lazy_static! {
    static ref RATE_LIMIT_EXCEEDED: IntCounter = prometheus::register_int_counter!(
        "rate_limit_exceeded_total",
        "Total number of requests exceeding rate limit"
    ).unwrap();
}

pub async fn rate_limit_middleware(
    Extension(config): Extension<Arc<Settings>>,
    Extension(pool): Extension<Pool<RedisConnectionManager>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, AppError> {
    let ip = req.headers()
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
    let user_id = req.headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("anonymous")
        .to_string();

    let endpoint = if req.uri().path().starts_with("/shorten") {
        "shorten"
    } else if req.uri().path().starts_with("/redirect") {
        "redirect"
    } else {
        "other"
    };

    let limit = if endpoint == "shorten" {
        config.rate_limit.shorten_requests_per_minute
    } else {
        config.rate_limit.redirect_requests_per_minute
    };

    let key = format!("rate:{}:{}:{}", endpoint, user_id, ip);
    let mut conn = pool.get().await.map_err(|_| AppError::RedisConnection)?;

    let now = chrono::Utc::now().timestamp(); // i64
    let window: i64 = 60;

    let mut pipeline = pipe();
    pipeline
        .atomic()
        .cmd("ZREMRANGEBYSCORE").arg(&key).arg(0).arg(now - window).ignore()
        .cmd("ZCARD").arg(&key)
        .cmd("ZADD").arg(&key).arg(now).arg(now).ignore()
        .cmd("EXPIRE").arg(&key).arg(window).ignore();

    let (count,): (i64,) = pipeline
        .query_async(&mut conn)
        .await
        .map_err(|e| AppError::RedisOperation(e.to_string()))?;

    if count >= limit as i64 {
        RATE_LIMIT_EXCEEDED.inc();
        warn!("Rate limit exceeded for {} on {}", user_id, endpoint);
        return Err(AppError::RateLimitExceeded);
    }

    Ok(next.run(req).await)
}

#[cfg(test)]
/// Integration tests for the rate limiting middleware.
/// 
/// This module contains tests that verify the correct behavior of the rate limiting middleware
/// in the application. It sets up a test Axum router with the middleware applied, and checks
/// that requests are handled as expected under rate limiting conditions.
/// 
/// # Tests
/// 
/// - `test_rate_limit_middleware`: Ensures that a request to the `/shorten` endpoint with
///   appropriate headers passes through the rate limiting middleware and returns a 200 OK
///   status code.
/// 
/// # Dependencies
/// 
/// - Uses `axum` for HTTP server and routing.
/// - Uses `bb8` for connection pooling.
/// - Uses `tokio` for asynchronous testing.
/// - Relies on application-specific settings and middleware implementations.
/// 
/// # Note
/// 
/// These tests require a working Redis connection and valid configuration settings.
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request};
    use tower::ServiceExt;
    use std::sync::Arc;
    use crate::config::settings::Settings;

    #[tokio::test]
    async fn test_rate_limit_middleware() {
        let config = Arc::new(Settings::default());
        let pool = bb8::Pool::builder()
            .max_size(config.cache.redis_pool_size)
            .build(RedisConnectionManager::new(&config.database_url).unwrap())
            .await
            .unwrap();

        let app = Router::new()
            .route("/shorten", get(|| async { "ok" }))
            .layer(from_fn(rate_limit_middleware))
            .layer(Extension(config))
            .layer(Extension(pool));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/shorten")
                    .header("X-API-Key", "testkey")
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}