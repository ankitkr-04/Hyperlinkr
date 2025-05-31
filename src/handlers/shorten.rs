use axum::{extract::{Json, State}, response::IntoResponse};
use std::sync::Arc;
use tracing::info;
use validator::Validate;
use crate::{
    config::settings::Settings,
    errors::AppError,
    services::{
        analytics::AnalyticsService,
        cache::cache::CacheService,
        codegen::generator::CodeGenerator,
        storage::dragonfly::DatabaseClient,
    },
    types::{ShortenRequest, ShortenResponse},
    clock::SystemClock,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Settings>,
    pub cache: Arc<CacheService>,
    pub analytics: Arc<AnalyticsService>,
    pub codegen: Arc<CodeGenerator>,
    pub clock: Arc<SystemClock>,
    pub rl_db: Arc<DatabaseClient>,
}

#[axum::debug_handler]
pub async fn shorten_handler(
    State(state): State<AppState>,
    Json(req): Json<ShortenRequest>,
) -> Result<impl IntoResponse, AppError> {
    req.validate().map_err(AppError::Validation)?;

    let code = match req.custom_alias {
        Some(alias) => alias,
        None => state
            .codegen
            .next()
            .map_err(AppError::CodeGen)?
            .to_string(),
    };

    // No need to parse expiration_date; validation in types.rs ensures it's valid RFC 3339 and in the future
    if state.cache.contains_key(&code) {
        if let Ok(existing_url) = state.cache.get(&code).await {
            if existing_url == req.url {
                let short_url = format!("{}/redirect/{}", state.config.base_url, code);
                return Ok(Json(ShortenResponse {
                    short_url,
                    code,
                    expiration_date: req.expiration_date,
                }));
            }
        }
    }

    state.cache.insert(code.clone(), req.url.clone()).await?;

    let short_url = format!("{}/redirect/{}", state.config.base_url, code);
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
    use axum::{
        body::Body,
        http::{header, Request, StatusCode},
        routing::post,
        Router,
    };
    use std::sync::Arc;
    use tower::ServiceExt;
    use chrono::{Duration, Utc};

    use crate::{
        clock::SystemClock,
        services::{
            analytics::AnalyticsService,
            cache::{cache::CacheService, circuit_breaker::CircuitBreaker},
            codegen::generator::CodeGenerator,
            storage::dragonfly::DatabaseClient,
        },
        types::ShortenRequest,
    };

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
            .route("/shorten", post(shorten_handler))
            .with_state(state);

        // Test case 1: No expiration_date
        let request_payload = ShortenRequest {
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
                    .body(Body::from(serde_json::to_string(&request_payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let parsed: ShortenResponse = serde_json::from_slice(&body_bytes).unwrap();
        assert!(parsed.short_url.contains("/redirect/"));
        assert_eq!(parsed.expiration_date, None);

        // Test case 2: With valid expiration_date
        let future_date = (Utc::now() + Duration::days(1)).to_rfc3339();
        let request_payload = ShortenRequest {
            url: "https://example.com".to_string(),
            custom_alias: Some("testAlias".to_string()),
            expiration_date: Some(future_date.clone()),
        };

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/shorten")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_string(&request_payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let parsed: ShortenResponse = serde_json::from_slice(&body_bytes).unwrap();
        assert!(parsed.short_url.contains("/redirect/testAlias"));
        assert_eq!(parsed.code, "testAlias");
        assert_eq!(parsed.expiration_date, Some(future_date));
    }
}