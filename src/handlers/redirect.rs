use axum::{extract::{Path, State}, response::Redirect};
use crate::{errors::AppError, handlers::shorten::AppState};
use tracing::info;

#[axum::debug_handler]
pub async fn redirect_handler(
    Path(code): Path<String>,
    State(state): State<AppState>,
) -> Result<Redirect, AppError> {
    let url = state.cache.get(&code).await?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| AppError::Internal(e.to_string()))?
        .as_secs();
    state.analytics.record_click(&code, timestamp).await;
    info!("Redirecting code {} to {}", code, url);
    Ok(Redirect::to(&url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::{Request, StatusCode}, body::Body, routing::get, Router};
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
    async fn test_redirect_handler() {
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

        cache.insert("test".to_string(), "https://example.com".to_string()).await.unwrap();

        let state = AppState {
            config: Arc::clone(&config),
            cache: Arc::clone(&cache),
            analytics: Arc::clone(&analytics),
            codegen: Arc::clone(&codegen),
            clock: Arc::clone(&clock),
            rl_db: Arc::clone(&rl_db),
        };

        let app = Router::new()
            .route("/redirect/:code", get(redirect_handler))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/redirect/test")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FOUND);
    }
}