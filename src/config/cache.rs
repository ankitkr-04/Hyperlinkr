use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CacheConfig {
    pub l1_capacity: usize,
    pub l2_capacity: usize,
    pub bloom_bits: usize,    // Total bits for Bloom filter
    pub bloom_expected: usize, // Expected number of items
    pub bloom_block_size: usize, // Configurable block size
    pub redis_pool_size: u32, // Size of Redis connection pool
    pub ttl_seconds: u64, // Time-to-live for cache entries
}