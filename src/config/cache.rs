use serde::Deserialize;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct CacheConfig {
    #[validate(range(min = 1000))]
    pub l1_capacity: usize,
    #[validate(range(min = 10000))]
    pub l2_capacity: usize,
    #[validate(range(min = 1048576))]
    pub bloom_bits: usize,
    #[validate(range(min = 1000))]
    pub bloom_expected: usize,
    #[validate(range(min = 8))]
    pub bloom_shards: usize,
    #[validate(range(min = 128))]
    pub bloom_block_size: usize,
    #[validate(range(min = 8))]
    pub redis_pool_size: u32,
    #[validate(range(min = 60))]
    pub ttl_seconds: u64,
    #[validate(range(min = 3))]
    pub max_failures: u32,
    #[validate(range(min = 10))]
    pub retry_interval_secs: u64,
    #[validate(range(min = 1))]
    pub redis_command_timeout_secs: u64, // Adjusted min to 1 for flexibility
    #[validate(range(min = 1))]
    pub redis_max_feed_count: u64,
    #[validate(range(min = 16))]
    pub redis_broadcast_channel_capacity: usize,
    #[validate(range(min = 1))]
    pub redis_max_command_attempts: u32, // Added for connection_config
    #[validate(range(min = 1000))]
    pub redis_connection_timeout_ms: u64, // Added for connection_config
    #[validate(range(min = 1))]
    pub redis_reconnect_max_attempts: u32, // Added for ReconnectPolicy
    #[validate(range(min = 100))]
    pub redis_reconnect_delay_ms: u64, // Added for ReconnectPolicy
    #[validate(range(min = 100))]
    pub redis_reconnect_max_delay_ms: u64, // Added for ReconnectPolicy
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            l1_capacity: 10_000, // Valid: >= 1000
            l2_capacity: 100_000, // Valid: >= 10000
            bloom_bits: 1_048_576, // Valid: >= 1048576
            bloom_expected: 100_000, // Valid: >= 1000
            bloom_shards: 8, // Valid: >= 8
            bloom_block_size: 128, // Valid: >= 128
            redis_pool_size: 8, // Valid: >= 8
            ttl_seconds: 3_600, // Valid: >= 60
            max_failures: 5, // Valid: >= 3
            retry_interval_secs: 10, // Valid: >= 10
            redis_command_timeout_secs: 1, // Valid: >= 1 (adjusted from 100 for practical default)
            redis_max_feed_count: 200, // Valid: >= 1 (matches original hardcoded value)
            redis_broadcast_channel_capacity: 32, // Valid: >= 16 (matches original hardcoded value)
            redis_max_command_attempts: 3, // Valid: >= 1 (matches original hardcoded value)
            redis_connection_timeout_ms: 10_000, // Valid: >= 1000 (matches original hardcoded value)
            redis_reconnect_max_attempts: 3, // Valid: >= 1 (matches original hardcoded value)
            redis_reconnect_delay_ms: 100, // Valid: >= 100 (matches original hardcoded value)
            redis_reconnect_max_delay_ms: 500, // Valid: >= 100 (matches original hardcoded value)
        }
    }
}