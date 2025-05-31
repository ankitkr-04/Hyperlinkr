use serde::Deserialize;
use validator::Validate;



#[derive(Debug, Deserialize, Validate)]
pub struct CacheConfig {
    // Existing fields (unchanged)
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
    pub redis_command_timeout_secs: u64,
    #[validate(range(min = 1))]
    pub redis_max_feed_count: u64,
    #[validate(range(min = 16))]
    pub redis_broadcast_channel_capacity: usize,
    #[validate(range(min = 1))]
    pub redis_max_command_attempts: u32,
    #[validate(range(min = 1000))]
    pub redis_connection_timeout_ms: u64,
    #[validate(range(min = 1))]
    pub redis_reconnect_max_attempts: u32,
    #[validate(range(min = 100))]
    pub redis_reconnect_delay_ms: u64,
    #[validate(range(min = 100))]
    pub redis_reconnect_max_delay_ms: u64,

    // New Sled-specific fields
    #[validate(length(min = 1))]
    pub sled_path: String, // Storage path
    #[validate(range(min = 16_777_216))] // 16MB minimum
    pub sled_cache_bytes: u64,
    #[validate(range(min = 100, max = 60_000))] // 100ms to 60s
    pub sled_flush_ms: u64,
    #[validate(range(min = 1, max = 3600))] // 1s to 1hr
    pub sled_snapshot_ttl_secs: u64,
    pub sled_compression: bool, // Toggle compression
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            // Existing defaults (unchanged)
            l1_capacity: 10_000,
            l2_capacity: 100_000,
            bloom_bits: 1_048_576,
            bloom_expected: 100_000,
            bloom_shards: 8,
            bloom_block_size: 128,
            redis_pool_size: 8,
            ttl_seconds: 3_600,
            max_failures: 5,
            retry_interval_secs: 10,
            redis_command_timeout_secs: 1,
            redis_max_feed_count: 200,
            redis_broadcast_channel_capacity: 32,
            redis_max_command_attempts: 3,
            redis_connection_timeout_ms: 10_000,
            redis_reconnect_max_attempts: 3,
            redis_reconnect_delay_ms: 100,
            redis_reconnect_max_delay_ms: 500,

            // New Sled defaults
            sled_path: "/tmp/sled_hyperlinkr".to_string(),
            sled_cache_bytes: 64 * 1024 * 1024, // 64MB
            sled_flush_ms: 1_000, // 1s
            sled_snapshot_ttl_secs: 5, // 5s
            sled_compression: true,
        }
    }
}