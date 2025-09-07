use axum::{extract::{Path, State}, response::Redirect};
use crate::{clock::Clock, errors::AppError, handlers::shorten::AppState};
use tracing::info;
use crate::types::UrlData;

#[axum::debug_handler]
pub async fn redirect_handler(
    Path(code): Path<String>,
    State(state): State<AppState>,
) -> Result<Redirect, AppError> {
    let url_data_json = state.cache.get(&code).await?;
    let url_data: UrlData = serde_json::from_str(&url_data_json)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Check expiration
    if let Some(expires_at) = url_data.expires_at {
        let expiry = chrono::DateTime::parse_from_rfc3339(&expires_at)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        if expiry < state.clock.now() {
            return Err(AppError::NotFound("URL not found".to_string()));
        }
    }

    // Dummy/default values for required analytics fields
    state.analytics.record_click(
        &code,
        "0.0.0.0", // ip
        None,        // referrer
        None,        // country
        None,        // device_type
        None         // browser
    ).await;
    info!("Redirecting code {} to {}", code, url_data.long_url);
    Ok(Redirect::to(&url_data.long_url))
    }
