use serde::Deserialize;
use validator::Validate;
use crate::validator::{validate_email_list, validate_same_site};



#[derive(Debug, Deserialize, Validate)]
pub struct SecurityConfig {
    #[validate(custom(function = "validate_email_list"))]
    pub global_admins: Vec<String>,
    #[validate(length(min = 32))]
    pub jwt_secret: String,
    #[validate(range(min = 60))]
    pub token_expiry_secs: u64,
    #[validate(length(min = 1))]
    pub domain: String, // e.g., "hyperlinkr.com"
    #[validate(length(min = 1))]
    pub subdomains: Vec<String>, // e.g., ["api", "auth"]
    #[validate(length(min = 1))]
    pub cookie_name: String, // e.g., "jwt_token"
    pub cookie_secure: bool, // HTTPS in prod
    #[validate(range(min = 60))]
    pub cookie_max_age_secs: u64,
    #[validate(custom(function = "validate_same_site"))]
    pub cookie_same_site: String, // "strict", "lax", "none"

    #[validate(length(min = 1))]
    pub cookie_domains: Option<Vec<String>>, // Optional, for subdomains
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            global_admins: vec![],
            jwt_secret: "0ecEuxack4XAdudiWTWXT3UocVEFhPZBaE0PhIJk3M3PNIfk5BnM+1WSYb0PaPaDCpApBRCPmrH89wDJNjQdyvkl6rEHoebJbmnYf+GqHA2WM6LqhNG+LCAHke8NFRnnlyHEhvr3KiJpQSKR0yWA8jqENpdLjVury+OknAJvQptoANSdIY8uF0FXU0kHLpnxdJ9HXRdyH0A3NTYX+EP9x8Jo3G5ymweJdLp/KUSHBjJGnAsHZAWlg9bOrqIEjau1VwUdDuFrv7yRMZYLBQsa6MRCZ09eRABl5MvqBMs/B8O3tYwUKeP04GqxwI2k5mk2qgMBPpij/zi5iKhDQ=".to_string(),
            token_expiry_secs: 3600 * 24 * 30, // 30 days
            domain: "hyperlinkr.cloud".to_string(),
            subdomains: vec!["api".to_string()],
            cookie_name: "jwt_token".to_string(),
            cookie_secure: true,
            cookie_max_age_secs: 3600,
            cookie_same_site: "strict".to_string(),
            cookie_domains: Some(vec![
                "hyperlinkr.cloud".to_string(),
                "admin.hyperlinkr.cloud".to_string(),
            ]), // Optional, can be set later
        }
    }
}






