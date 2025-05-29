use regex::Regex;
use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};
use once_cell::sync::Lazy;
use chrono::{DateTime, Utc};
use crate::clock::{Clock, SystemClock};
use crate::clock::MockClock;

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

fn validate_future_date(date: &DateTime<Utc>, clock: &dyn Clock) -> Result<(), ValidationError> {
    let now = clock.now();
    if date > &now {
        Ok(())
    } else {
        let mut err = ValidationError::new("date_must_be_in_future");
        err.add_param("date".into(), &date.to_rfc3339());
        err.add_param("now".into(), &now.to_rfc3339());
        Err(err)
    }
}

fn validate_future_date_wrapper(date: &DateTime<Utc>) -> Result<(), ValidationError> {
    validate_future_date(date, &SystemClock)
}

#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct ShortenRequest {
    #[validate(url, custom (function= "validate_url"))]
    pub url: String,
    #[validate(length(min = 1, max = 20), custom (function= "validate_custom_alias"))]
    pub custom_alias: Option<String>,
    #[validate(custom (function= "validate_future_date_wrapper"))]
    pub expiration_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct ShortenResponse {
    pub short_url: String,
    pub code: String,
    pub expiration_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UrlData {
    pub long_url: String,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Duration};

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
    fn test_future_date_validation() {
        let clock = MockClock(Utc::now());
        let past = clock.now() - Duration::days(1);
        let future = clock.now() + Duration::days(1);
        assert!(validate_future_date(&future, &clock).is_ok());
        assert!(validate_future_date(&past, &clock).is_err());
    }

    #[test]
    fn test_shorten_request_validation() {
        let clock = MockClock(Utc::now());
        let mut request = ShortenRequest {
            url: "https://example.com".to_string(),
            custom_alias: Some(" MyAlias ".to_string()),
            expiration_date: Some(clock.now() + Duration::days(1)),
        };
        assert!(request.validate().is_ok()); // Normalization happens in validate_custom_alias
        request.custom_alias = Some("admin".to_string());
        assert!(request.validate().is_err());
    }
}