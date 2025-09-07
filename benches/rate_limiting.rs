use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

// Simple token bucket rate limiter for benchmarking
#[derive(Debug)]
struct TokenBucket {
    capacity: u64,
    tokens: u64,
    refill_rate: u64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: u64, refill_rate: u64) -> Self {
        Self {
            capacity,
            tokens: capacity,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    fn try_consume(&mut self, tokens: u64) -> bool {
        self.refill();
        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        let tokens_to_add = (elapsed.as_secs() * self.refill_rate).min(self.capacity);
        
        self.tokens = (self.tokens + tokens_to_add).min(self.capacity);
        self.last_refill = now;
    }
}

// Simple rate limiter using HashMap of token buckets
struct SimpleRateLimiter {
    buckets: Arc<Mutex<HashMap<String, TokenBucket>>>,
    capacity: u64,
    refill_rate: u64,
}

impl SimpleRateLimiter {
    fn new(capacity: u64, refill_rate: u64) -> Self {
        Self {
            buckets: Arc::new(Mutex::new(HashMap::new())),
            capacity,
            refill_rate,
        }
    }

    fn check_rate_limit(&self, key: &str) -> bool {
        let mut buckets = self.buckets.lock().unwrap();
        let bucket = buckets
            .entry(key.to_string())
            .or_insert_with(|| TokenBucket::new(self.capacity, self.refill_rate));
        
        bucket.try_consume(1)
    }
}

// Sliding window rate limiter simulation
struct SlidingWindow {
    window: Arc<Mutex<std::collections::VecDeque<Instant>>>,
    capacity: usize,
    window_size: Duration,
}

impl SlidingWindow {
    fn new(capacity: usize, window_size: Duration) -> Self {
        Self {
            window: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            capacity,
            window_size,
        }
    }
    
    fn allow_request(&self) -> bool {
        let mut window = self.window.lock().unwrap();
        let now = Instant::now();
        let window_start = now - self.window_size;
        
        // Remove old requests
        while let Some(&front) = window.front() {
            if front < window_start {
                window.pop_front();
            } else {
                break;
            }
        }
        
        if window.len() < self.capacity {
            window.push_back(now);
            true
        } else {
            false
        }
    }
}

fn bench_token_bucket_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("token_bucket");
    
    group.bench_function("create_token_bucket", |b| {
        b.iter(|| TokenBucket::new(black_box(100), black_box(10)))
    });
    
    group.bench_function("consume_token_available", |b| {
        let mut bucket = TokenBucket::new(100, 10);
        b.iter(|| bucket.try_consume(black_box(1)))
    });
    
    group.bench_function("consume_token_unavailable", |b| {
        let mut bucket = TokenBucket::new(1, 1);
        bucket.tokens = 0; // Empty the bucket
        b.iter(|| bucket.try_consume(black_box(1)))
    });
    
    group.bench_function("refill_tokens", |b| {
        let mut bucket = TokenBucket::new(100, 10);
        bucket.tokens = 0; // Start empty
        b.iter(|| bucket.refill())
    });
    
    group.finish();
}

fn bench_rate_limiter_single_key(c: &mut Criterion) {
    let limiter = SimpleRateLimiter::new(1000, 100);
    
    c.bench_function("rate_limit_check_single_key", |b| {
        b.iter(|| limiter.check_rate_limit(black_box("test_key")))
    });
}

fn bench_rate_limiter_multiple_keys(c: &mut Criterion) {
    let limiter = SimpleRateLimiter::new(1000, 100);
    let mut counter = 0;
    
    c.bench_function("rate_limit_check_multiple_keys", |b| {
        b.iter(|| {
            counter += 1;
            limiter.check_rate_limit(&format!("key_{}", counter % 100))
        })
    });
}

