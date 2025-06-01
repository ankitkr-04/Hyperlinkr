use axum::{
    extract::{Extension, Json, Path, State},
    response::{IntoResponse, Redirect},
    routing::{get, post},
    Router,
};
use serde_json::json;
use std::sync::Arc;
use tracing::{info, warn};
use validator::Validate;
use crate::{
    clock::{Clock, SystemClock}, config::settings::Settings, errors::AppError, middleware::rate_limit::{auth_rate_limit_middleware, AuthContext}, services::{
        analytics::AnalyticsService,
        cache::cache::CacheService,
        codegen::generator::CodeGenerator,
        storage::{dragonfly::DatabaseClient, storage::Storage},
    }, types::{ApiResponse, ShortenRequest, ShortenResponse, UrlData}
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
    Extension(auth_context): Extension<AuthContext>,
    Json(req): Json<ShortenRequest>,
) -> Result<impl IntoResponse, AppError> {
    req.validate().map_err(AppError::Validation)?;

    // Require authentication
    let user_id = auth_context.user_id.ok_or_else(|| {
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
        user_id: Some(user_id),
        created_at: state.clock.now().to_rfc3339(),
        expires_at: req.expiration_date,
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
    Extension(auth_context): Extension<AuthContext>,
    Path(code): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = auth_context.user_id.ok_or_else(|| {
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
                .delete_url(&code, Some(&user_id), &auth_context.email.unwrap_or_default())
                .await?;
            info!("Deleted URL code {} for user {}", code, user_id);
            Ok(Json(ApiResponse {
                success: true,
                data: Some(AuthResponse {
                    message: "URL deleted successfully".into(),
                    token: None,
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