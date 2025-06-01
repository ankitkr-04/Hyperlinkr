use axum::{
    extract::{State, Extension},
    http::{header, Request, Response, StatusCode},
    middleware::Next,
};
use once_cell::sync::OnceCell;
use std::collections::HashSet;
use jsonwebtoken::{decode, DecodingKey, Validation};
use tracing::warn;
use crate::{
    clock::Clock,
    errors::AppError,
    handlers::shorten::AppState,
    services::storage::storage::Storage,
    types::AuthToken,
    middleware::RequestContext,
};

static PUBLIC_ENDPOINTS: OnceCell<HashSet<&'static str>> = OnceCell::new();

pub fn init_auth_middleware() {
    PUBLIC_ENDPOINTS.get_or_init(|| {
        HashSet::from([
            "/v1/redirect",
            "/v1/auth/login",
            "/v1/auth/register",
        ])
    });
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response<axum::body::Body>, AppError> {
    let path = req.uri().path();
    if PUBLIC_ENDPOINTS.get().map_or(false, |endpoints| endpoints.contains(path)) {
        return Ok(next.run(req).await);
    }

    let mut context = req
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_default();

    // Extract and validate JWT
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| {
            warn!("Missing Bearer token for {}", path);
            AppError::Unauthorized("Missing Bearer token".into())
        })?;

    // Check blacklist
    if state.rl_db.is_token_blacklisted(token).await? {
        warn!("Blacklisted token used for {}", path);
        return Err(AppError::Unauthorized("Token is blacklisted".into()));
    }

    // Decode JWT
    let token_data = decode::<AuthToken>(
        token,
        &DecodingKey::from_secret(state.config.security.jwt_secret.as_ref()),
        &Validation::default(),
    ).map_err(|e| {
        warn!("Invalid JWT for {}: {}", path, e);
        AppError::Unauthorized("Invalid JWT".into())
    })?;

    let auth_token = token_data.claims;
    if state.clock.now().timestamp() as u64 > auth_token.expires_at.parse::<u64>().unwrap_or(0) {
        warn!("Expired JWT for {}", path);
        return Err(AppError::Unauthorized("Expired JWT".into()));
    }

    // Populate RequestContext
    context.user_id = auth_token.user_id;
    context.email = Some(auth_token.email);
    context.username = Some(auth_token.username);
    context.is_admin = auth_token.is_admin;

    // Inject RequestContext
    req.extensions_mut().insert(context);
    Ok(next.run(req).await)
}