fn bench_rate_limiter_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("rate_limiter_throughput");
    
    for num_keys in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*num_keys as u64));
        group.bench_with_input(
            BenchmarkId::new("throughput", num_keys),
            num_keys,
            |b, &num_keys| {
                let limiter = SimpleRateLimiter::new(1000, 100);
                b.iter(|| {
                    for i in 0..num_keys {
                        let allowed = limiter.check_rate_limit(&format!("key_{}", i));
                        black_box(allowed);
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_concurrent_rate_limiting(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let rate_limiter = Arc::new(SimpleRateLimiter::new(1000, 100));
    
    let mut group = c.benchmark_group("concurrent_rate_limiting");
    
    for threads in [1, 2, 4, 8].iter() {
        group.throughput(Throughput::Elements(*threads as u64 * 100));
        group.bench_with_input(
            BenchmarkId::new("concurrent_checks", threads),
            threads,
            |b, &thread_count| {
                b.iter_custom(|iters| {
                    rt.block_on(async {
                        let handles: Vec<_> = (0..thread_count).map(|thread_id| {
                            let rate_limiter = Arc::clone(&rate_limiter);
                            let iterations = iters / thread_count as u64;
                            
                            tokio::spawn(async move {
                                for i in 0..iterations {
                                    let ip = format!("192.168.{}.{}", 
                                        thread_id + 1, 
                                        (i % 254) + 1
                                    );
                                    let allowed = rate_limiter.check_rate_limit(&ip);
                                    black_box(allowed);
                                }
                            })
                        }).collect();
                        
                        let start = std::time::Instant::now();
                        futures::future::join_all(handles).await;
                        start.elapsed()
                    })
                });
            },
        );
    }
    group.finish();
}

fn bench_sliding_window_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("sliding_window");
    
    group.bench_function("sliding_window_check", |b| {
        let window = SlidingWindow::new(100, Duration::from_secs(1));
        b.iter(|| window.allow_request())
    });
    
    group.bench_function("sliding_window_cleanup", |b| {
        let window = SlidingWindow::new(100, Duration::from_millis(10));
        // Fill the window first
        for _ in 0..50 {
            window.allow_request();
        }
        // Wait a bit so cleanup is needed
        std::thread::sleep(Duration::from_millis(20));
        
        b.iter(|| window.allow_request())
    });
    
    group.finish();
}

fn bench_rate_limiter_algorithms_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("algorithm_comparison");
    
    let token_bucket = SimpleRateLimiter::new(100, 10);
    let sliding_window = SlidingWindow::new(100, Duration::from_secs(1));
    
    group.bench_function("token_bucket_algorithm", |b| {
        b.iter(|| token_bucket.check_rate_limit("test_key"))
    });
    
    group.bench_function("sliding_window_algorithm", |b| {
        b.iter(|| sliding_window.allow_request())
    });
    
    group.finish();
}

fn bench_rate_limiter_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage");
    
    // Test memory growth with many keys
    group.bench_function("many_keys_creation", |b| {
        b.iter(|| {
            let limiter = SimpleRateLimiter::new(100, 10);
            for i in 0..1000 {
                let _ = limiter.check_rate_limit(&format!("key_{}", i));
            }
        })
    });
    
    // Test repeated access patterns
    group.bench_function("repeated_access_pattern", |b| {
        let limiter = SimpleRateLimiter::new(100, 10);
        b.iter(|| {
            for _ in 0..100 {
                let _ = limiter.check_rate_limit("hot_key");
            }
        })
    });
    
    group.finish();
}

fn bench_burst_handling(c: &mut Criterion) {
    let mut group = c.benchmark_group("burst_handling");
    
    // Test how rate limiters handle burst traffic
    group.bench_function("burst_traffic_token_bucket", |b| {
        let limiter = SimpleRateLimiter::new(50, 10);
        b.iter(|| {
            // Simulate burst of 100 requests
            let mut allowed = 0;
            for i in 0..100 {
                if limiter.check_rate_limit(&format!("burst_key_{}", i % 10)) {
                    allowed += 1;
                }
            }
            black_box(allowed)
        })
    });
    
    group.bench_function("burst_traffic_sliding_window", |b| {
        let window = SlidingWindow::new(50, Duration::from_secs(1));
        b.iter(|| {
            // Simulate burst of 100 requests
            let mut allowed = 0;
            for _ in 0..100 {
                if window.allow_request() {
                    allowed += 1;
                }
            }
            black_box(allowed)
        })
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_token_bucket_operations,
    bench_rate_limiter_single_key,
    bench_rate_limiter_multiple_keys,
    bench_rate_limiter_throughput,
    bench_concurrent_rate_limiting,
    bench_sliding_window_simulation,
    bench_rate_limiter_algorithms_comparison,
    bench_rate_limiter_memory_usage,
    bench_burst_handling
);

criterion_main!(benches);
