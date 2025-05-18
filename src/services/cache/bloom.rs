use fastbloom::BloomFilter;


const BLOOM_BLOCK_SIZE: usize = 128;
#[derive(Clone)]
pub struct CacheBloom {
    inner: BloomFilter<BLOOM_BLOCK_SIZE>,
}

impl CacheBloom {
    pub fn new(size: usize, expected: usize) -> Self {
        let inner = BloomFilter::with_num_bits(size)
            .block_size_128()
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
        let mut bloom = CacheBloom::new(1000, 100);
        bloom.insert("test_key");
        assert!(bloom.contains("test_key"));
        assert!(!bloom.contains("missing_key"));
    }
}