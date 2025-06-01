use axum::{
  extract::ConnectInfo,
  http::{header, Request, Response},
  middleware::Next,
};
use std::net::{IpAddr, SocketAddr};
use crate::{
  errors::AppError,
  services::geo_lookup,
  middleware::RequestContext,
  services::ua_parser,
};

pub async fn device_info_middleware(
  ConnectInfo(addr): ConnectInfo<SocketAddr>,
  mut req: Request<axum::body::Body>,
  next: Next,
) -> Result<Response<axum::body::Body>, AppError> {
  let ip = addr.ip().to_string();

  let referrer = req
    .headers()
    .get(header::REFERER)
    .and_then(|v| v.to_str().ok())
    .map(str::to_owned);

  let (user_agent, browser, os, device_type) = req
    .headers()
    .get(header::USER_AGENT)
    .and_then(|v| v.to_str().ok())
    .map(|ua| {
      let info = ua_parser::parse_user_agent(ua);
      (
        Some(ua.to_owned()),
        info.browser,
        info.os,
        Some(info.device_type),
      )
    })
    .unwrap_or((None, None, None, None));

  let (country, continent_code, city_name, timezone, latitude, longitude) = match ip.parse::<IpAddr>() {
    Ok(ip_addr) => match geo_lookup::lookup_geo(ip_addr).await {
      Ok(Some(geo)) => (
        geo.country_iso,
        geo.continent_code,
        geo.city_name,
        geo.timezone,
        geo.latitude,
        geo.longitude,
      ),
      _ => (None, None, None, None, None, None),
    },
    _ => (None, None, None, None, None, None),
  };

  let context = RequestContext {
    user_id: None,
    email: None,
    username: None,
    is_admin: false,
    ip: Some(ip),
    referrer,
    user_agent,
    browser,
    os,
    device_type,
    country,
    continent_code,
    city_name,
    timezone,
    latitude,
    longitude,
  };

  req.extensions_mut().insert(context);
  Ok(next.run(req).await)
}