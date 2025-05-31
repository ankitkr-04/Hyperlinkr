use axum::{routing::{get, post}, Router};
use axum_server::{bind, Handle};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tracing::info;

use hyperlinkr::{
    clock::SystemClock,
    config::settings::load,
    handlers::{metrics::metrics_handler, redirect::redirect_handler, shorten::{shorten_handler, AppState}},
    middleware::rate_limit::rate_limit_middleware,
    services::{
        analytics::AnalyticsService,
        cache::{cache::CacheService, circuit_breaker::CircuitBreaker},
        codegen::generator::CodeGenerator,
        storage::dragonfly::DatabaseClient,
    },
};

#[tokio::main]
async fn main() {
    let config = Arc::new(load().expect("Failed to load configuration"));
    // dbg!(&config);
    tracing_subscriber::fmt::init(); // Must be after load() to use RUST_LOG

    let cache = Arc::new(CacheService::new(&config).await);
    let codegen = Arc::new(CodeGenerator::new(&config));

    let analytics_cb = Arc::new(CircuitBreaker::new(
        config.database_urls.clone(),
        config.cache.max_failures,
        Duration::from_secs(config.cache.retry_interval_secs),
    ));
    let _analytics_db = Arc::new(
        DatabaseClient::new(&config, Arc::clone(&analytics_cb))
            .await
            .expect("Failed to create Analytics DB client"),
    );
    let analytics = Arc::new(AnalyticsService::new(&config, analytics_cb).await);

    let rl_cb = Arc::new(CircuitBreaker::new(
        config.database_urls.clone(),
        config.cache.max_failures,
        Duration::from_secs(config.cache.retry_interval_secs),
    ));
    let rl_db = Arc::new(
        DatabaseClient::new(&config, Arc::clone(&rl_cb))
            .await
            .expect("Failed to create Rate-Limit DB client"),
    );

    let clock = Arc::new(SystemClock);

    let state = AppState {
        config: Arc::clone(&config),
        cache: Arc::clone(&cache),
        codegen: Arc::clone(&codegen),
        analytics: Arc::clone(&analytics),
        rl_db: Arc::clone(&rl_db),
        clock: Arc::clone(&clock),
    };

    let app = Router::new()
        .route("/shorten", post(shorten_handler))
        .route("/redirect/{code}", get(redirect_handler))
        .route("/metrics", get(metrics_handler))
        .layer(axum::middleware::from_fn_with_state(state.clone(), rate_limit_middleware))
        .with_state(state);

    let addr: SocketAddr = format!("0.0.0.0:{}", config.app_port)
        .parse()
        .expect("Invalid listen address");
    info!("Listening on {}", addr);

    let handle = Handle::new();
    let shutdown_handle = handle.clone();

    tokio::spawn(async move {
        shutdown_signal().await;
        shutdown_handle.graceful_shutdown(Some(Duration::from_secs(30)));
    });

    bind(addr)
        .handle(handle)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
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

    info!("Shutdown signal received, initiating graceful shutdown");
}