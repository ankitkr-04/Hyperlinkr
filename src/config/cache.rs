use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CacheConfig {
    pub l1_capacity: usize,
    pub bloom_bits: usize,    // Total bits for Bloom filter
    pub bloom_expected: usize, // Expected number of items
}