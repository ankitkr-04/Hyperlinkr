// benches/safe_url_processing.rs
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput, BatchSize};
use std::hint::black_box;
use url::Url;
use urlencoding::{encode, decode};
use tokio::runtime::Runtime;

mod common;
use common::{BenchConfig, MemoryGuard};

// URL validation function (same as original)
fn validate_url(url_str: &str) -> Result<Url, url::ParseError> {
    let parsed = Url::parse(url_str)?;
    
    if !["http", "https"].contains(&parsed.scheme()) {
        return Err(url::ParseError::RelativeUrlWithoutBase);
    }
    
    if parsed.host().is_none() {
        return Err(url::ParseError::RelativeUrlWithoutBase);
    }
    
    Ok(parsed)
}

// URL normalization function (same as original)
fn normalize_url(url_str: &str) -> Result<String, url::ParseError> {
    let mut parsed = Url::parse(url_str)?;
    
    if let Some(port) = parsed.port() {
        let default_port = match parsed.scheme() {
            "http" => 80,
            "https" => 443,
            _ => return Ok(parsed.to_string()),
        };
        
        if port == default_port {
            let _ = parsed.set_port(None);
        }
    }
    
    let path = parsed.path().to_string();
    if path != "/" && path.ends_with('/') {
        parsed.set_path(&path[..path.len() - 1]);
    }
    
    Ok(parsed.to_string())
}

// Memory-safe URL shortening simulation
fn shorten_url_simulation(short_code: &str) -> String {
    format!("https://s.ly/{}", short_code) // Shorter domain to save memory
}

fn url_validation(c: &mut Criterion) {
    let config = BenchConfig::for_system();
    let mut guard = MemoryGuard::new(config);
    
    if let Err(e) = guard.check_memory_usage() {
        eprintln!("Skipping URL validation benchmark: {}", e);
        return;
    }

    // Smaller test set for memory safety
    let test_urls = vec![
        "https://example.com",
        "http://example.com/path",
        "https://sub.example.com:8080/path?q=v",
        "ftp://invalid.com", // Invalid
        "https://", // Invalid
        "https://example.com/long/path/with/params?p1=v1&p2=v2",
    ];
    
    let mut group = c.benchmark_group("safe_url_validation");
    let _monitor = guard.start_monitoring();
    
    for (i, url) in test_urls.iter().enumerate() {
        if guard.check_memory_usage().is_err() {
            break;
        }
        
        group.bench_with_input(
            BenchmarkId::new("validate", i),
            url,
            |b, &url| {
                b.iter(|| {
                    let result = validate_url(black_box(url));
                    black_box(result.is_ok()); // Only check success, don't keep result
                })
            },
        );
    }
    
    guard.stop_monitoring();
    group.finish();
}

fn url_normalization(c: &mut Criterion) {
    let config = BenchConfig::for_system();
    let mut guard = MemoryGuard::new(config);
    
    if let Err(e) = guard.check_memory_usage() {
        eprintln!("Skipping URL normalization benchmark: {}", e);
        return;
    }

    let test_urls = vec![
        "https://example.com:443/",
        "http://example.com:80/path/",
        "https://example.com/path/resource/",
        "https://example.com:8080/api/v1/",
    ];
    
    let mut group = c.benchmark_group("safe_url_normalization");
    let _monitor = guard.start_monitoring();
    
    for (i, url) in test_urls.iter().enumerate() {
        if guard.check_memory_usage().is_err() {
            break;
        }
        
        group.bench_with_input(
            BenchmarkId::new("normalize", i),
            url,
            |b, &url| {
                b.iter(|| {
                    let result = normalize_url(black_box(url));
                    let _ = black_box(result.as_ref().map(|s| s.len())); // Only measure length
                })
            },
        );
    }
    
    guard.stop_monitoring();
    group.finish();
}

