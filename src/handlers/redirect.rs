use axum::{extract::Path, response::Redirect, Extension};
use std::sync::Arc;
use crate::{services::cache::CacheService, errors::AppError, services::analytics::AnalyticsService};
use tracing::info;

pub async fn redirect_handler(
    Path(code): Path<String>,
    Extension(cache): Extension<Arc<CacheService>>,
    Extension(analytics): Extension<Arc<AnalyticsService>,
) -> Result<Redirect, AppError> {
    let url = cache.get(&code).await?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| AppError::Internal(e.to_string()))?
        .as_secs();
    analytics.record_click(code, timestamp);
    info!("Redirecting code {} to {}", code, url);
    Ok(Redirect::to(&url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use axum::body::Body;
    use tower::ServiceExt;
    use std::sync::Arc;
    use crate::services::{cache::CacheService, analytics::AnalyticsService};
    use crate::config::settings::Settings;

    #[tokio::test]
    async fn test_redirect_handler() {
        let config = Arc::new(Settings::default());
        let cache = Arc::new(CacheService::new(&config).await);
        let analytics = Arc::new(AnalyticsService::new(config.analytics.clone()));
        cache.l1.insert("test".to_string(), "https://example.com".to_string());
        let app = redirect_handler
            .layer(Extension(cache))
            .layer(Extension(analytics));
        let response = app
            .oneshot(Request::builder().uri("/redirect/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::FOUND);
    }
}