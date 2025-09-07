use criterion::{Criterion, criterion_group, criterion_main};
use hyperlinkr::services::codegen::generator::CodeGenerator;
use hyperlinkr::config::settings::Settings;
use std::hint::black_box;
use std::sync::Arc;
use std::thread;

pub fn bench_codegen(c: &mut Criterion) {
    let generator = Arc::new(CodeGenerator::new(&Settings::default()));

    // Single-threaded benchmark
    c.bench_function("codegen_next_single", |b| {
        b.iter(|| {
            let code = generator.next().unwrap();
            black_box(code);
        });
    });

    // Multi-threaded benchmark
    c.bench_function("codegen_next_multi", |b| {
        b.iter_custom(|iters| {
            let generator = Arc::clone(&generator);
            let threads = 4;
            let iters_per_thread = iters / threads;

            let mut handles = Vec::new();
            for _ in 0..threads {
                let g = Arc::clone(&generator);
                handles.push(thread::spawn(move || {
                    for _ in 0..iters_per_thread {
                        let code = g.next().unwrap();
                        black_box(code);
                    }
                }));
            }

            let start = std::time::Instant::now();
            for h in handles {
                h.join().unwrap();
            }
            start.elapsed()
        });
    });
}

criterion_group!(benches, bench_codegen);
criterion_main!(benches);
