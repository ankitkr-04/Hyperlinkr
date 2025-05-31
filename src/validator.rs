/// Validates a list of email strings
use regex::Regex;
use validator::ValidationError;
use once_cell::sync::Lazy;
use chrono::{DateTime, Utc};
use crate::clock::{Clock, SystemClock};
use chrono::offset::Utc;

static ALPHANUMERIC_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z0-9]+$").unwrap());
static MALICIOUS_URL_REGEX: Lazy<Regex> = Lazy::new(|| 
    Regex::new(r"(?i)^javascript:|^data:|<script|eval\(|onload=").unwrap()
);


pub fn validate_email_list(emails: &Vec<String>) -> Result<(), ValidationError> {
    let email_regex = Regex::new(r"(?i)^[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}$")
        .map_err(|_| ValidationError::new("invalid_regex"))?;

    for email in emails {
        if !email_regex.is_match(email) {
            let mut error = ValidationError::new("invalid_email");
            error.add_param("value".into(), &email.to_string());
            return Err(error);
        }
    }

    Ok(())
}

pub fn validate_same_site(value: &str) -> Result<(), ValidationError> {
    if !["strict", "lax", "none"].contains(&value.to_lowercase().as_str()) {
        let mut err = ValidationError::new("invalid_same_site");
        err.add_param("value".into(), &value);
        return Err(err);
    }
    Ok(())
}




pub fn validate_url(url: &str) -> Result<(), ValidationError> {
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

pub fn validate_custom_alias(alias: &str) -> Result<(), ValidationError> {
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

pub fn validate_rfc3339_date(date: &str) -> Result<(), ValidationError> {
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
