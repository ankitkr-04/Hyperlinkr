use once_cell::sync::OnceCell;
use prometheus::{
    IntCounterVec, HistogramVec, IntCounter, IntGauge, register_histogram_vec,
    register_int_counter_vec, register_int_counter, register_int_gauge,
};
use std::time::Instant;

pub static CACHE_HITS: OnceCell<IntCounterVec> = OnceCell::new();
pub static CACHE_MISSES: OnceCell<IntCounterVec> = OnceCell::new();
pub static CACHE_LATENCY: OnceCell<HistogramVec> = OnceCell::new();
pub static DB_LATENCY: OnceCell<HistogramVec> = OnceCell::new();
pub static DB_ERRORS: OnceCell<IntCounterVec> = OnceCell::new();
pub static HTTP_REQUESTS: OnceCell<IntCounterVec> = OnceCell::new();
pub static HTTP_LATENCY: OnceCell<HistogramVec> = OnceCell::new();
pub static CLICKS_RECORDED: OnceCell<IntCounter> = OnceCell::new();
pub static BATCHES_FLUSHED: OnceCell<IntCounter> = OnceCell::new();
pub static BATCH_SIZE: OnceCell<HistogramVec> = OnceCell::new();
pub static ANALYTICS_DROPPED: OnceCell<IntCounter> = OnceCell::new();
pub static QUEUE_LENGTH: OnceCell<IntGauge> = OnceCell::new();
pub static ANALYTICS_ERRORS: OnceCell<IntCounterVec> = OnceCell::new();
pub static SHORT_URLS_CREATED: OnceCell<IntCounter> = OnceCell::new();
pub static REDIRECTS_SERVED: OnceCell<IntCounter> = OnceCell::new();
pub fn init_metrics() {
    CACHE_HITS.set(
        register_int_counter_vec!(
            "cache_hits_total",
            "Number of cache hits",
            &["tier"]
        ).unwrap()
    ).unwrap();
    CACHE_MISSES.set(
        register_int_counter_vec!(
            "cache_misses_total",
            "Number of cache misses",
            &["tier"]
        ).unwrap()
    ).unwrap();
    CACHE_LATENCY.set(
        register_histogram_vec!(
            "cache_latency_seconds",
            "Cache access latency in seconds",
            &["tier"],
            vec![0.000001, 0.000005, 0.00001, 0.00005, 0.0001]
        ).unwrap()
    ).unwrap();
    DB_LATENCY.set(
        register_histogram_vec!(
            "db_latency_seconds",
            "Database access latency in seconds",
            &["operation"],
            vec![0.0001, 0.001, 0.01, 0.1, 1.0]
        ).unwrap()
    ).unwrap();
    DB_ERRORS.set(
        register_int_counter_vec!(
            "db_errors_total",
            "Total number of database errors",
            &["operation"]
        ).unwrap()
    ).unwrap();
    HTTP_REQUESTS.set(
        register_int_counter_vec!(
            "http_requests_total",
            "Total number of HTTP requests",
            &["endpoint", "method", "status"]
        ).unwrap()
    ).unwrap();
    HTTP_LATENCY.set(
        register_histogram_vec!(
            "http_latency_seconds",
            "HTTP request latency in seconds",
            &["endpoint", "method"],
            vec![0.1, 0.5, 1.0, 2.0, 5.0]
        ).unwrap()
    ).unwrap();
    CLICKS_RECORDED.set(
        register_int_counter!(
            "clicks_recorded_total",
            "Total number of clicks recorded"
        ).unwrap()
    ).unwrap();
    BATCHES_FLUSHED.set(
        register_int_counter!(
            "batches_flushed_total",
            "Total number of analytics batches flushed"
        ).unwrap()
    ).unwrap();
    BATCH_SIZE.set(
        register_histogram_vec!(
            "analytics_batch_size",
            "Size of analytics batches flushed",
            &["metrics"],
            vec![100.0, 500.0, 1000.0, 5000.0, 10000.0]
        ).unwrap()
    ).unwrap();
    ANALYTICS_DROPPED.set(
        register_int_counter!(
            "analytics_dropped_total",
            "Number of analytics events dropped due to full buffer"
        ).unwrap()
    ).unwrap();
    QUEUE_LENGTH.set(
        register_int_gauge!(
            "analytics_queue_length",
            "Current number of analytics events in the queue"
        ).unwrap()
    ).unwrap();
    ANALYTICS_ERRORS.set(
        register_int_counter_vec!(
            "analytics_errors_total",
            "Total number of analytics processing errors",
            &["operation"]
        ).unwrap()
    ).unwrap();
    SHORT_URLS_CREATED.set(
        register_int_counter!(
            "short_urls_created_total",
            "Total number of short URLs created"
        ).unwrap()
    ).unwrap();
    REDIRECTS_SERVED.set(
        register_int_counter!(
            "redirects_served_total",
            "Total number of redirects served"
        ).unwrap()
    ).unwrap();
}

