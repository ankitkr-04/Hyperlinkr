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
    #[validate(range(min = 8, message = "Bloom shards must be at least 8"))]
    pub bloom_shards: usize,
    #[validate(range(min = 128, message = "Bloom block size must be at least 128"))]
    pub bloom_block_size: usize,
    #[validate(range(min = 8, message = "Redis pool size must be at least 8"))]
    pub redis_pool_size: u32,
    #[validate(range(min = 60, message = "Cache TTL must be at least 60 seconds"))]
    pub ttl_seconds: u64,
    #[validate(range(min = 3, message = "Max failures must be at least 3"))]
    pub max_failures: u32,
    #[validate(range(min = 10, message = "Retry interval must be at least 10 seconds"))]
    pub retry_interval_secs: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            l1_capacity: 10_000,
            l2_capacity: 100_000,
            bloom_bits: 10_485_760, // ~10MB
            bloom_expected: 1_000_000,
            bloom_shards: 8,
            bloom_block_size: 128,
            redis_pool_size: 128, // Increased for 1M req/sec
            ttl_seconds: 3600,
            max_failures: 10,
            retry_interval_secs: 30,
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
            batch_size: 10_000,
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct Settings {
    #[validate(length(min = 1, message = "Environment must not be empty"))]
    pub environment: String,
    #[validate(length(min = 1, message = "At least one Redis URL must be provided"))]
    pub database_urls: Vec<String>,
    #[validate(url(message = "Invalid base URL"))]
    pub base_url: String,
    #[validate(range(min = 1024, max = 65535, message = "Port must be between 1024 and 65535"))]
    pub app_port: u16,
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
            database_urls: vec!["redis://localhost:6379".to_string()],
            base_url: "http://localhost:3000".to_string(),
            app_port: 3000,
            cache: CacheConfig::default(),
            rate_limit: RateLimitConfig::default(),
            codegen: CodeGenConfig::default(),
            analytics: AnalyticsConfig::default(),
        }
    }
}

// Manual validation for database_urls
impl Validate for Settings {
    fn validate(&self) -> Result<(), validator::ValidationErrors> {
        let mut errors = validator::ValidationErrors::new();

        // Validate struct fields using derive
        if let Err(e) = <Self as validator::Validate>::validate_derived(self) {
            errors = e;
        }

        // Manually validate each URL in database_urls
        for (idx, url) in self.database_urls.iter().enumerate() {
            if validator::validate_url(url).is_err() {
                errors.add(
                    "database_urls",
                    validator::ValidationError {
                        code: std::borrow::Cow::from("url"),
                        message: Some(std::borrow::Cow::from(format!("Invalid Redis URL at index {}", idx))),
                        params: std::collections::HashMap::new(),
                    },
                );
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
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
        .set_default("database_urls", vec!["redis://localhost:6379"])?
        .set_default("base_url", "http://localhost:3000")?
        .set_default("app_port", 3000)?
        .set_default("cache.l1_capacity", 10_000)?
        .set_default("cache.l2_capacity", 100_000)?
        .set_default("cache.bloom_bits", 10_485_760)?
        .set_default("cache.bloom_expected", 1_000_000)?
        .set_default("cache.bloom_shards", 8)?
        .set_default("cache.bloom_block_size", 128)?
        .set_default("cache.redis_pool_size", 128)?
        .set_default("cache.ttl_seconds", 3600)?
        .set_default("cache.max_failures", 10)?
        .set_default("cache.retry_interval_secs", 30)?
        .set_default("rate_limit.shorten_requests_per_minute", 10)?
        .set_default("rate_limit.redirect_requests_per_minute", 1000)?
        .set_default("codegen.shard_bits", 12)?
        .set_default("codegen.max_attempts", 5)?
        .set_default("analytics.flush_interval_ms", 200)?
        .set_default("analytics.batch_size", 10_000)?
        .build()?;

    let settings: Settings = config.try_deserialize()?;
    settings.validate().map_err(|e| ConfigError::Message(format!("Validation failed: {}", e)))?;
    Ok(settings)
}