fn url_encoding_decoding(c: &mut Criterion) {
    let config = BenchConfig::conservative(); // Use conservative for encoding
    let mut guard = MemoryGuard::new(config);
    
    if let Err(e) = guard.check_memory_usage() {
        eprintln!("Skipping URL encoding benchmark: {}", e);
        return;
    }

    let test_strings = vec![
        "hello world",
        "path/with spaces",
        "query=value&param=test",
        "unicode: ñá 中文", // Shorter unicode string
        "special!@#$%^&*()",
    ];
    
    let mut group = c.benchmark_group("safe_url_encoding");
    let _monitor = guard.start_monitoring();
    
    for (i, string) in test_strings.iter().enumerate() {
        if guard.check_memory_usage().is_err() {
            break;
        }
        
        // Encode benchmark
        group.bench_with_input(
            BenchmarkId::new("encode", i),
            string,
            |b, &string| {
                b.iter(|| {
                    let encoded = encode(black_box(string));
                    black_box(encoded.len()); // Only measure length
                })
            },
        );
        
        // Decode benchmark  
        let encoded = encode(string);
        group.bench_with_input(
            BenchmarkId::new("decode", i),
            &encoded,
            |b, encoded| {
                b.iter(|| {
                    let decoded = decode(black_box(encoded));
                    let _ = black_box(decoded.as_ref().map(|s| s.len()));
                })
            },
        );
    }
    
    guard.stop_monitoring();
    group.finish();
}

fn url_parsing_components(c: &mut Criterion) {
    let config = BenchConfig::for_system();
    let mut guard = MemoryGuard::new(config);
    
    if let Err(e) = guard.check_memory_usage() {
        eprintln!("Skipping URL parsing benchmark: {}", e);
        return;
    }

    // Shorter test URL to save memory
    let test_url = "https://user:pass@sub.ex.com:8080/path?q=v&p=t#frag";
    
    let mut group = c.benchmark_group("safe_url_parsing_components");
    let _monitor = guard.start_monitoring();
    
    group.bench_function("parse_full_url_efficient", |b| {
        b.iter(|| {
            let url = Url::parse(black_box(test_url)).unwrap();
            // Only measure component lengths, don't allocate strings
            let scheme_len = url.scheme().len();
            let host_len = url.host_str().map(|s| s.len()).unwrap_or(0);
            let port = url.port();
            let path_len = url.path().len();
            let query_len = url.query().map(|s| s.len()).unwrap_or(0);
            let fragment_len = url.fragment().map(|s| s.len()).unwrap_or(0);
            black_box((scheme_len, host_len, port, path_len, query_len, fragment_len));
        })
    });
    
    group.bench_function("extract_domain_efficient", |b| {
        b.iter(|| {
            let url = Url::parse(black_box(test_url)).unwrap();
            let domain_len = url.host_str().map(|s| s.len()).unwrap_or(0);
            black_box(domain_len);
        })
    });
    
    guard.stop_monitoring();
    group.finish();
}

