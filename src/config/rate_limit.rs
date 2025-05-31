use serde::Deserialize;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct RateLimitConfig {
    #[validate(range(min = 1))]
    pub shorten_requests_per_minute: u32,
    #[validate(range(min = 100))]
    pub redirect_requests_per_minute: u32,

    // Additional fields can be added here as needed
    #[validate(range(min = 1, max = 3600))]
    pub window_size_seconds: Option<u64>, // Optional, defaults to 60 seconds if not set
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            shorten_requests_per_minute: 10,
            redirect_requests_per_minute: 1_000,
            window_size_seconds: Some(60), // Default to 60 seconds
        }
    }
}
