use lazy_static::lazy_static;
use prometheus::{IntCounterVec,HistogramVec,register_histogram_vec, register_int_counter_vec};



lazy_static!{
  pub static ref CACHE_HITS: IntCounterVec = register_int_counter_vec!(
    "cache_hits",
    "Number of cache hits",
    &["tier"]
  ).unwrap();

  pub static ref CACHE_LATENCY: HistogramVec = register_histogram_vec!(
    "cache_latency_seconds",
    "Cache access latency in seconds",
    &["tier"],
    vec![0.000001, 0.000005, 0.00001] // 1µs, 5µs, 10µs
  ).unwrap();
}