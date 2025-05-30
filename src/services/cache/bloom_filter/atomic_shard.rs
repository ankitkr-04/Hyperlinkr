use fastbloom::BloomFilter;
use std::cell::UnsafeCell;

pub struct AtomicBloomShard {
    inner: UnsafeCell<BloomFilter>, // Not Sync by default
}

unsafe impl Sync for AtomicBloomShard {} // You must ensure no race conditions manually

impl AtomicBloomShard {
    pub fn new(bits: usize, expected: usize) -> Self {
        let filter = BloomFilter::with_num_bits(bits)
            .block_size_512()
            .expected_items(expected);
        Self {
            inner: UnsafeCell::new(filter),
        }
    }

    #[inline(always)]
    pub fn contains(&self, key: &[u8]) -> bool {
        unsafe { (*self.inner.get()).contains(key) }
    }

    #[inline(always)]
    pub fn insert(&self, key: &[u8]) {
        let _ = unsafe { (*self.inner.get()).insert(key) };
    }
}
