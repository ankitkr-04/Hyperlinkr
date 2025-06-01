use axum::{
    extract::{State, Extension},
    http::{header, Request, Response, StatusCode},
    middleware::Next,
};
use once_cell::sync::OnceCell;
use prometheus::IntCounter;
use tracing::warn;
use crate::{
    clock::Clock,
    errors::AppError,
    handlers::shorten::AppState,
    middleware::RequestContext, services::storage::storage::Storage,
};

static RATE_LIMIT_EXCEEDED: OnceCell<IntCounter> = OnceCell::new();

pub fn init_rate_limit_middleware() {
    RATE_LIMIT_EXCEEDED.get_or_init(|| {
        prometheus::register_int_counter!(
            "rate_limit_exceeded_total",
            "Total number of requests exceeding rate limit"
        ).unwrap()
    });
}

async fn check_rate_limit(
    key: String,
    limit: u64,
    window: i64,
    state: &AppState,
) -> Result<bool, AppError> {
    if state.config.cache.use_sled {
        state.rl_db.rate_limit(&key, limit, window).await
    } else {
        let lua_script = r#"
            local key = KEYS[1]
            local limit = tonumber(ARGV[1])
            local window = tonumber(ARGV[2])
            local now = tonumber(ARGV[3])
            local count = redis.call('GET', key) or 0
            count = tonumber(count)
            if count >= limit then
                local ttl = redis.call('TTL', key)
                if ttl > 0 then
                    return 0
                end
            end
            if count == 0 then
                redis.call('SET', key, 1, 'EX', window)
            else
                redis.call('INCR', key)
            end
            return 1
        "#;
        let now = state.clock.now().timestamp() as u64;
        let result: i64 = state.rl_db.eval_lua(
            lua_script,
            vec![key],
            vec![limit.to_string(), window.to_string(), now.to_string()],
        ).await?;
        Ok(result == 1)
    }
}

fn get_endpoint(path: &str) -> &'static str {
    if path.starts_with("/v1/shorten") {
        "shorten"
    } else if path.starts_with("/v1/redirect") {
        "redirect"
    } else {
        "other"
    }
}

fn build_rate_limit_response(window: i64) -> Result<Response<axum::body::Body>, AppError> {
    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header(header::RETRY_AFTER, window.to_string())
        .body(axum::body::Body::empty())
        .map_err(|e| AppError::Internal(e.to_string()))
}

pub async fn rate_limit_middleware(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response<axum::body::Body>, AppError> {
    let path = req.uri().path();
    let endpoint = get_endpoint(path);

    let ip = context.ip.as_deref().unwrap_or("unknown");
    let ip_limit = if endpoint == "shorten" {
        state.config.rate_limit.shorten_requests_per_minute
    } else {
        state.config.rate_limit.redirect_requests_per_minute
    };
    let window = state.config.rate_limit.window_size_seconds.unwrap_or(60) as i64;
    let ip_key = format!("rate:{}:ip:{}", endpoint, ip);

    let ip_allowed = check_rate_limit(ip_key, ip_limit as u64, window, &state).await?;

    if !ip_allowed {
        RATE_LIMIT_EXCEEDED.get().unwrap().inc();
        warn!("IP rate limit exceeded for {} on {}", ip, endpoint);
        let response = build_rate_limit_response(window)?;
        return Err(AppError::RateLimitExceededWithResponse(response));
    }

    if let Some(user_id) = &context.user_id {
        let user_key = format!("rate:{}:user:{}", endpoint, user_id);
        let user_allowed = check_rate_limit(user_key, ip_limit as u64, window, &state).await?;

        if !user_allowed {
            RATE_LIMIT_EXCEEDED.get().unwrap().inc();
            warn!("User rate limit exceeded for {} on {}", user_id, endpoint);
            let response = build_rate_limit_response(window)?;
            return Err(AppError::RateLimitExceededWithResponse(response));
        }
    }

    Ok(next.run(req).await)
}
