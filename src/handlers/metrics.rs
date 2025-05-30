use axum::{extract::State, response::IntoResponse, http::StatusCode};
use prometheus::Encoder;
use crate::handlers::shorten::AppState;

#[axum::debug_handler]
pub async fn metrics_handler(
    State(_state): State<AppState>,
) -> impl IntoResponse {
    let encoder = prometheus::TextEncoder::new();
    let mut buffer = vec![];
    encoder.encode(&prometheus::gather(), &mut buffer).unwrap();
    (StatusCode::OK, buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::{Request, StatusCode}, routing::get, Router};
    use tower::ServiceExt;
    use std::sync::Arc;
    use crate::{
        config::settings::Settings,
        services::{
            analytics::AnalyticsService,
            cache::{cache::CacheService, circuit_breaker::CircuitBreaker},
            codegen::generator::CodeGenerator,
            storage::dragonfly::DatabaseClient,
        },
        clock::SystemClock,
        handlers::shorten::AppState,
    };

    #[tokio::test]
    async fn test_metrics_handler() {
        let config = Arc::new(Settings::default());
        let cache = Arc::new(CacheService::new(&config).await);
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            config.database_urls.clone(),
            config.cache.max_failures,
            std::time::Duration::from_secs(config.cache.retry_interval_secs),
        ));
        let analytics = Arc::new(AnalyticsService::new(&config, Arc::clone(&circuit_breaker)).await);
        let codegen = Arc::new(CodeGenerator::new(&config));
        let clock = Arc::new(SystemClock);
        let rl_db = Arc::new(DatabaseClient::new(&config, Arc::clone(&circuit_breaker)).await.unwrap());

        let state = AppState {
            config: Arc::clone(&config),
            cache: Arc::clone(&cache),
            analytics: Arc::clone(&analytics),
            codegen: Arc::clone(&codegen),
            clock: Arc::clone(&clock),
            rl_db: Arc::clone(&rl_db),
        };

        let app = Router::new()
            .route("/metrics", get(metrics_handler))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
        assert!(!bytes.is_empty());
    }
}