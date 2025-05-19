use axum::{extract::Path, response::Redirect, Extension};
use std::sync::Arc;
use crate::services::cache::CacheService;



#[derive(Debug)]
pub struct AppError(&'static str);

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
       (axum::http::StatusCode::NOT_FOUND, self.0).into_response()
    }
}

pub async fn redirect_handler(
    Path(code): Path<String>,
    Extension(cache): Extension<Arc<CacheService>>,
    Extension(analytics): Extension<Arc<AnalyticsService>>,
) -> Result<Redirect, AppError> {
    if let Some(url) = cache.l1.get(&code) {
        analytics.record_click(code, std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs());
        return Ok(Redirect::to(&url));
    }

    let url = cache.get(&code).await.map_err(|e| AppError(e))?;
    analytics.record_click(code, std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs());
    Ok(Redirect::to(&url))
}