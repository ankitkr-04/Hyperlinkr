use once_cell::sync::Lazy;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct UAInfo {
    pub browser: Option<String>,
    pub os: Option<String>,
    pub device_type: String,
}

// Browsers - more specific patterns first
static BROWSER_PATTERNS: Lazy<[(&str, &str); 8]> = Lazy::new(|| [
    ("edg/", "Edge"),
    ("opr/", "Opera"),
    ("opera", "Opera"),
    ("firefox", "Firefox"),
    ("chrome", "Chrome"),
    ("safari", "Safari"),
    ("msie", "Internet Explorer"),
    ("trident", "Internet Explorer"),
]);

// OS - more specific patterns first
static OS_PATTERNS: Lazy<[(&str, &str); 8]> = Lazy::new(|| [
    ("windows phone", "Windows Phone"),
    ("windows nt", "Windows"),
    ("iphone os", "iOS"),
    ("ipad os", "iOS"),
    ("mac os x", "macOS"),
    ("android", "Android"),
    ("linux", "Linux"),
    ("freebsd", "FreeBSD"),
]);

// Devices - priority order (tablet > mobile > fallback)
static DEVICE_PATTERNS: Lazy<[(&str, &str); 8]> = Lazy::new(|| [
    ("ipad", "tablet"),
    ("kindle fire", "tablet"),
    ("tablet", "tablet"),
    ("windows phone", "mobile"),
    ("iphone", "mobile"),
    ("ipod", "mobile"),
    ("mobile", "mobile"),
    ("android", "mobile"),
]);

// OS → Device fallback
static OS_DEVICE_FALLBACK: Lazy<HashMap<&str, &str>> = Lazy::new(|| {
    HashMap::from([
        ("iOS", "mobile"),
        ("Android", "mobile"),
        ("Windows Phone", "mobile"),
    ])
});

/// High-performance UA parser using substring match
pub fn parse_user_agent(ua: &str) -> UAInfo {
    let ua = ua.to_lowercase();

    let browser = BROWSER_PATTERNS
        .iter()
        .find(|(pattern, _)| ua.contains(pattern))
        .map(|(_, name)| name.to_string());

    let os = OS_PATTERNS
        .iter()
        .find(|(pattern, _)| ua.contains(pattern))
        .map(|(_, name)| name.to_string());

    let device_type = DEVICE_PATTERNS
        .iter()
        .find(|(pattern, _)| ua.contains(pattern))
        .map(|(_, kind)| kind.to_string())
        .or_else(|| os.as_ref().and_then(|os| OS_DEVICE_FALLBACK.get(os.as_str()).map(|v| v.to_string())))
        .unwrap_or_else(|| "desktop".to_string());

    UAInfo {
        browser,
        os,
        device_type,
    }
}
