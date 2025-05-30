use serde::Deserialize;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct CodeGenConfig {
    #[validate(range(min = 8, max = 16))]
    pub shard_bits: usize,
    #[validate(range(min = 3, max = 10))]
    pub max_attempts: usize,
}

impl Default for CodeGenConfig {
    fn default() -> Self {
        Self {
            shard_bits: 12,
            max_attempts: 5,
        }
    }
}
