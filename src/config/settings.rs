use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;
use std::env;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct CacheConfig {
    #[validate(range(min = 1000, message = "L1 cache capacity must be at least 1000"))]
    pub l1_capacity: usize,
    #[validate(range(min = 10000, message = "L2 cache capacity must be at least 10000"))]
    pub l2_capacity: usize,
    #[validate(range(min = 1048576, message = "Bloom filter bits must be at least 1M"))]
    pub bloom_bits: usize,
    #[validate(range(min = 1000, message = "Bloom expected items must be at least 1000"))]
    pub bloom_expected: usize,
    #[validate(range(min = 128, message = "Bloom block size must be at least 128"))]
    pub bloom_block_size: usize,
    #[validate(range(min = 8, message = "Redis pool size must be at least 8"))]
    pub redis_pool_size: u32,
    #[validate(range(min = 60, message = "Cache TTL must be at least 60 seconds"))]
    pub ttl_seconds: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            l1_capacity: 10000,
            l2_capacity: 100000,
            bloom_bits: 1048576,
            bloom_expected: 100000,
            bloom_block_size: 128,
            redis_pool_size: 32,
            ttl_seconds: 3600,
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct RateLimitConfig {
    #[validate(range(min = 1, message = "Shorten rate limit must be at least 1"))]
    pub shorten_requests_per_minute: u32,
    #[validate(range(min = 100, message = "Redirect rate limit must be at least 100"))]
    pub redirect_requests_per_minute: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            shorten_requests_per_minute: 10,
            redirect_requests_per_minute: 1000,
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct CodeGenConfig {
    #[validate(range(min = 8, max = 16, message = "Shard bits must be between 8 and 16"))]
    pub shard_bits: Option<usize>,
    #[validate(range(min = 3, max = 10, message = "Max attempts must be between 3 and 10"))]
    pub max_attempts: Option<usize>,
}

impl Default for CodeGenConfig {
    fn default() -> Self {
        Self {
            shard_bits: Some(12),
            max_attempts: Some(5),
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct AnalyticsConfig {
    #[validate(range(min = 100, message = "Flush interval must be at least 100ms"))]
    pub flush_interval_ms: u64,
    #[validate(range(min = 1000, message = "Batch size must be at least 1000"))]
    pub batch_size: usize,
}

impl AnalyticsConfig {
    pub fn clone(&self) -> Self {
        Self {
            flush_interval_ms: self.flush_interval_ms,
            batch_size: self.batch_size,
        }
    }
}

impl Default for AnalyticsConfig {
    fn default() -> Self {
        Self {
            flush_interval_ms: 200,
            batch_size: 10000,
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct Settings {
    #[validate(length(min = 1, message = "Environment must not be empty"))]
    pub environment: String,
    #[validate(url(message = "Invalid Redis URL"))]
    pub database_url: String,
    #[validate(url(message = "Invalid base URL"))]
    pub base_url: String,
    #[validate(range(min = 1024, max = 65535, message = "Port must be between 1024 and 65535"))]
    pub app_port: u16,
    #[validate(length(min = 1, message = "Dragonfly host must not be empty"))]
    pub dragonfly_host: String,
    #[validate(range(min = 1024, max = 65535, message = "Dragonfly port must be between 1024 and 65535"))]
    pub dragonfly_port: u16,
    #[validate(nested)]
    pub cache: CacheConfig,
    #[validate(nested)]
    pub rate_limit: RateLimitConfig,
    #[validate(nested)]
    pub codegen: CodeGenConfig,
    #[validate(nested)]
    pub analytics: AnalyticsConfig,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            environment: "development".to_string(),
            database_url: "redis://localhost:6379".to_string(),
            base_url: "http://localhost:3000".to_string(),
            app_port: 3000,
            dragonfly_host: "localhost".to_string(),
            dragonfly_port: 6379,
            cache: CacheConfig::default(),
            rate_limit: RateLimitConfig::default(),
            codegen: CodeGenConfig::default(),
            analytics: AnalyticsConfig::default(),
        }
    }
}

pub fn load() -> Result<Settings, ConfigError> {
    let env = env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string());
    let config_file = match env.as_str() {
        "production" => "config.production.toml",
        _ => "config.development.toml",
    };

    let config = Config::builder()
        .add_source(File::with_name(config_file).required(false))
        .add_source(File::with_name("config.toml").required(false))
        .add_source(Environment::with_prefix("HYPERLINKR").separator("_").try_parsing(true))
        .set_default("environment", env)?
        .set_default("database_url", "redis://localhost:6379")?
        .set_default("base_url", "http://localhost:3000")?
        .set_default("app_port", 3000)?
        .set_default("dragonfly_host", "localhost")?
        .set_default("dragonfly_port", 6379)?
        .set_default("cache.l1_capacity", 10000)?
        .set_default("cache.l2_capacity", 100000)?
        .set_default("cache.bloom_bits", 1048576)?
        .set_default("cache.bloom_expected", 100000)?
        .set_default("cache.bloom_block_size", 128)?
        .set_default("cache.redis_pool_size", 32)?
        .set_default("cache.ttl_seconds", 3600)?
        .set_default("rate_limit.shorten_requests_per_minute", 10)?
        .set_default("rate_limit.redirect_requests_per_minute", 1000)?
        .set_default("codegen.shard_bits", 12)?
        .set_default("codegen.max_attempts", 5)?
        .set_default("analytics.flush_interval_ms", 200)?
        .set_default("analytics.batch_size", 10000)?
        .build()?;

    let settings: Settings = config.try_deserialize()?;
    settings.validate().map_err(|e| ConfigError::Message(format!("Validation failed: {}", e)))?;
    Ok(settings)
}