use once_cell::sync::OnceCell;
use prometheus::{IntCounterVec, HistogramVec, register_histogram_vec, register_int_counter_vec, IntCounter, register_int_counter};
use std::time::Instant;

pub static CACHE_HITS: OnceCell<IntCounterVec> = OnceCell::new();
pub static CACHE_LATENCY: OnceCell<HistogramVec> = OnceCell::new();
pub static DB_LATENCY: OnceCell<HistogramVec> = OnceCell::new();
pub static DB_ERRORS: OnceCell<IntCounterVec> = OnceCell::new();
pub static ANALYTICS_DROPPED: OnceCell<IntCounter> = OnceCell::new();

pub fn init_metrics() {
    CACHE_HITS.set(
        register_int_counter_vec!(
            "cache_hits",
            "Number of cache hits",
            &["tier"]
        ).unwrap()
    ).unwrap();
    CACHE_LATENCY.set(
        register_histogram_vec!(
            "cache_latency_seconds",
            "Cache access latency in seconds",
            &["tier"],
            vec![0.000001, 0.000005, 0.00001]
        ).unwrap()
    ).unwrap();
    DB_LATENCY.set(
        register_histogram_vec!(
            "db_latency_seconds",
            "Database access latency in seconds",
            &["operation"],
            vec![0.0001, 0.001, 0.01]
        ).unwrap()
    ).unwrap();
    DB_ERRORS.set(
        register_int_counter_vec!(
            "db_errors_total",
            "Total number of database errors",
            &["operation"]
        ).unwrap()
    ).unwrap();

     ANALYTICS_DROPPED.set(
        register_int_counter!(
            "analytics_dropped_total",
            "Number of analytics events dropped due to full buffer"
        ).unwrap()
    ).unwrap();
}

pub fn record_cache_hit(layer: &'static str, start: Instant) {
    if let Some(cache_hits) = CACHE_HITS.get() {
        cache_hits.with_label_values(&[layer]).inc();
    }
    record_cache_latency(layer, start);
}

pub fn record_cache_latency(layer: &'static str, start: Instant) {
    if let Some(cache_latency) = CACHE_LATENCY.get() {
        let elapsed = start.elapsed().as_secs_f64();
        cache_latency.with_label_values(&[layer]).observe(elapsed);
    }
}

pub fn record_db_latency(op: &'static str, start: Instant) {
    if let Some(db_latency) = DB_LATENCY.get() {
        let elapsed = start.elapsed().as_secs_f64();
        db_latency.with_label_values(&[op]).observe(elapsed);
    }
}


pub fn record_db_error(op: &'static str) {
    if let Some(db_errors) = DB_ERRORS.get() {
        db_errors.with_label_values(&[op]).inc();
    }
}
pub fn record_analytics_dropped() {
    if let Some(analytics_dropped) = ANALYTICS_DROPPED.get() {
        analytics_dropped.inc();
    }
}