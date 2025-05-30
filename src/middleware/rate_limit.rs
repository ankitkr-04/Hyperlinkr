use axum::{middleware::Next, response::Response, http::Request, extract::State};
use crate::{errors::AppError, services::storage::storage::Storage};
use prometheus::IntCounter;
use tracing::warn;
use once_cell::sync::Lazy;
use crate::handlers::shorten::AppState;

static RATE_LIMIT_EXCEEDED: Lazy<IntCounter> = Lazy::new(|| {
    prometheus::register_int_counter!(
        "rate_limit_exceeded_total",
        "Total number of requests exceeding rate limit"
    ).unwrap()
});

pub async fn rate_limit_middleware(
    State(state): State<AppState>,
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
        state.config.rate_limit.shorten_requests_per_minute
    } else {
        state.config.rate_limit.redirect_requests_per_minute
    };

    let key = format!("rate:{}:{}:{}", endpoint, user_id, ip);
    let window: i64 = 60;

    if !state.rl_db.rate_limit(&key, limit as u64, window).await? {
        RATE_LIMIT_EXCEEDED.inc();
        warn!("Rate limit exceeded for {} on {}", user_id, endpoint);
        return Err(AppError::RateLimitExceeded);
    }

    Ok(next.run(req).await)
}


#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::{Request, StatusCode}, routing::get, Router};
    use tower::ServiceExt;
    use std::sync::Arc;
    use crate::{config::settings::Settings, services::cache::circuit_breaker::CircuitBreaker, clock::SystemClock, services::analytics::AnalyticsService, services::codegen::generator::CodeGenerator};

    #[tokio::test]
    async fn test_rate_limit_middleware() {
        let config = Arc::new(Settings::default());
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            config.database_urls.clone(),
            config.cache.max_failures,
            std::time::Duration::from_secs(config.cache.retry_interval_secs),
        ));
        let db = Arc::new(DatabaseClient::new(&config, circuit_breaker.clone()).await.unwrap());
        let analytics = Arc::new(AnalyticsService::new(&config, Arc::clone(&circuit_breaker)).await);
        let codegen = Arc::new(CodeGenerator::new(&config));
        let clock = Arc::new(SystemClock);

        let state = AppState {
            config: Arc::clone(&config),
            cache: Arc::new(CacheService::new(&config).await),
            analytics: Arc::clone(&analytics),
            codegen: Arc::clone(&codegen),
            clock: Arc::clone(&clock),
            rl_db: Arc::clone(&db),
        };

        let app = Router::new()
            .route("/shorten", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(rate_limit_middleware))
            .with_state(state);

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
        let db = Arc::new(DatabaseClient::new(&config, circuit_breaker.clone()).await.unwrap());
        let analytics = Arc::new(AnalyticsService::new(&config, Arc::clone(&circuit_breaker)).await);
        let codegen = Arc::new(CodeGenerator::new(&config));
        let clock = Arc::new(SystemClock);

        let state = AppState {
            config: Arc::clone(&config),
            cache: Arc::new(CacheService::new(&config).await),
            analytics: Arc::clone(&analytics),
            codegen: Arc::clone(&codegen),
            clock: Arc::clone(&clock),
            rl_db: Arc::clone(&db),
        };

        let app = Router::new()
            .route("/shorten", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(rate_limit_middleware))
            .with_state(state);

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