use axum::{Router, Extension};
use std::sync::Arc;
use bb8_redis::RedisConnectionManager;
use hyperlinkr::{config::settings::{Settings, load}, services::{cache::cache::CacheService, codegen::generator::CodeGenerator, analytics::AnalyticsService}, handlers::{shorten::shorten_handler, redirect::redirect_handler, metrics::metrics_handler}, middleware::rate_limit::rate_limit_middleware};
use tracing::info;
use tokio::signal;

async fn create_app(config: Arc<Settings>) -> Router {
    let cache = Arc::new(CacheService::new(&config).await);
    let codegen = Arc::new(CodeGenerator::new(&config));
    let analytics = Arc::new(AnalyticsService::new(config.analytics.clone()));
    let redis_manager = RedisConnectionManager::new(&config.database_url).unwrap();
    let redis_pool = bb8::Pool::builder()
        .max_size(config.cache.redis_pool_size)
        .build(redis_manager)
        .await
        .unwrap();

    Router::new()
        .route("/shorten", axum::routing::post(shorten_handler))
        .route("/redirect/:code", axum::routing::get(redirect_handler))
        .route("/metrics", axum::routing::get(metrics_handler))
        .layer(axum::middleware::from_fn_with_state(
            (config.clone(), redis_pool.clone()),
            rate_limit_middleware,
        ))
        .layer(Extension(config))
        .layer(Extension(cache))
        .layer(Extension(codegen))
        .layer(Extension(analytics))
        .layer(Extension(redis_pool))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let config = Arc::new(load().expect("Failed to load config"));
    let app = create_app(config.clone()).await;
    let addr = format!("0.0.0.0:{}", config.app_port).parse().unwrap();
    info!("Starting server on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    info!("Received shutdown signal, shutting down gracefully");
}