pub fn record_cache_hit(layer: &'static str, start: Instant) {
    if let Some(counter) = CACHE_HITS.get() {
        counter.with_label_values(&[layer]).inc();
    }
    record_cache_latency(layer, start);
}

pub fn record_cache_miss(layer: &'static str) {
    if let Some(counter) = CACHE_MISSES.get() {
        counter.with_label_values(&[layer]).inc();
    }
}

pub fn record_cache_latency(layer: &'static str, start: Instant) {
    if let Some(hist) = CACHE_LATENCY.get() {
        let elapsed = start.elapsed().as_secs_f64();
        hist.with_label_values(&[layer]).observe(elapsed);
    }
}

pub fn record_db_latency(op: &'static str, start: Instant) {
    if let Some(hist) = DB_LATENCY.get() {
        let elapsed = start.elapsed().as_secs_f64();
        hist.with_label_values(&[op]).observe(elapsed);
    }
}

pub fn record_db_error(op: &'static str) {
    if let Some(counter) = DB_ERRORS.get() {
        counter.with_label_values(&[op]).inc();
    }
}

pub fn record_analytics_dropped() {
    if let Some(counter) = ANALYTICS_DROPPED.get() {
        counter.inc();
    }
}

pub fn record_click() {
    if let Some(counter) = CLICKS_RECORDED.get() {
        counter.inc();
    }
}

pub fn record_batch_flush(size: usize) {
    if let Some(counter) = BATCHES_FLUSHED.get() {
        counter.inc();
    }
    if let Some(hist) = BATCH_SIZE.get() {
        hist.with_label_values(&["analytics"]).observe(size as f64);
    }
}

pub fn update_queue_length(length: u64) {
    if let Some(gauge) = QUEUE_LENGTH.get() {
        gauge.set(length as i64);
    }
}

pub fn record_analytics_error(op: &'static str) {
    if let Some(counter) = ANALYTICS_ERRORS.get() {
        counter.with_label_values(&[op]).inc();
    }
}

pub fn record_short_url_created() {
    if let Some(counter) = SHORT_URLS_CREATED.get() {
        counter.inc();
    }
}

pub fn record_redirect_served() {
    if let Some(counter) = REDIRECTS_SERVED.get() {
        counter.inc();
    }
}

pub fn record_http_request(endpoint: &str, method: &str, status: u32) {
    if let Some(counter) = HTTP_REQUESTS.get() {
        counter.with_label_values(&[endpoint, method, &status.to_string()]).inc();
    }
}

pub fn record_http_latency(endpoint: &str, method: &str, start: Instant) {
    if let Some(hist) = HTTP_LATENCY.get() {
        let elapsed = start.elapsed().as_secs_f64();
        hist.with_label_values(&[endpoint, method]).observe(elapsed);
    }
}