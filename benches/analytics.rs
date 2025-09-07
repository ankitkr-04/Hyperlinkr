use criterion::{Criterion, criterion_group, criterion_main, BenchmarkId, Throughput, BatchSize};
use hyperlinkr::services::analytics::AnalyticsMessage;
use hyperlinkr::types::{AnalyticsRequest, AnalyticsFilters};
use std::hint::black_box;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashMap;

mod common;
use common::{BenchConfig, MemoryGuard};

fn create_sample_analytics_message(id: u64) -> AnalyticsMessage {
    AnalyticsMessage::Click {
        code: format!("c{}", id), // Shorter format to save memory
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        ip: format!("192.168.1.{}", (id % 254) + 1),
        referrer: Some(format!("ex{}.com", id % 10)), // Shorter URLs
        country: Some("US".to_string()),
        device_type: Some("Desktop".to_string()),
        browser: Some("Chrome".to_string()),
    }
}

fn analytics_processing(c: &mut Criterion) {
    let config = BenchConfig::for_system();
    let mut guard = MemoryGuard::new(config);
    
    if let Err(e) = guard.check_memory_usage() {
        eprintln!("Skipping analytics processing benchmark: {}", e);
        return;
    }

    let mut group = c.benchmark_group("analytics");
    let _monitor = guard.start_monitoring();

    // Basic analytics message creation (memory-safe)
    group.bench_function("create_analytics_message", |b| {
        let mut counter = 0u64;
        b.iter_batched(
            || {
                counter = counter.wrapping_add(1);
                counter
            },
            |id| {
                let message = create_sample_analytics_message(black_box(id));
                black_box(&message);
                // Explicitly drop to free memory immediately
                drop(message);
            },
            BatchSize::SmallInput,
        );
    });

    // User agent parsing (simplified)
    group.bench_function("parse_user_agent_simple", |b| {
        let user_agents = vec![
            "Mozilla/5.0 (X11; Linux x86_64) Chrome/91.0",
            "Mozilla/5.0 (Windows NT 10.0) Firefox/89.0",
            "Mozilla/5.0 (Macintosh) Safari/14.1",
        ];
        
        b.iter_batched(
            || &user_agents[fastrand::usize(0..user_agents.len())],
            |ua| {
                let browser = if ua.contains("Chrome") { "Chrome" }
                           else if ua.contains("Firefox") { "Firefox" }
                           else if ua.contains("Safari") { "Safari" }
                           else { "Unknown" };
                let device = if ua.contains("Mobile") { "Mobile" } else { "Desktop" };
                black_box((browser, device));
            },
            BatchSize::SmallInput,
        );
    });

    guard.stop_monitoring();
    group.finish();
}

