use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, BatchSize};
use hyperlinkr::services::cache::{l1_cache::L1Cache, l2_cache::L2Cache};
use tokio::runtime::Runtime;
use std::sync::Arc;

// ==================== L1 Cache Benchmarks ====================

fn l1_cache_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // GET HIT
    c.bench_function("l1_cache_get_hit", |b| {
        let cache = L1Cache::new(1000, 300);
        rt.block_on(async { cache.insert("test_key".to_string(), "test_value".to_string()).await });

        b.iter(|| {
            rt.block_on(async { cache.get("test_key").await })
        });
    });

    // GET MISS
    c.bench_function("l1_cache_get_miss", |b| {
        let cache = L1Cache::new(1000, 300);
        b.iter(|| rt.block_on(async { cache.get("nonexistent_key").await }) );
    });

    // INSERT
    c.bench_function("l1_cache_insert", |b| {
        let cache = L1Cache::new(1000, 300);
        b.iter_batched(
            || rand::random::<u32>(),
            |k| rt.block_on(async { cache.insert(format!("key_{}", k), "value".to_string()).await }),
            BatchSize::SmallInput
        );
    });
}

// ==================== L2 Cache Benchmarks ====================

fn l2_cache_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // GET HIT
    c.bench_function("l2_cache_get_hit", |b| {
        let cache = L2Cache::new(1000, 300);
        rt.block_on(async { cache.insert("test_key".to_string(), "test_value".to_string()).await });

        b.iter(|| rt.block_on(async { cache.get("test_key").await }) );
    });

    // GET MISS
    c.bench_function("l2_cache_get_miss", |b| {
        let cache = L2Cache::new(1000, 300);
        b.iter(|| rt.block_on(async { cache.get("nonexistent_key").await }) );
    });

    // INSERT
    c.bench_function("l2_cache_insert", |b| {
        let cache = L2Cache::new(1000, 300);
        b.iter_batched(
            || rand::random::<u32>(),
            |k| rt.block_on(async { cache.insert(format!("key_{}", k), "value".to_string()).await }),
            BatchSize::SmallInput
        );
    });
}

// ==================== Cache Size Comparison ====================

fn cache_size_comparison(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("cache_size_comparison");

    for &size in &[100, 1000, 10000] {
        group.bench_with_input(BenchmarkId::new("l1_cache", size), &size, |b, &size| {
            let cache = L1Cache::new(size, 300);
            b.iter_batched(
                || rand::random::<u32>(),
                |k| rt.block_on(async {
                    cache.insert(format!("key_{}", k), "value".to_string()).await;
                    cache.get(&format!("key_{}", k)).await;
                }),
                BatchSize::SmallInput
            );
        });

        group.bench_with_input(BenchmarkId::new("l2_cache", size), &size, |b, &size| {
            let cache = L2Cache::new(size, 300);
            b.iter_batched(
                || rand::random::<u32>(),
                |k| rt.block_on(async {
                    cache.insert(format!("key_{}", k), "value".to_string()).await;
                    cache.get(&format!("key_{}", k)).await;
                }),
                BatchSize::SmallInput
            );
        });
    }

    group.finish();
}

// ==================== Concurrent Benchmark ====================

fn concurrent_cache_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let cache = Arc::new(L1Cache::new(1000, 300));

    // Pre-populate cache
    rt.block_on(async {
        for i in 0..100 {
            cache.insert(format!("key_{}", i), format!("value_{}", i)).await;
        }
    });

    c.bench_function("concurrent_cache_reads", |b| {
        b.iter_custom(|iters| {
            rt.block_on(async {
                let start = std::time::Instant::now();
                let mut handles = Vec::with_capacity(iters as usize);

                for _ in 0..iters {
                    let cache_clone = cache.clone();
                    handles.push(tokio::spawn(async move {
                        let key = format!("key_{}", rand::random::<u32>() % 100);
                        cache_clone.get(&key).await
                    }));
                }

                futures::future::join_all(handles).await;
                start.elapsed()
            })
        });
    });
}

criterion_group!(
    benches,
    l1_cache_benchmark,
    l2_cache_benchmark,
    cache_size_comparison,
    concurrent_cache_benchmark
);
criterion_main!(benches);
