use axum::{
    extract::{State, Extension},
    http::{Request, Response, StatusCode},
    middleware::Next,
};
use crate::{
    errors::AppError,
    handlers::shorten::AppState,
    types::User,
    services::storage::storage::Storage,
};
use once_cell::sync::Lazy;
use prometheus::IntCounter;
use tracing::warn;

static AUTH_FAILURES: Lazy<IntCounter> = Lazy::new(|| {
    prometheus::register_int_counter!(
        "auth_failures_total",
        "Total number of failed authentication attempts"
    )
    .unwrap()
});

#[derive(Clone)]
pub struct AuthContext {
    pub user_id: Option<String>,
    pub email: Option<String>,
}

use axum::body::Body;

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response<Body>, AppError> 
{
    let path = req.uri().path();

    // Bypass auth for public endpoints
    if path.starts_with("/v1/shorten") || path.starts_with("/v1/redirect") {
        req.extensions_mut().insert(AuthContext {
            user_id: None,
            email: None,
        });
        return Ok(next.run(req).await);
    }

    // Extract API key or Bearer token
    let auth_token = req
        .headers()
        .get("X-API-Key")
        .cloned()
        .or_else(|| {
            req.headers().get("Authorization").and_then(|v| {
                v.to_str()
                    .ok()
                    .and_then(|s| s.strip_prefix("Bearer "))
                    .map(|token| http::HeaderValue::from_str(token).ok())
                    .flatten()
            })
        })
        .ok_or_else(|| {
            AUTH_FAILURES.inc();
            warn!("Missing authentication token for {}", path);
            AppError::Unauthorized("Missing API key or Bearer token".into())
        })?;

    // Check if token is blacklisted
    let token_str = auth_token
        .to_str()
        .map_err(|_| {
            AUTH_FAILURES.inc();
            warn!("Invalid token format for {}", path);
            AppError::Unauthorized("Invalid token format".into())
        })?;
    if state.rl_db.is_token_blacklisted(token_str).await? {
        AUTH_FAILURES.inc();
        warn!("Blacklisted token used for {}", path);
        return Err(AppError::Unauthorized("Token is blacklisted".into()));
    }

    // Fetch user by API key (assuming API key is user ID or email)
    let token_str = auth_token
        .to_str()
        .map_err(|_| {
            AUTH_FAILURES.inc();
            warn!("Invalid token format for {}", path);
            AppError::Unauthorized("Invalid token format".into())
        })?;
    let user = state.rl_db.get_user(token_str).await?.ok_or_else(|| {
        AUTH_FAILURES.inc();
        warn!("Invalid user for token on {}", path);
        AppError::Unauthorized("Invalid API key or token".into())
    })?;

    // Inject user context
    let auth_context = AuthContext {
        user_id: Some(user.id),
        email: Some(user.email),
    };
    req.extensions_mut().insert(auth_context);

    Ok(next.run(req).await)
}