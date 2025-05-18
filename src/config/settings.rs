use config::{Config, ConfigError, Environment};
use serde::Deserialize;
use std::env;

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Suppress dead code warnings for prototyping
pub struct Settings {
    pub environment: String,
    pub database_url: String,
    pub base_url: String,
    pub app_port: u16,
}

pub fn load() -> Result<Settings, ConfigError> {
    let env = env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string());
    let env_file = match env.as_str() {
        "production" => ".env.production",
        _ => ".env.development",
    };

    dotenv::from_filename(env_file).ok();

    let config = Config::builder()
        .add_source(Environment::default().try_parsing(true))
        .build()?;

    config
        .try_deserialize::<Settings>()
        .map_err(|e| ConfigError::Message(format!("Failed to deserialize settings: {}", e)))
}