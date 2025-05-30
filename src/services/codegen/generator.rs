use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;
use arrayvec::ArrayString;
use crate::config::settings::Settings;
use prometheus::{Histogram, IntCounter};
use lazy_static::lazy_static;
use tracing::debug;

const BASE62_CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

lazy_static! {
    static ref CODEGEN_LATENCY: Histogram = prometheus::register_histogram!(
        "codegen_latency_seconds",
        "Latency of code generation in seconds"
    ).unwrap();
    static ref CODEGEN_OVERFLOW_RETRIES: IntCounter = prometheus::register_int_counter!(
        "codegen_overflow_retries_total",
        "Total number of overflow retry attempts"
    ).unwrap();
    static ref CODEGEN_SHARD_USAGE: Histogram = prometheus::register_histogram!(
        "codegen_shard_usage",
        "Shard ID usage distribution",
        vec![0.0, 100.0, 500.0, 1000.0, 2000.0, 3000.0, 4000.0]
    ).unwrap();
}

#[derive(Debug, Error)]
pub enum CodeGenError {
    #[error("Counter overflow detected after multiple attempts")]
    CounterOverflow,
}

#[repr(align(64))]
#[derive(Debug)]
struct PaddedAtomicU64(AtomicU64);

#[allow(dead_code)]
#[derive(Debug)]
pub struct CodeGenerator {
    counters: Box<[PaddedAtomicU64]>,
    shard_prefixes: Box<[[u8; 2]]>,
    lookup_table: Box<[u8]>,
    shard_bits: usize,
    shard_mask: u64,
    chunk: u64,
    lookup_size: usize,
    max_attempts: usize,
}

impl CodeGenerator {
    pub fn new(config: &Settings) -> Self {
        let shard_bits = config.codegen.shard_bits;
        let max_attempts = config.codegen.max_attempts;
        let shard_mask = (1 << shard_bits) - 1;
        let chunk = 62u64.pow(3);
        let lookup_size = chunk as usize * 3;

        let mut prefixes = vec![[0u8; 2]; 1 << shard_bits].into_boxed_slice();
        for i in 0..(1 << shard_bits) {
            prefixes[i][0] = BASE62_CHARS[(i / 62) % 62];
            prefixes[i][1] = BASE62_CHARS[i % 62];
        }

        let mut lookup_table = vec![0u8; lookup_size].into_boxed_slice();
        for v in 0..chunk as usize {
            let val = v as u64;
            let off = v * 3;
            lookup_table[off] = BASE62_CHARS[(val / (62 * 62)) as usize];
            lookup_table[off + 1] = BASE62_CHARS[((val / 62) % 62) as usize];
            lookup_table[off + 2] = BASE62_CHARS[(val % 62) as usize];
        }

        
        let counters = (0..(1 << shard_bits))
            .map(|_| PaddedAtomicU64(AtomicU64::new(0)))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            counters,
            shard_prefixes: prefixes,
            lookup_table,
            shard_bits,
            shard_mask,
            chunk,
            lookup_size,
            max_attempts,
        }
    }

    #[inline(always)]
    pub fn next(&self) -> Result<ArrayString<13>, CodeGenError> {
        let timer = CODEGEN_LATENCY.start_timer();
        let mut attempts = 0;

        loop {
            let shard_id = self.current_shard();
            CODEGEN_SHARD_USAGE.observe(shard_id as f64);
            let counter = unsafe { &self.counters.get_unchecked(shard_id).0 };

            let current = counter.load(Ordering::Relaxed);
            if current == u64::MAX {
                attempts += 1;
                CODEGEN_OVERFLOW_RETRIES.inc();
                if attempts >= self.max_attempts {
                    timer.stop_and_discard();
                    return Err(CodeGenError::CounterOverflow);
                }
                std::hint::spin_loop();
                continue;
            }

            match counter.compare_exchange_weak(
                current,
                current.wrapping_add(1),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    let prefix = &self.shard_prefixes[shard_id];
                    let mut buf = ArrayString::<13>::new();
                    buf.push_str(std::str::from_utf8(prefix).unwrap());
                    unsafe {
                        self.encode(current, buf.as_mut_ptr().add(2));
                        debug!("Generated code: {}", buf);
                        timer.stop_and_record();
                        return Ok(buf);
                    }
                }
                Err(_) => continue,
            }
        }
    }

    #[inline(always)]
    fn encode(&self, mut num: u64, output: *mut u8) {
        let mut ptr = unsafe { output.add(10) };

        unsafe {
            while num >= self.chunk {
                let rem = (num % self.chunk) as usize;
                num /= self.chunk;
                let src = self.lookup_table.as_ptr().add(rem * 3);
                ptr = ptr.sub(3);
                ptr.copy_from_nonoverlapping(src, 3);
            }

            if num >= 62 {
                let rem = num as usize;
                let src = self.lookup_table.as_ptr().add(rem * 3);
                let take = if num >= 62 * 62 { 3 } else { 2 };
                ptr = ptr.sub(take);
                ptr.copy_from_nonoverlapping(src.add(3 - take), take);
            } else {
                *ptr = BASE62_CHARS[num as usize];
            }
        }
    }

    #[inline(always)]
    fn current_shard(&self) -> usize {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let mut id = 0;
            while std::arch::x86_64::_rdrand64_step(&mut id) != 1 {
                std::hint::spin_loop();
            }
            (id as usize) & self.shard_mask as usize
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            use std::cell::Cell;
            thread_local! {
                static CTR: Cell<usize> = Cell::new(0);
            }
            CTR.with(|c| {
                let v = c.get();
                c.set(v.wrapping_add(1));
                v & self.shard_mask as usize
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{settings::Settings, codegen::CodeGenConfig};

    #[test]
    fn test_code_generation() {
        let config = Settings {
            codegen: CodeGenConfig { shard_bits: 12, max_attempts: 5 },
            ..Default::default()
        };
        let g = CodeGenerator::new(&config);
        let a = g.next().unwrap();
        let b = g.next().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.len(), 13);
        assert_eq!(b.len(), 13);
    }

    #[test]
    fn test_overflow_handling() {
        let config = Settings {
            codegen: CodeGenConfig { shard_bits: 2, max_attempts: 5 },
            ..Default::default()
        };
        let g = CodeGenerator::new(&config);
        for c in g.counters.iter() {
            c.0.store(u64::MAX, Ordering::Relaxed);
        }
        for _ in 0..10 {
            assert!(matches!(g.next(), Err(CodeGenError::CounterOverflow)));
        }
    }

    #[test]
    fn test_high_concurrency() {
        let config = Settings {
            codegen: CodeGenConfig { shard_bits: 1, max_attempts: 5 },
            ..Default::default()
        };
        let g = Arc::new(CodeGenerator::new(&config));
        let mut handles = vec![];
        for _ in 0..10 {
            let g = g.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..1000 {
                    g.next().unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }
}