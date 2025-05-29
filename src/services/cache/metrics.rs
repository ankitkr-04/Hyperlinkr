use once_cell::sync::OnceCell;
use prometheus::{IntCounterVec, HistogramVec, register_histogram_vec, register_int_counter_vec};

pub static CACHE_HITS: OnceCell<IntCounterVec> = OnceCell::new();
pub static CACHE_LATENCY: OnceCell<HistogramVec> = OnceCell::new();
pub static DB_LATENCY: OnceCell<HistogramVec> = OnceCell::new();
pub static DB_ERRORS: OnceCell<IntCounterVec> = OnceCell::new();

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
}