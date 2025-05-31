use axum::{
    extract::{ConnectInfo, State},
    http::{header, Request, Response, StatusCode},
    middleware::Next,
};
use crate::{
    errors::AppError,
    handlers::shorten::AppState,
    services::storage::storage::Storage,
};
use once_cell::sync::Lazy;
use prometheus::IntCounter;
use std::net::SocketAddr;
use tracing::warn;

static RATE_LIMIT_EXCEEDED: Lazy<IntCounter> = Lazy::new(|| {
    prometheus::register_int_counter!(
        "rate_limit_exceeded_total",
        "Total number of requests exceeding rate limit"
    )
    .unwrap()
});

pub async fn rate_limit_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response<axum::body::Body>, AppError>
{
    let ip = addr.ip().to_string();
    let user_id = req
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("anonymous")
        .to_string();

    let endpoint = if req.uri().path().starts_with("/shorten") {
        "shorten"
    } else if req.uri().path().starts_with("/redirect") {
        "redirect"
    } else {
        "other"
    };

    let limit = if endpoint == "shorten" {
        state.config.rate_limit.shorten_requests_per_minute
    } else {
        state.config.rate_limit.redirect_requests_per_minute
    };

    let window = state.config.rate_limit.window_size_seconds.unwrap_or(60) as i64;
    let key = format!("rate:{}:{}:{}", endpoint, user_id, ip);

    if !state.rl_db.rate_limit(&key, limit as u64, window).await? {
        RATE_LIMIT_EXCEEDED.inc();
        warn!("Rate limit exceeded for {} on {}", user_id, endpoint);
        let response = Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header(header::RETRY_AFTER, window.to_string())
            .body(axum::body::Body::empty())
            .map_err(|e| AppError::Internal(e.to_string()))?;
        return Err(AppError::RateLimitExceededWithResponse(response));
    }

    Ok(next.run(req).await)
}