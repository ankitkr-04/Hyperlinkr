use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;
use std::env;
use validator::Validate;
use super::analytics::AnalyticsConfig;
use super::cache::CacheConfig;
use super::rate_limit::RateLimitConfig;
use super::codegen::CodeGenConfig;
use super::security::SecurityConfig;

#[derive(Debug, Deserialize, Validate)]
pub struct Settings {
    #[validate(length(min = 1))]
    pub environment: String,
    #[validate(length(min = 1))]
    pub database_urls: Vec<String>,
    #[validate(url)]
    pub base_url: String,
    #[validate(range(min = 1024, max = 65535))]
    pub app_port: u16,
    #[validate(length(min = 1))]
    pub rust_log: String,
    #[validate(nested)]
    pub cache: CacheConfig,
    #[validate(nested)]
    pub rate_limit: RateLimitConfig,
    #[validate(nested)]
    pub codegen: CodeGenConfig,
    #[validate(nested)]
    pub analytics: AnalyticsConfig,

     #[validate(nested)]
    pub security: SecurityConfig,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            environment: "development".into(),
            database_urls: vec![
                "redis://dragonfly1:6379".into(),
                "redis://dragonfly2:6380".into(),
                "redis://dragonfly3:6381".into(),
                "redis://dragonfly4:6382".into(),
            ],
            base_url: "http://localhost:3000".into(),
            app_port: 3000,
            rust_log: "debug".into(),
            cache: CacheConfig::default(),
            rate_limit: RateLimitConfig::default(),
            codegen: CodeGenConfig::default(),
            analytics: AnalyticsConfig::default(),
            security: SecurityConfig::default(),
        }
    }
}

pub fn load() -> Result<Settings, ConfigError> {
    let env = env::var("ENVIRONMENT").unwrap_or("development".into());
    let file = format!("config.{}.toml", env);

    let cfg = Config::builder()
        .add_source(File::with_name(&file).required(false))
        .add_source(Environment::with_prefix("HYPERLINKR").separator("_").try_parsing(true))
        .set_default("environment", env)?
        .set_default("database_urls", vec![
            "redis://dragonfly1:6379",
            "redis://dragonfly2:6380",
            "redis://dragonfly3:6381",
            "redis://dragonfly4:6382",
        ])?
        .set_default("base_url", "http://localhost:3000")?
        .set_default("app_port", 3000)?
        .set_default("rust_log", "debug")?
        .build()?;

    let settings: Settings = cfg.try_deserialize()?;
    settings.validate().map_err(|e| ConfigError::Message(format!("Validation failed: {}", e)))?;

    for (i, url) in settings.database_urls.iter().enumerate() {
        if !url.starts_with("redis://") {
            return Err(ConfigError::Message(format!("Invalid Redis URL[{}]: {}", i, url)));
        }
    }

    // Set RUST_LOG environment variable
    unsafe { env::set_var("RUST_LOG", &settings.rust_log) };

    Ok(settings)
}