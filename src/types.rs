use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};
use regex::Regex;
use once_cell::sync::Lazy;
use chrono::{DateTime, Utc};
use crate::clock::{Clock, SystemClock};

static ALPHANUMERIC_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z0-9]+$").unwrap());
static MALICIOUS_URL_REGEX: Lazy<Regex> = Lazy::new(|| 
    Regex::new(r"(?i)^javascript:|^data:|<script|eval\(|onload=").unwrap()
);

fn validate_url(url: &str) -> Result<(), ValidationError> {
    if url.len() > 2048 {
        let mut err = ValidationError::new("url_too_long");
        err.add_param("max_length".into(), &2048);
        return Err(err);
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        let mut err = ValidationError::new("invalid_url_scheme");
        err.add_param("url".into(), &url);
        return Err(err);
    }
    if MALICIOUS_URL_REGEX.is_match(url) {
        let mut err = ValidationError::new("malicious_url");
        err.add_param("url".into(), &url);
        return Err(err);
    }
    Ok(())
}

fn validate_custom_alias(alias: &str) -> Result<(), ValidationError> {
    static RESERVED_ALIASES: [&str; 16] = [
        "home", "about", "contact", "help", "terms", "privacy", "login", "signup",
        "dashboard", "settings", "profile", "admin", "api", "docs", "support", "blog"
    ];
    let normalized = alias.trim().to_lowercase();
    if normalized.len() < 1 || normalized.len() > 20 {
        let mut err = ValidationError::new("invalid_alias_length");
        err.add_param("length".into(), &normalized.len());
        return Err(err);
    }
    if RESERVED_ALIASES.contains(&normalized.as_str()) {
        let mut err = ValidationError::new("alias_is_reserved");
        err.add_param("alias".into(), &normalized);
        return Err(err);
    }
    if !ALPHANUMERIC_REGEX.is_match(&normalized) {
        let mut err = ValidationError::new("invalid_custom_alias");
        err.add_param("alias".into(), &normalized);
        return Err(err);
    }
    Ok(())
}

fn validate_rfc3339_date(date: &str) -> Result<(), ValidationError> {
    let parsed = DateTime::parse_from_rfc3339(date)
        .map_err(|_e| {
            let mut err = ValidationError::new("invalid_rfc3339_date");
            err.add_param("value".into(), &date);
            err
        })?;
    let date_utc = parsed.with_timezone(&Utc);
    let now = SystemClock.now();
    if date_utc <= now {
        let mut err = ValidationError::new("date_must_be_in_future");
        err.add_param("date".into(), &date_utc.to_rfc3339());
        err.add_param("now".into(), &now.to_rfc3339());
        return Err(err);
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct ShortenRequest {
    #[validate(url, custom(function = "validate_url"))]
    pub url: String,
    #[validate(length(min = 1, max = 20), custom(function = "validate_custom_alias"))]
    pub custom_alias: Option<String>,
    #[validate(custom(function = "validate_rfc3339_date"))]
    pub expiration_date: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ShortenResponse {
    pub short_url: String,
    pub code: String,
    pub expiration_date: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UrlData {
    pub long_url: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthAction {
    Register,
    Login,
}


#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct AuthRequest {
    #[validate(length(min = 1, max = 100))]
    pub username: String,
    #[validate(length(min = 8, max = 100))]
    pub password: String,
    #[validate(email)]
    pub email: Option<String>, // Required for register, optional for login
    pub action: AuthAction,
   
}
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub message: String,
    pub token: Option<String>, // Only present on successful login
}

#[derive(Debug, Serialize)]
pub struct AnalyticsResponse {
    pub code: u64,
    pub total_clicks: u64,
    pub daily_clicks: HashMap<String, u64>
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub created_at: String, // ISO 8601 format
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthToken {
    pub user_id: u64,
    pub expires_at: String, // ISO 8601 format
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Duration};
    use crate::clock::MockClock;

    #[test]
    fn test_url_validation() {
        let valid_url = "https://example.com/path?query=value";
        assert!(validate_url(valid_url).is_ok());
        let long_url = "https://example.com/".to_string() + &"a".repeat(3000);
        assert!(validate_url(&long_url).is_err());
        let malicious_url = "javascript:alert('xss')";
        assert!(validate_url(malicious_url).is_err());
    }

    #[test]
    fn test_custom_alias_validation() {
        assert!(validate_custom_alias("validAlias123").is_ok());
        assert!(validate_custom_alias(" admin ").is_err()); // Reserved
        assert!(validate_custom_alias("invalid@alias").is_err());
    }

   #[test]
fn test_rfc3339_date_validation() {
    let clock = MockClock(Utc::now());
    let past = (clock.now() - Duration::days(1)).to_rfc3339();
    let future = (clock.now() + Duration::days(1)).to_rfc3339();
    let invalid = "invalid-date";

    // Valid future date
    assert!(validate_rfc3339_date(&future).is_ok());

    // Past date (should fail)
    assert!(validate_rfc3339_date(&past).is_err());

    // Invalid string format
    assert!(validate_rfc3339_date(invalid).is_err());

    // None case: use full struct to test skipping
    let req = ShortenRequest {
        url: "https://example.com".to_string(),
        custom_alias: None,
        expiration_date: None,
    };
    assert!(req.validate().is_ok()); // Should skip date validation
}


    #[test]
    fn test_shorten_request_validation() {
        let clock = MockClock(Utc::now());
        let request = ShortenRequest {
            url: "https://example.com".to_string(),
            custom_alias: Some("MyAlias".to_string()),
            expiration_date: Some((clock.now() + Duration::days(1)).to_rfc3339()),
        };
        assert!(request.validate().is_ok());

        let invalid_request = ShortenRequest {
            url: "https://example.com".to_string(),
            custom_alias: Some("admin".to_string()),
            expiration_date: Some((clock.now() - Duration::days(1)).to_rfc3339()),
        };
        assert!(invalid_request.validate().is_err());
    }
}