fn batch_processing(c: &mut Criterion) {
    let config = BenchConfig::for_system();
    let mut guard = MemoryGuard::new(config.clone());
    
    if let Err(e) = guard.check_memory_usage() {
        eprintln!("Skipping batch processing benchmark: {}", e);
        return;
    }

    let mut group = c.benchmark_group("batch_processing");
    let _monitor = guard.start_monitoring();

    // Use smaller, memory-safe batch sizes
    for &batch_size in &config.batch_sizes {
        if guard.check_memory_usage().is_err() {
            eprintln!("Memory limit reached, stopping at batch size {}", batch_size);
            break;
        }

        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("process_batch_safe", batch_size),
            &batch_size,
            |b, &size| {
                b.iter_batched(
                    || {
                        // Create batch in setup to avoid counting allocation time
                        (0..size)
                            .map(|i| create_sample_analytics_message(i as u64))
                            .collect::<Vec<_>>()
                    },
                    |batch| {
                        // Process batch
                        let count = batch.len();
                        black_box(count);
                        // Explicitly drop to free memory
                        drop(batch);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    guard.stop_monitoring();
    group.finish();
}

fn serialization(c: &mut Criterion) {
    let config = BenchConfig::conservative(); // Use conservative config for serialization
    let mut guard = MemoryGuard::new(config);
    
    if let Err(e) = guard.check_memory_usage() {
        eprintln!("Skipping serialization benchmark: {}", e);
        return;
    }

    let mut group = c.benchmark_group("serialization");
    let _monitor = guard.start_monitoring();

    let analytics_request = AnalyticsRequest {
        code: Some("test".to_string()), // Shorter strings
        page: Some(1),
        per_page: Some(10),
        filters: Some(AnalyticsFilters {
            start_date: Some("2023-01-01T00:00:00Z".to_string()),
            end_date: Some("2023-12-31T23:59:59Z".to_string()),
            country: Some("US".to_string()),
            referrer: Some("ex.com".to_string()), // Shorter
            device_type: Some("Desktop".to_string()),
            browser: Some("Chrome".to_string()),
        }),
    };

    // Serialize
    group.bench_function("serialize_request", |b| {
        b.iter_batched(
            || &analytics_request,
            |req| {
                let json = serde_json::to_string(black_box(req)).unwrap();
                black_box(json.len()); // Only measure length to avoid keeping string
            },
            BatchSize::SmallInput,
        );
    });

    // Deserialize
    let json_data = serde_json::to_string(&analytics_request).unwrap();
    group.bench_function("deserialize_request", |b| {
        b.iter_batched(
            || &json_data,
            |json| {
                let data: AnalyticsRequest = serde_json::from_str(black_box(json)).unwrap();
                black_box(data.code.as_ref().map(|s| s.len())); // Only check length
            },
            BatchSize::SmallInput,
        );
    });

    guard.stop_monitoring();
    group.finish();
}

fn hashmap_aggregation(c: &mut Criterion) {
    let config = BenchConfig::for_system();
    let mut guard = MemoryGuard::new(config);
    
    if let Err(e) = guard.check_memory_usage() {
        eprintln!("Skipping hashmap aggregation benchmark: {}", e);
        return;
    }

    let mut group = c.benchmark_group("hashmap_aggregation");
    let _monitor = guard.start_monitoring();

    // Smaller datasets for memory safety
    let countries = ["US", "CA", "UK", "DE", "FR"];
    let browsers = ["Chrome", "Firefox", "Safari", "Edge"];

    group.bench_function("country_aggregation_small", |b| {
        b.iter(|| {
            let mut counts: HashMap<&str, u32> = HashMap::with_capacity(countries.len());
            // Process smaller dataset (100 instead of 1000)
            for i in 0..100 {
                let country = countries[i % countries.len()];
                *counts.entry(country).or_insert(0) += 1;
            }
            black_box(counts.len());
        });
    });

    group.bench_function("browser_aggregation_small", |b| {
        b.iter(|| {
            let mut counts: HashMap<&str, u32> = HashMap::with_capacity(browsers.len());
            for i in 0..100 {
                let browser = browsers[i % browsers.len()];
                *counts.entry(browser).or_insert(0) += 1;
            }
            black_box(counts.len());
        });
    });

    guard.stop_monitoring();
    group.finish();
}

fn time_operations(c: &mut Criterion) {
    let config = BenchConfig::conservative();
    let mut guard = MemoryGuard::new(config);
    
    if let Err(e) = guard.check_memory_usage() {
        eprintln!("Skipping time operations benchmark: {}", e);
        return;
    }

    let mut group = c.benchmark_group("time_operations");
    let _monitor = guard.start_monitoring();

    group.bench_function("timestamp_now", |b| {
        b.iter(|| {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            black_box(timestamp);
        });
    });

    group.bench_function("format_timestamp_efficient", |b| {
        let timestamp = 1693987200u64;
        b.iter(|| {
            // Use itoa for faster integer formatting
            let mut buffer = itoa::Buffer::new();
            let formatted = buffer.format(black_box(timestamp));
            black_box(formatted.len());
        });
    });

    guard.stop_monitoring();
    group.finish();
}

criterion_group!(
    benches,
    analytics_processing,
    batch_processing,
    serialization,
    hashmap_aggregation,
    time_operations
);
criterion_main!(benches);