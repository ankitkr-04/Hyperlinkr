use regex::Regex;
use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};
use lazy_static::lazy_static;

lazy_static! {
   static ref ALPHANUMERIC_REGEX: Regex = Regex::new(r"^[a-zA-Z0-9]+$").unwrap();
}

fn validate_custom_alias(alias: &str) -> Result<(), ValidationError> {
    static RESERVED_ALIASES: [&str; 16] = ["home", "about", "contact", "help", "terms", "privacy", "login", "signup", "dashboard", "settings", "profile", "admin", "api", "docs", "support", "blog"];
    if RESERVED_ALIASES.contains(&alias) {
        return Err(ValidationError::new("alias_is_reserved"));
    }
    if ALPHANUMERIC_REGEX.is_match(alias) {
        Ok(())
    } else {
        Err(ValidationError::new("invalid_custom_alias"))
    }
}

fn validate_future_date(date: &chrono::DateTime<chrono::Utc>) -> Result<(), ValidationError> {
    if date > &chrono::Utc::now() {
        Ok(())
    } else {
        Err(ValidationError::new("date_must_be_in_future"))
    }
}


#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct ShortenRequest {

  #[validate(url)]
  pub url: String,

  #[validate(length(min = 1, max = 20))]
  #[validate(custom(function = "validate_custom_alias"))]
  pub custom_alias: Option<String>,

    #[validate(custom(function = "validate_future_date"))]
    pub expiration_date: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize)]
pub struct ShortenResponse {
  pub short_url: String,
  pub code : String,
  pub expiration_date: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UrlData {
  pub long_url: String,
  pub created_at: chrono::DateTime<chrono::Utc>,
}