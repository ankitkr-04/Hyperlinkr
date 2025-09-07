use serde::Deserialize;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct StorageConfig {
    #[validate(length(min = 1))]
    pub sled_path: String,
    #[validate(range(min = 1048576))] // min 1MB
    pub sled_cache_bytes: usize,
    #[validate(range(min = 1))]
    pub sled_flush_ms: u64,
    #[validate(range(min = 1))]
    pub sled_snapshot_ttl_secs: u64,
    pub sled_compression: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            sled_path: "./data/storage.sled".into(),
            sled_cache_bytes: 67_108_864, // 64MB
            sled_flush_ms: 300_000,       // 5 minutes
            sled_snapshot_ttl_secs: 5,
            sled_compression: true,
        }
    }
}
