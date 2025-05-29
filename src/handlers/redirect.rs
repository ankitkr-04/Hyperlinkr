use axum::{extract::Path, response::Redirect, Extension};
use std::sync::Arc;
use crate::{services::cache::cache::CacheService, errors::AppError, services::analytics::AnalyticsService};
use tracing::info;

pub async fn redirect_handler(
    Path(code): Path<String>,
    Extension(cache): Extension<Arc<CacheService>>,
    Extension(analytics): Extension<Arc<AnalyticsService>>,
) -> Result<Redirect, AppError> {
    let url = cache.get(&code).await?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| AppError::Internal(e.to_string()))?
        .as_secs();
    analytics.record_click(code, timestamp).await;
    info!("Redirecting code {} to {}", code, url);
    Ok(Redirect::to(&url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::{Request, StatusCode}, body::Body, routing::get, Router};
    use tower::ServiceExt;
    use std::sync::Arc;
    use crate::{services::{cache::cache::CacheService, analytics::AnalyticsService, cache::circuit_breaker::CircuitBreaker}, config::settings::Settings};

    #[tokio::test]
    async fn test_redirect_handler() {
        let config = Arc::new(Settings::default());
        let cache = Arc::new(CacheService::new(&config).await);
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            config.database_urls.clone(),
            config.cache.max_failures,
            std::time::Duration::from_secs(config.cache.retry_interval_secs),
        ));
        let analytics = Arc::new(AnalyticsService::new(&config, circuit_breaker).await);
        cache.insert("test".to_string(), "https://example.com".to_string()).await.unwrap();

        let app = Router::new()
            .route("/redirect/:code", get(redirect_handler))
            .layer(Extension(Arc::clone(&cache)))
            .layer(Extension(Arc::clone(&analytics)));

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