fn batch_url_processing(c: &mut Criterion) {
    let config = BenchConfig::for_system();
    let mut guard = MemoryGuard::new(config.clone());
    
    if let Err(e) = guard.check_memory_usage() {
        eprintln!("Skipping batch URL processing: {}", e);
        return;
    }

    let mut group = c.benchmark_group("safe_batch_url_processing");
    let _monitor = guard.start_monitoring();
    
    // Use much smaller batch sizes based on available memory
    for &batch_size in &config.batch_sizes {
        if batch_size > 100 { continue; } // Skip large batches
        if guard.check_memory_usage().is_err() {
            break;
        }
        
        group.throughput(Throughput::Elements(batch_size as u64));
        
        group.bench_with_input(
            BenchmarkId::new("validate_batch", batch_size), 
            &batch_size,
            |b, &size| {
                b.iter_batched(
                    || {
                        // Generate URLs in setup phase
                        (0..size)
                            .map(|i| format!("https://ex{}.com/path", i))
                            .collect::<Vec<_>>()
                    },
                    |urls| {
                        let mut valid_count = 0;
                        for url in urls {
                            if validate_url(&url).is_ok() {
                                valid_count += 1;
                            }
                        }
                        black_box(valid_count);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
        
        group.bench_with_input(
            BenchmarkId::new("normalize_batch", batch_size),
            &batch_size,
            |b, &size| {
                b.iter_batched(
                    || {
                        (0..size)
                            .map(|i| format!("https://ex{}.com/path/", i))
                            .collect::<Vec<_>>()
                    },
                    |urls| {
                        let mut processed = 0;
                        for url in urls {
                            if normalize_url(&url).is_ok() {
                                processed += 1;
                            }
                        }
                        black_box(processed);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    
    guard.stop_monitoring();
    group.finish();
}

fn url_shortening_workflow(c: &mut Criterion) {
    let config = BenchConfig::conservative();
    let mut guard = MemoryGuard::new(config);
    
    if let Err(e) = guard.check_memory_usage() {
        eprintln!("Skipping URL shortening workflow: {}", e);
        return;
    }

    let test_data = vec![
        ("https://example.com/very/long/path", "abc123"),
        ("https://another.com/api/v1/users/123", "def456"),
        ("https://social.com/posts/987/comments", "ghi789"),
    ];
    
    let mut group = c.benchmark_group("safe_url_shortening_workflow");
    let _monitor = guard.start_monitoring();
    
    group.bench_function("full_workflow_efficient", |b| {
        b.iter(|| {
            let mut processed = 0;
            for (url, code) in &test_data {
                // Validate (only check success)
                if validate_url(black_box(url)).is_ok() {
                    processed += 1;
                }
                
                // Normalize (only check success)
                if normalize_url(black_box(url)).is_ok() {
                    processed += 1;
                }
                
                // Generate short URL (measure length only)
                let short = shorten_url_simulation(black_box(code));
                black_box(short.len());
                processed += 1;
            }
            black_box(processed);
        })
    });
    
    guard.stop_monitoring();
    group.finish();
}

fn concurrent_url_processing(c: &mut Criterion) {
    let config = BenchConfig::for_system();
    let mut guard = MemoryGuard::new(config.clone());
    
    if let Err(e) = guard.check_memory_usage() {
        eprintln!("Skipping concurrent URL processing: {}", e);
        return;
    }

    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("safe_concurrent_url_processing");
    let _monitor = guard.start_monitoring();
    
    // Limit concurrency based on system capabilities
    let thread_counts = vec![1, 2, config.max_concurrent_tasks.min(4)];
    
    for thread_count in thread_counts {
        if guard.check_memory_usage().is_err() {
            break;
        }
        
        group.bench_with_input(
            BenchmarkId::new("concurrent_validation", thread_count),
            &thread_count,
            |b, &thread_count| {
                b.iter_custom(|iters| {
                    rt.block_on(async {
                        let start = std::time::Instant::now();
                        let tasks_per_thread = (iters / thread_count as u64).max(1);
                        let mut handles = Vec::with_capacity(thread_count);
                        
                        for thread_id in 0..thread_count {
                            handles.push(tokio::spawn(async move {
                                let mut valid_count = 0;
                                for i in 0..tasks_per_thread {
                                    let url = format!(
                                        "https://ex{}.com/path", 
                                        (thread_id as u64 * tasks_per_thread + i) % 100
                                    );
                                    if validate_url(&url).is_ok() {
                                        valid_count += 1;
                                    }
                                }
                                valid_count
                            }));
                        }
                        
                        let results = futures::future::join_all(handles).await;
                        let total: usize = results.into_iter().map(|r| r.unwrap()).sum();
                        black_box(total);
                        
                        start.elapsed()
                    })
                });
            },
        );
    }
    
    guard.stop_monitoring();
    group.finish();
}

fn url_memory_patterns(c: &mut Criterion) {
    let config = BenchConfig::conservative();
    let mut guard = MemoryGuard::new(config);
    
    if let Err(e) = guard.check_memory_usage() {
        eprintln!("Skipping URL memory patterns: {}", e);
        return;
    }

    let mut group = c.benchmark_group("safe_url_memory_patterns");
    let _monitor = guard.start_monitoring();
    
    // Reduced iteration count for memory safety
    group.bench_function("repeated_parsing_small", |b| {
        let url = "https://example.com/path?q=v";
        b.iter(|| {
            let mut valid_count = 0;
            for _ in 0..25 { // Reduced from 100
                if Url::parse(black_box(url)).is_ok() {
                    valid_count += 1;
                }
            }
            black_box(valid_count);
        })
    });
    
    group.bench_function("efficient_string_building", |b| {
        b.iter(|| {
            let mut url_count = 0;
            for i in 0..25 { // Reduced from 100
                let url = format!("https://ex{}.com/p", i);
                black_box(url.len()); // Only measure length
                url_count += 1;
            }
            black_box(url_count);
        })
    });
    
    guard.stop_monitoring();
    group.finish();
}

criterion_group!(
   url_benches,
   url_validation,
   url_normalization,
   url_encoding_decoding,
   url_parsing_components,
   batch_url_processing,
   url_shortening_workflow,
   concurrent_url_processing,
   url_memory_patterns
);

criterion_main!(url_benches);