use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct RateLimitConfig {
    pub shorten_requests_per_minute: u32,
    pub redirect_requests_per_minute: u32,
}