use axum::response::IntoResponse;
use prometheus::Encoder;

pub async fn metrics_handler() -> impl IntoResponse {
    let encoder = prometheus::TextEncoder::new();
    let mut buffer = vec![];
    encoder.encode(&prometheus::gather(), &mut buffer).unwrap();
    (axum::http::StatusCode::OK, buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn test_metrics_handler() {
        let response = metrics_handler().await.into_response();
        let (status, _body) = response.into_parts();
        assert_eq!(status, StatusCode::OK);
    }
}