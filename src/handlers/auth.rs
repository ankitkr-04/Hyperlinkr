use axum::{
    extract::{Json, State}, http::HeaderMap, response::IntoResponse, routing::post, Router
};
use bcrypt::{hash, verify, DEFAULT_COST};
use chrono::Duration;
use jsonwebtoken::{encode, EncodingKey, Header};
use std::sync::Arc;
use tracing::{info, warn};
use cuid::cuid2;
use validator::Validate;

use crate::{
    clock::{Clock, SystemClock}, config::settings::Settings, errors::AppError, services::{
        analytics::AnalyticsService,
        cache::cache::CacheService,
        codegen::generator::CodeGenerator,
        storage::{dragonfly::DatabaseClient, storage::Storage},
    }, types::{ApiResponse, AuthAction, AuthResponse, AuthToken, User, AuthRequest, DeleteAccountRequest}
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



pub fn routes(state: AppState) -> Router {
    Router::new()
        .route("/v1/auth/register", post(register_handler))
        .route("/v1/auth/login", post(login_handler))
        .route("/v1/auth/logout", post(logout_handler))
        .route("/v1/auth/delete-account", post(delete_account_handler))
    // .layer(axum::middleware::from_fn_with_state(state.clone(), auth_rate_limit_middleware))
        .with_state(state)
}

#[axum::debug_handler]
pub async fn register_handler(
    State(state): State<AppState>,
    Json(req): Json<AuthRequest>,
) -> Result<impl IntoResponse, AppError> {
    req.validate().map_err(AppError::Validation)?;
    if req.action != AuthAction::Register {
        return Err(AppError::BadRequest("Invalid action for register".into()));
    }

    // Check if username or email exists
    if let Some(email) = &req.email {
        if state.rl_db.get_user(email).await?.is_some() {
            return Err(AppError::Conflict("Email already registered".into()));
        }
    }
    if state.rl_db.get_user(&req.username).await?.is_some() {
        return Err(AppError::Conflict("Username already taken".into()));
    }

    // Hash password
    let password_hash = hash(&req.password, DEFAULT_COST)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Create user
    let user_id = cuid2();
    let user = User {
        id: user_id.clone(),
        username: req.username,
        email: req.email.unwrap_or_default(),
        password_hash,
        created_at: state.clock.now().to_rfc3339(),
    };
    state.rl_db.set_user(&user).await?;

    // Generate JWT
    let expires_at = state.clock.now() + Duration::hours(24);
    let is_admin = if !user.email.is_empty() {
        state.rl_db.is_global_admin(&user.email).await?
    } else {
        false
    };
    let claims = AuthToken {
        user_id: Some(user_id),
        expires_at: expires_at.to_rfc3339(),
        username: user.username.clone(),
        email: user.email.clone(),
    is_admin,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.config.security.jwt_secret.as_ref()),
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    info!("Registered user: {}", user.id);
        Ok(Json(ApiResponse {
            success: true,
            data: Some(AuthResponse {
                token,
                user_id: user.id.clone(),
                is_admin,
            }),
            error: None,
        }))
}

#[axum::debug_handler]
pub async fn login_handler(
    State(state): State<AppState>,
    Json(req): Json<AuthRequest>,
) -> Result<impl IntoResponse, AppError> {
    req.validate().map_err(AppError::Validation)?;
    if req.action != AuthAction::Login {
        return Err(AppError::BadRequest("Invalid action for login".into()));
    }

    // Find user by username or email
    let user = state
        .rl_db
        .get_user(req.email.as_ref().unwrap_or(&req.username))
        .await?
        .ok_or_else(|| {
            warn!("Login failed: User not found");
            AppError::Unauthorized("Invalid credentials".into())
        })?;

    // Verify password
    if !verify(&req.password, &user.password_hash)
        .map_err(|e| AppError::Internal(e.to_string()))?
    {
        warn!("Login failed: Invalid password for {}", user.id);
        return Err(AppError::Unauthorized("Invalid credentials".into()));
    }

    // Generate JWT
    let expires_at = state.clock.now() + Duration::hours(24);
    let is_admin = state.rl_db.is_global_admin(&user.email).await?;
    let claims = AuthToken {
        user_id: Some(user.id.clone()),
        expires_at: expires_at.to_rfc3339(),
        username: user.username.clone(),
        email: user.email.clone(),
    is_admin: false,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.config.security.jwt_secret.as_ref()),
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    info!("User logged in: {}", user.id);
        Ok(Json(ApiResponse {
            success: true,
            data: Some(AuthResponse {
                token,
                user_id: user.id.clone(),
                is_admin,
            }),
            error: None,
        }))
}

#[axum::debug_handler]
pub async fn logout_handler(
    State(state): State<AppState>,
    // Extension(auth_context): Extension<AuthContext>,
    req: axum::http::Request<axum::body::Body>,
) -> Result<impl IntoResponse, AppError> {
    // Extract JWT from headers
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::Unauthorized("Missing Bearer token".into()))?;

    // Blacklist token
    let ttl_secs = state.config.security.token_expiry_secs;
    state.rl_db.blacklist_token(token, ttl_secs).await?;

    info!("User logged out");
    Ok(Json(ApiResponse {
        success: true,
        data: Some(AuthResponse {
            token: String::new(),
            user_id: String::new(),
            is_admin: false,
        }),
        error: None,
    }))
}

#[axum::debug_handler]
pub async fn delete_account_handler(
    State(state): State<AppState>,
    // Extension(auth_context): Extension<AuthContext>,
    headers: HeaderMap,
    Json(req): Json<DeleteAccountRequest>,
) -> Result<impl IntoResponse, AppError> {
    req.validate().map_err(AppError::Validation)?;

    // Extract user_id from request (if available)
    let user_id = req.user_id.clone().to_string();

    // Find user by user_id
    let user = state
        .rl_db
        .get_user(&user_id)
        .await?
        .ok_or_else(|| {
            warn!("Delete account failed: User not found: {}", user_id);
            AppError::Unauthorized("User not found".into())
        })?;

    if !verify(&req.password, &user.password_hash)
        .map_err(|e| AppError::Internal(e.to_string()))?
    {
        warn!("Delete account failed: Invalid password for {}", user_id);
        return Err(AppError::Unauthorized("Invalid password".into()));
    }

    // Delete user URLs
    let pattern = format!("url:*:{}", user_id);
    let keys = state.rl_db.scan_keys(&pattern, 1000).await?;
    for key in keys {
        state.rl_db.delete_url(&key, Some(&user_id), &user.email).await?;
    }

    // Delete user data
    let user_key = format!("user:{}", user_id);
    state.rl_db.delete_url(&user_key, None, "").await?;

    // ✅ Extract and blacklist token
    let token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::Unauthorized("Missing Bearer token".into()))?;

    let ttl_secs = state.config.security.token_expiry_secs;
    state.rl_db.blacklist_token(token, ttl_secs).await?;

    info!("User account deleted: {}", user_id);

    Ok(Json(ApiResponse {
        success: true,
        data: Some(AuthResponse {
            token: String::new(),
            user_id,
            is_admin: false,
        }),
        error: None,
    }))
}