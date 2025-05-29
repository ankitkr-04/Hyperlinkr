use axum::{middleware::{from_fn, Next}, response::{IntoResponse, Response}, http::{Request, StatusCode}, Extension};
use std::sync::Arc;
use crate::{config::settings::Settings, errors::AppError, services::storage::{dragonfly::DatabaseClient, storage::Storage}};
use prometheus::IntCounter;
use tracing::warn;
use lazy_static::lazy_static;
use once_cell::sync::Lazy;

static RATE_LIMIT_EXCEEDED: Lazy<IntCounter> = Lazy::new(|| {
    prometheus::register_int_counter!(
        "rate_limit_exceeded_total",
        "Total number of requests exceeding rate limit"
    ).unwrap()
});

pub async fn rate_limit_middleware(
    Extension(config): Extension<Arc<Settings>>,
    Extension(db): Extension<Arc<DatabaseClient>>,
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
    let window: i64 = 60;

    if !db.rate_limit(&key, limit as u64, window).await? {
        RATE_LIMIT_EXCEEDED.inc();
        warn!("Rate limit exceeded for {} on {}", user_id, endpoint);
        return Err(AppError::RateLimitExceeded);
    }

    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, routing::get, Router};
    use tower::ServiceExt;
    use std::sync::Arc;
    use crate::{config::settings::Settings, services::cache::circuit_breaker::CircuitBreaker};
use once_cell::sync::Lazy;

    #[tokio::test]
    async fn test_rate_limit_middleware() {
        let config = Arc::new(Settings::default());
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            config.database_urls.clone(),
            config.cache.max_failures,
            std::time::Duration::from_secs(config.cache.retry_interval_secs),
        ));
        let db = Arc::new(DatabaseClient::new(&config, circuit_breaker).await.unwrap());

        let app = Router::new()
            .route("/shorten", get(|| async { "ok" }))
            .layer(from_fn(rate_limit_middleware))
            .layer(Extension(Arc::clone(&config)))
            .layer(Extension(Arc::clone(&db)));

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

    #[tokio::test]
    async fn test_rate_limit_exceeded() {
        let mut config = Settings::default();
        config.rate_limit.shorten_requests_per_minute = 1;
        let config = Arc::new(config);
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            config.database_urls.clone(),
            config.cache.max_failures,
            std::time::Duration::from_secs(config.cache.retry_interval_secs),
        ));
        let db = Arc::new(DatabaseClient::new(&config, circuit_breaker).await.unwrap());

        let app = Router::new()
            .route("/shorten", get(|| async { "ok" }))
            .layer(from_fn(rate_limit_middleware))
            .layer(Extension(Arc::clone(&config)))
            .layer(Extension(Arc::clone(&db)));

        // First request should pass
        let response1 = app.clone()
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
        assert_eq!(response1.status(), StatusCode::OK);

        // Second request should fail (rate limit exceeded)
        let response2 = app
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
        assert_eq!(response2.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}