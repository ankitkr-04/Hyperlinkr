use axum::{
    extract::{Json, Path, State},
    Extension,
    response::IntoResponse,
};
use serde_json::json;
use std::sync::Arc;
use tracing::{info, warn};
use validator::Validate;
use crate::{
    clock::{Clock, SystemClock}, config::settings::Settings, errors::AppError, services::{
        analytics::AnalyticsService,
        cache::cache::CacheService,
        codegen::generator::CodeGenerator,
        storage::{dragonfly::DatabaseClient, storage::Storage},
    }, types::{ApiResponse, ShortenRequest, ShortenResponse, UrlData, AuthResponse},
    middleware::RequestContext,
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
pub async fn list_urls_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    // Example: fetch all URL codes from cache/storage
    // Use available cache method for listing URLs (pagination stub: page 1, 100 per page)
    let urls_page = state.cache.list_urls_cache(None, 1, 100).await.map_err(|e| AppError::Internal(e.to_string()))?;
    let urls = urls_page.map(|p| p.items).unwrap_or_default();
    Ok(Json(ApiResponse {
        success: true,
        data: Some(json!({"urls": urls})),
        error: None,
    }))
}

#[axum::debug_handler]
pub async fn shorten_handler(
    State(state): State<AppState>,
    Extension(request_context): Extension<RequestContext>,
    Json(req): Json<ShortenRequest>,
) -> Result<impl IntoResponse, AppError> {
    req.validate().map_err(AppError::Validation)?;

    // Require authentication
    let user_id = request_context.user_id.ok_or_else(|| {
        AppError::Unauthorized("Authentication required for /v1/shorten".into())
    })?;

    let code = match req.custom_alias {
        Some(alias) => alias,
        None => state
            .codegen
            .next()
            .map_err(AppError::CodeGen)?
            .to_string(),
    };

    // Check for existing code
    if state.cache.contains_key(&code) {
        if let Ok(existing_data) = state.cache.get(&code).await {
            let existing_url_data: UrlData = serde_json::from_str(&existing_data)
                .map_err(|e| AppError::Internal(e.to_string()))?;
            if existing_url_data.long_url == req.url && existing_url_data.user_id.as_ref() == Some(&user_id) {
                let short_url = format!("{}/v1/redirect/{}", state.config.base_url, code);
                return Ok(Json(ApiResponse {
                    success: true,
                    data: Some(ShortenResponse {
                        short_url,
                        code,
                        expiration_date: req.expiration_date,
                    }),
                    error: None,
                }));
            } else {
                return Err(AppError::Conflict("Code already in use".into()));
            }
        }
    }

    // Create UrlData
    let url_data = UrlData {
        long_url: req.url.clone(),
        user_id: Some(user_id.clone()),
        created_at: state.clock.now().to_rfc3339(),
        expires_at: req.expiration_date.clone(),
    };

    // Store UrlData as JSON
    let url_data_json = serde_json::to_string(&url_data)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    state.cache.insert(code.clone(), url_data_json).await?;

    let short_url = format!("{}/v1/redirect/{}", state.config.base_url, code);
    info!("Shortened URL: {} -> {} for user {}", req.url, short_url, user_id);

    Ok(Json(ApiResponse {
        success: true,
        data: Some(ShortenResponse {
            short_url,
            code,
            expiration_date: req.expiration_date,
        }),
        error: None,
    }))
}

#[axum::debug_handler]
pub async fn delete_shorten_handler(
    State(state): State<AppState>,
    Extension(request_context): Extension<RequestContext>,
    Path(code): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = request_context.user_id.ok_or_else(|| {
        AppError::Unauthorized("Authentication required for /v1/shorten/:code deletion".into())
    })?;

    // Fetch URL data to verify ownership
    let url_data_json = state
        .cache
        .get(&code)
        .await
        .map_err(|_| AppError::NotFound("URL not found".into()))?;
    let url_data: UrlData = serde_json::from_str(&url_data_json)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Check ownership
    match url_data.user_id {
        Some(url_user_id) if url_user_id == user_id => {
            // Delete URL
            state
                .rl_db
                .delete_url(&code, Some(&user_id), "")
                .await?;
            info!("Deleted URL code {} for user {}", code, user_id);
            Ok(Json(ApiResponse {
                success: true,
                data: Some(AuthResponse {
                    token: String::new(),
                    user_id: user_id.clone(),
                    is_admin: false,
                }),
                error: None,
            }))
        }
        Some(_) => {
            warn!("User {} attempted to delete URL code {} owned by another user", user_id, code);
            Err(AppError::Forbidden("You do not own this URL".into()))
        }
        None => {
            warn!("URL code {} has no owner, deletion by user {} denied", code, user_id);
            Err(AppError::Forbidden("URL has no owner".into()))
        }
    }
}