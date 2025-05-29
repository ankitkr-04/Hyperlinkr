use fastbloom::BloomFilter;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

#[derive(Clone)]
pub struct CacheBloom {
    shards: Vec<BloomFilter>,
    shard_count: usize,
}

impl CacheBloom {
    pub fn new(size: usize, expected: usize, _block_size: usize) -> Self {
        let shard_count = 16;
        let size_per_shard = (size + shard_count - 1) / shard_count;
        let expected_per_shard = (expected + shard_count - 1) / shard_count;
        let shards = (0..shard_count)
            .map(|_| {
                BloomFilter::with_num_bits(size_per_shard)
                .block_size_512()
                .expected_items(expected_per_shard)
            })
            
            .collect();

        Self {
            shards,
            shard_count,
        }
    }

    #[inline]
    pub fn contains(&mut self, key: &[u8]) -> bool {
       let shard_idx = self.get_shard_index(key);
         self.shards[shard_idx].contains(key)
    }

    #[inline]
    pub fn insert(&mut self, key: &[u8]) {
        let shard_idx = self.get_shard_index(key);
        self.shards[shard_idx].insert(key);
    }

    #[inline]
    fn get_shard_index(&self, key: &[u8]) -> usize {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        (hasher.finish() % self.shard_count as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::CacheBloom;

    #[test]
    fn test_bloom_filter() {
        let mut bloom = CacheBloom::new(1000, 100, 128);
       bloom.insert(b"test_key");
        assert!(bloom.contains(b"test_key"));
        assert!(!bloom.contains(b"non_existent_key"));

        // Test with different keys
        bloom.insert(b"another_key");
        assert!(bloom.contains(b"another_key"));
        assert!(!bloom.contains(b"yet_another_key"));
    }
}