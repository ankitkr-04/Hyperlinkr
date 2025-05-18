use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;
use std::arch::x86_64::*;
use arrayvec::ArrayString;

const BASE62_CHARS: &[u8] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
/// Number of bits devoted to shard selection (16 ⇒ 65 536 shards) || (10 ⇒ 1 024 shards)
const SHARD_BITS: usize = 10;
/// Mask for extracting the low SHARD_BITS bits of a random u64
const SHARD_MASK: u64 = (1 << SHARD_BITS) - 1;
/// 62³ = 238 328, used to chunk the counter into base-62 segments of 3 chars
const CHUNK: u64 = 62u64.pow(3);
/// Size of the lookup table: CHUNK entries × 3 bytes each
const LOOKUP_SIZE: usize = CHUNK as usize * 3;

/// Errors returned by [`CodeGenerator`].
#[derive(Debug, Error)]
pub enum CodeGenError {
    /// When a per-shard counter wraps from u64::MAX ⇒ 0
    #[error("Counter overflow detected")]
    CounterOverflow,
}

/// A cache-aligned wrapper around [`AtomicU64`].
#[repr(align(64))]
#[derive(Debug)]
struct PaddedAtomicU64(AtomicU64);


/// A thread-safe generator of fixed-length, 13-byte, base-62 codes.
/// 
/// Internally splits namespace into `2^SHARD_BITS` independent shards.  
/// Each call to [`next()`] picks a random shard, atomically increments its counter,  
/// and encodes (prefix + counter) in base-62 to yield a 13-char string.
#[derive(Debug)]
pub struct CodeGenerator {
    /// One 64-bit counter per shard, padded to avoid false sharing.
    counters: Box<[PaddedAtomicU64]>,
    /// Two-byte base-62 prefixes, one per shard.
    shard_prefixes: Box<[[u8; 2]; 1 << SHARD_BITS]>,
    /// Precomputed base-62 3-char strings for values 0..CHUNK.
    lookup_table: Box<[u8; LOOKUP_SIZE]>,
}

impl Default for CodeGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeGenerator {
    /// Build a new generator:
    ///
    /// 1.  Allocate and fill `shard_prefixes` with two-char base62 representations of each shard ID.  
    /// 2.  Allocate and build `lookup_table` of all 3-char base62 strings for values 0..238 328.  
    /// 3.  Allocate zeroed, padded atomic counters (one per shard).
    pub fn new() -> Self {
        // 1) Prefixes: 2-char base62 for each shard index i
        let mut prefixes = Box::new([[0u8; 2]; 1 << SHARD_BITS]);
        for i in 0..(1 << SHARD_BITS) {
            prefixes[i][0] = BASE62_CHARS[(i / 62) % 62];
            prefixes[i][1] = BASE62_CHARS[i % 62];
        }

        // 2) Lookup table: for each v in 0..238328, store 3-byte base62 code
        let mut lookup_table = Box::new([0u8; LOOKUP_SIZE]);
        for v in 0..CHUNK as usize {
            let val = v as u64;
            let off = v * 3;
            lookup_table[off] = BASE62_CHARS[(val / (62 * 62)) as usize];
            lookup_table[off + 1] = BASE62_CHARS[((val / 62) % 62) as usize];
            lookup_table[off + 2] = BASE62_CHARS[(val % 62) as usize];
        }

        // 3) One padded atomic counter per shard
        let counters = (0..(1 << SHARD_BITS))
            .map(|_| PaddedAtomicU64(AtomicU64::new(0)))
            .collect();

        Self {
            counters,
            shard_prefixes: prefixes,
            lookup_table,
        }
    }

    /// Generate the next unique 13-char code.
    ///
    /// 1.  Pick a shard via RDRAND (or thread-local counter fallback).  
    /// 2.  Atomically increment that shard’s counter (`u64`).  
    /// 3.  Combine the 2-byte shard prefix + base62(counter) to exactly 13 bytes.  
    /// 4.  Return an [`ArrayString<13>`].
    ///
    /// Returns [`CodeGenError::CounterOverflow`] if the counter wrapped.
    #[inline(always)]
    pub fn next(&self) -> Result<ArrayString<13>, CodeGenError> {
        let shard_id = self.current_shard();
        let counter = unsafe { &self.counters.get_unchecked(shard_id).0 };

        let value = counter.fetch_add(1, Ordering::Relaxed);
        if value == u64::MAX {
            return Err(CodeGenError::CounterOverflow);
        }

        // Pull prefix bytes
        let prefix = &self.shard_prefixes[shard_id];

        // Build an ArrayString of exactly 13 bytes:
        let mut buf = ArrayString::<13>::new();
        unsafe {
            // Safety: prefix is valid UTF-8, length = 2
            buf.push_str(std::str::from_utf8_unchecked(prefix));
            // Encode `value` into the remaining 11 bytes
            self.encode(value, buf.as_mut_ptr().add(2));
            Ok(buf)
        }
    }

    /// Core base-62 encoding routine: writes up to 11 chars into `output`.
    ///
    /// - Chunks of 3 chars via `lookup_table` for speed  
    /// - Falls back to 1–2 chars for remainder < CHUNK
    #[inline(always)]
    fn encode(&self, mut num: u64, output: *mut u8) {
        let mut ptr = unsafe { output.add(10) };

        // Process as many 3-char chunks as possible
        unsafe {
            while num >= CHUNK {
            let rem = (num % CHUNK) as usize;
            num /= CHUNK;
            let src = self.lookup_table.as_ptr().add(rem * 3);
            ptr = ptr.sub(3);
            ptr.copy_from_nonoverlapping(src, 3);
            }

            // Handle final 1–3 chars
            if num >= 62 {
            let rem = num as usize;
            let src = self.lookup_table.as_ptr().add(rem * 3);
            // Two or three chars? we know num < CHUNK so at most 3
            let take = if num >= 62 * 62 { 3 } else { 2 };
            ptr = ptr.sub(take);
            ptr.copy_from_nonoverlapping(src.add(3 - take), take);
            } else {
            // Single char
            *ptr = BASE62_CHARS[num as usize];
            }
        }
    }

    /// Select a shard index in 0..2^SHARD_BITS:
    /// - On x86_64: uses RDRAND  
    /// - Else: simple thread-local round-robin
    #[inline(always)]
    fn current_shard(&self) -> usize {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let mut id = 0;
            while _rdrand64_step(&mut id) != 1 {
                std::hint::spin_loop();
            }
            (id as usize) & SHARD_MASK as usize
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
                v & SHARD_MASK as usize
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_generation() {
        let g = CodeGenerator::new();
        let a = g.next().unwrap();
        let b = g.next().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.len(), 13);
        assert_eq!(b.len(), 13);
    }
}
