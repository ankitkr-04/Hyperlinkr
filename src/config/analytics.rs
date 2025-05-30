use serde::Deserialize;
use validator::Validate;


#[derive(Debug, Deserialize, Validate)]
pub struct AnalyticsConfig {
    #[validate(range(min = 100))]
    pub flush_interval_ms: u64,
    #[validate(range(min = 1000))]
    pub batch_size: usize,
    #[validate(range(min = 1000))]
    pub max_batch_size_ms: u64,
    #[validate(range(min = 1000))]
    pub max_batch_size: usize,
}

impl Default for AnalyticsConfig {
    fn default() -> Self {
        Self {
            flush_interval_ms: 200,
            batch_size: 10_000,
            max_batch_size_ms: 1_000,
            max_batch_size: 10_000,
        }
    }
}