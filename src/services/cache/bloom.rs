use fastbloom::BloomFilter;

#[derive(Clone)]
pub struct CacheBloom {
    inner: BloomFilter,
}

impl CacheBloom {
    pub fn new(size: usize, expected: usize, _block_size: usize) -> Self {
        let inner = BloomFilter::with_num_bits(size)
            .block_size_512()
            .expected_items(expected);
        Self { inner }
    }

    #[inline]
    pub fn contains(&self, key: &str) -> bool {
        self.inner.contains(key)
    }

    #[inline]
    pub fn insert(&mut self, key: &str) {
        self.inner.insert(key);
    }
}

#[cfg(test)]
mod tests {
    use super::CacheBloom;

    #[test]
    fn test_bloom_filter() {
        let mut bloom = CacheBloom::new(1000, 100, 128);
        bloom.insert("test_key");
        assert!(bloom.contains("test_key"));
        assert!(!bloom.contains("missing_key"));
    }
}