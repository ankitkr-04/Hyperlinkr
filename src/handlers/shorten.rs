use axum::{extract::{Json, Extension}, response::IntoResponse, routing::post, Router};
use validator::Validate;
use std::sync::Arc;
use crate::{services::{codegen::generator::CodeGenerator, cache::CacheService, analytics::AnalyticsService}, types::{ShortenRequest, ShortenResponse}, errors::AppError, config::settings::Settings};
use tracing::info;
use redis::AsyncCommands;

pub async fn shorten_handler(
    Json(mut req): Json<ShortenRequest>,
    Extension(config): Extension<Arc<Settings>>,
    Extension(cache): Extension<Arc<CacheService>>,
    Extension(analytics): Extension<Arc<AnalyticsService>>,
    Extension(codegen): Extension<Arc<CodeGenerator>>,
) -> Result<impl IntoResponse, AppError> {
    req.validate().map_err(AppError::Validation)?;

    // Generate short code
    let code = match req.custom_alias {
        Some(alias) => alias,
        None => codegen.next().map_err(|e| AppError::CodeGen(e))?.to_string(),
    };

    // Check expiration
    if let Some(expiration) = req.expiration_date {
        if chrono::Utc::now() > expiration {
            return Err(AppError::Expired);
        }
    }

    // Store in cache and database
    cache.l1.insert(code.clone(), req.url.clone());
    cache.l2.insert(code.clone(), req.url.clone()).await;
    cache.bloom.read().insert(&code);
    let mut conn = cache.db_pool.get().await.map_err(|_| AppError::RedisConnection)?;
    conn.set_ex(&code, &req.url, config.cache.ttl_seconds)
        .await
        .map_err(|e| AppError::RedisOperation(e.to_string()))?;

    // Record analytics
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| AppError::Internal(e.to_string()))?
        .as_secs();
    analytics.record_click(code.clone(), timestamp);

    // Construct response
    let short_url = format!("{}/redirect/{}", config.base_url, code);
    info!("Shortened URL: {} -> {}", req.url, short_url);
    Ok(Json(ShortenResponse {
        short_url,
        code,
        expiration_date: req.expiration_date,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Request, StatusCode, header};
    use axum::body::Body;
    use tower::ServiceExt;
    use std::sync::Arc;
    use crate::types::ShortenRequest;

    #[tokio::test]
    async fn test_shorten_handler() {
        let config = Arc::new(Settings::default());
        let cache = Arc::new(CacheService::new(&config).await.unwrap());
        let analytics = Arc::new(AnalyticsService::new(config.analytics.clone()));
        let codegen = Arc::new(CodeGenerator::new(&config));

        let app = Router::new()
            .route("/shorten", post(shorten_handler))
            .layer(Extension(config))
            .layer(Extension(cache))
            .layer(Extension(analytics))
            .layer(Extension(codegen));

        let request = ShortenRequest {
            url: "https://example.com".to_string(),
            custom_alias: None,
            expiration_date: None,
        };

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/shorten")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_string(&request).unwrap()))
                    .unwrap()
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}


