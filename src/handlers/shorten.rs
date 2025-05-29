
use axum::{extract::{Json, Extension}, response::IntoResponse, routing::post, Router};
use validator::Validate;
use std::sync::Arc;
use crate::{services::{codegen::generator::CodeGenerator, cache::cache::CacheService, analytics::AnalyticsService}, types::{ShortenRequest, ShortenResponse}, errors::AppError, config::settings::Settings};
use tracing::info;
use crate::clock::{SystemClock, Clock};

pub async fn shorten_handler(
    Json(mut req): Json<ShortenRequest>,
    Extension(config): Extension<Arc<Settings>>,
    Extension(cache): Extension<Arc<CacheService>>,
    Extension(analytics): Extension<Arc<AnalyticsService>>,
    Extension(codegen): Extension<Arc<CodeGenerator>>,
    Extension(clock): Extension<Arc<dyn Clock + Send + Sync>>,
) -> Result<impl IntoResponse, AppError> {
    req.validate().map_err(|e| AppError::Validation(e))?;

    let code = match req.custom_alias {
        Some(alias) => alias,
        None => codegen.next().map_err(|e| AppError::CodeGen(e))?.to_string(),
    };

    if let Some(expiration) = req.expiration_date {
        if clock.now() > expiration {
            return Err(AppError::Expired);
        }
    }

    if cache.contains_key(&code) {
        if let Ok(existing_url) = cache.get(&code).await {
            if existing_url == req.url {
                let short_url = format!("{}/redirect/{}", config.base_url, code);
                return Ok(Json(ShortenResponse {
                    short_url,
                    code,
                    expiration_date: req.expiration_date,
                }));
            }
        }
    }

    cache.insert(code.clone(), req.url.clone()).await?;

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
    use axum::{http::{Request, StatusCode, header}, body::Body};
    use tower::ServiceExt;
    use std::sync::Arc;
    use crate::{types::ShortenRequest, services::cache::circuit_breaker::CircuitBreaker};

    #[tokio::test]
    async fn test_shorten_handler() {
        let config = Arc::new(Settings::default());
        let cache = Arc::new(CacheService::new(&config).await);
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            config.database_urls.clone(),
            config.cache.max_failures,
            std::time::Duration::from_secs(config.cache.retry_interval_secs),
        ));
        let analytics = Arc::new(AnalyticsService::new(&config, Arc::clone(&circuit_breaker)).await);
        let codegen = Arc::new(CodeGenerator::new(&config));

        let app = Router::new()
            .route("/shorten", post(shorten_handler))
            .layer(Extension(Arc::clone(&config)))
            .layer(Extension(Arc::clone(&cache)))
            .layer(Extension(Arc::clone(&analytics)))
            .layer(Extension(Arc::clone(&codegen)));

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
