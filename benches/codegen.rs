use criterion::{Criterion, criterion_group, criterion_main};
use hyperlinkr::services::codegen::CodeGenerator;
use std::hint::black_box;
use std::sync::Arc;
use tokio::task;

pub fn bench_codegen(c: &mut Criterion) {
    let generator = Arc::new(CodeGenerator::new());

    // Single-threaded benchmark
    c.bench_function("codegen_next_single", |b| {
        b.iter(|| {
            let code = generator.next().unwrap();
            black_box(code);
        });
    });

    // Multi-threaded benchmark
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .build()
        .unwrap();
    c.bench_function("codegen_next_multi", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut handles = vec![];
                for _ in 0..4 {
                    let generator = Arc::clone(&generator);
                    handles.push(task::spawn(async move {
                        for _ in 0..100 {
                            let code = generator.next().unwrap();
                            black_box(code);
                        }
                    }));
                }
                for handle in handles {
                    handle.await.unwrap();
                }
            });
        });
    });
}

criterion_group!(benches, bench_codegen);
criterion_main!(benches);