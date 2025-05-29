// src/handlers/metrics.rs
use axum::response::IntoResponse;
use axum::http::StatusCode;
use prometheus::Encoder;

pub async fn metrics_handler() -> impl IntoResponse {
    let encoder = prometheus::TextEncoder::new();
    let mut buffer = vec![];
    encoder.encode(&prometheus::gather(), &mut buffer).unwrap();
    (StatusCode::OK, buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_metrics_handler() {
        // We can just call it directly since it takes no extensions
        let response = metrics_handler().await.into_response();
        let (status, body) = response.into_parts();
        assert_eq!(status, StatusCode::OK);
        // Body should be non-empty
        let bytes = hyper::body::to_bytes(body).await.unwrap();
        assert!(!bytes.is_empty());
    }
}
