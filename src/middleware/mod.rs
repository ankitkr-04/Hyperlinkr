pub mod rate_limit;
pub mod device_info;
pub mod auth;


#[derive(Clone, Default)]
pub struct RequestContext {
    pub user_id: Option<String>,      // From JWT
    pub email: Option<String>,        // From JWT
    pub username: Option<String>,     // From JWT
    pub is_admin: bool,               // From JWT
    pub ip: Option<String>,           // From ConnectInfo
    pub referrer: Option<String>,     // From Referer header
    pub user_agent: Option<String>,   // Raw User-Agent header
    pub browser: Option<String>,      // From UA parser
    pub os: Option<String>,           // From UA parser
    pub device_type: Option<String>,  // From UA parser
    pub country: Option<String>,      // From GeoLocation.country_iso
    pub continent_code: Option<String>, // From GeoLocation
    pub city_name: Option<String>,    // From GeoLocation
    pub timezone: Option<String>,     // From GeoLocation
    pub latitude: Option<f64>,        // From GeoLocation
    pub longitude: Option<f64>,       // From GeoLocation
}