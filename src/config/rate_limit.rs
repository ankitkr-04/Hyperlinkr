use serde::Deserialize;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct RateLimitConfig {
    #[validate(range(min = 1))]
    pub shorten_requests_per_minute: u32,
    #[validate(range(min = 100))]
    pub redirect_requests_per_minute: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            shorten_requests_per_minute: 10,
            redirect_requests_per_minute: 1_000,
        }
    }
}
