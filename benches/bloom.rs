use criterion::{Criterion, criterion_group, criterion_main};
use hyperlinkr::services::cache::bloom_filter::bloom::CacheBloom;

fn bloom_benchmarks(c: &mut Criterion) {
  let bloom = CacheBloom::new(1_000_000, 100_000, 16);
  c.bench_function("bloom_insert", |b| {
    b.iter(|| bloom.insert(b"some_key"));
  });
  c.bench_function("bloom_contains_hit", |b| {
    bloom.insert(b"some_key");
    b.iter(|| bloom.contains(b"some_key"));
  });
}

criterion_group!(bloom, bloom_benchmarks);
criterion_main!(bloom);
