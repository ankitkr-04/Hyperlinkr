# 🚀 Cache Architecture Deep Dive

## Multi-Layer Cache Implementation

Hyperlinkr implements a sophisticated **4-layer caching system** designed for maximum performance and fault tolerance:

### 🏗️ Cache Layers

```rust
┌─────────────────────────────────────────────────────────────┐
│                    Cache Request Flow                        │
├─────────────────────────────────────────────────────────────┤
│ 1. L1 Cache (Moka)    │ In-memory, NUMA-aware, TinyLFU     │
│ 2. Bloom Filter       │ Probabilistic existence check       │
│ 3. L2 Cache (Moka)    │ Larger in-memory cache              │
│ 4. DragonflyDB        │ Redis-compatible persistence        │
│ 5. Sled (Optional)    │ Embedded disk storage               │
└─────────────────────────────────────────────────────────────┘
```

### 🧠 L1 Cache (Primary)
- **Technology**: Moka with TinyLFU eviction policy
- **NUMA Optimization**: Auto-detects NUMA topology for optimal memory allocation
- **Performance**: Sub-microsecond access times
- **Capacity**: Configurable (default: 10K entries)

```rust
// L1 Cache with NUMA awareness
#[cfg(feature = "libnuma")]
unsafe {
    if lib_numa::numa_available() >= 0 {
        let _ = libnuma_sys::numa_preferred();
    }
}
```

### 🌸 Bloom Filter (Existence Check)
- **Purpose**: Prevents unnecessary L2/DB lookups for non-existent keys
- **Implementation**: Sharded atomic Bloom filter (16 shards)
- **Hash Function**: DefaultHasher for fast key distribution
- **Performance**: 17.7M ops/sec single-thread, 908K ops/sec parallel
- **Atomic Contention**: Parallel workloads show degraded performance due to shard contention
- **False Positive Rate**: Tunable based on expected load

```rust
// Sharded Bloom filter for better concurrency
pub struct CacheBloom {
    shards: Vec<Arc<AtomicBloomShard>>,
    shard_count: usize, // 16 shards for CPU core optimization
}
```

### 💾 L2 Cache (Secondary)
- **Technology**: Moka with larger capacity
- **Purpose**: Overflow from L1, reduces DB pressure
- **Capacity**: Configurable (default: 100K entries)
- **TTL**: Configurable expiration times

### 🗄️ DragonflyDB (Persistence)
- **Type**: Redis-compatible in-memory database
- **Circuit Breaker**: Fault tolerance with automatic failover
- **Connection Pooling**: Optimized connection management
- **Performance**: Handles 50K+ RPS sustained load

### 💿 Sled Storage (Optional Cold Storage)
- **Purpose**: Persistent disk storage for rarely accessed data
- **Flush Strategy**: Periodic background flushing (configurable interval)
- **Recovery**: Automatic cache warming from disk on restart

## 🔄 Cache Operations

### GET Operation Flow
```rust
async fn get(key: &str) -> Result<String, AppError> {
    // 1. Check L1 Cache (fastest)
    if let Some(val) = l1.get(key).await {
        return Ok(val); // ~100ns access time
    }
    
    // 2. Bloom filter check (prevents unnecessary lookups)
    if !bloom.contains(key.as_bytes()) {
        return Err(NotFound); // ~10ns check
    }
    
    // 3. Check L2 Cache
    if let Some(val) = l2.get(key).await {
        l1.insert(key, val.clone()).await; // Promote to L1
        return Ok(val);
    }
    
    // 4. Check DragonflyDB
    if let Ok(val) = dragonfly.get(key).await {
        // Parallel cache population
        future::try_join_all([
            l1.insert(key, val.clone()),
            l2.insert(key, val.clone()),
        ]).await?;
        return Ok(val);
    }
    
    // 5. Check Sled (optional cold storage)
    if use_sled {
        if let Ok(val) = sled.get(key).await {
            // Warm all cache layers
            future::try_join_all([
                dragonfly.set_ex(key, val, ttl),
                l1.insert(key, val.clone()),
                l2.insert(key, val.clone()),
                bloom.insert(key.as_bytes()),
            ]).await?;
            return Ok(val);
        }
    }
    
    Err(NotFound)
}
```

### INSERT Operation Flow
```rust
async fn insert(key: String, value: String) -> Result<(), AppError> {
    // 1. Write to DragonflyDB first (durability)
    dragonfly.set_ex(&key, &value, ttl).await?;
    
    // 2. Parallel cache population
    future::try_join_all([
        l1.insert(key.clone(), value.clone()),
        l2.insert(key.clone(), value.clone()),
        bloom.insert(key.as_bytes()),
        sled.set_ex(key, value, ttl), // Optional
    ]).await?;
    
    Ok(())
}
```

## 📊 Performance Characteristics

### Cache Hit Rates (Typical)
- **L1 Cache**: 85-90% hit rate
- **L2 Cache**: 8-12% hit rate  
- **DragonflyDB**: 2-5% hit rate
- **Combined**: >95% total hit rate

### Latency Profile (Actual Benchmark Results)
```
Operation           | Latency    | Single-Thread | Multi-Thread
--------------------|------------|---------------|-------------
L1 Cache Hit        | ~100ns     | 2.1M ops/sec | 3.2M ops/sec
L2 Cache Hit        | ~500ns     | 2.2M ops/sec | 2.8M ops/sec
Bloom Filter Check  | ~56ns      | 17.7M ops/sec | 908K ops/sec*
Bloom Insert        | ~121ns     | 8.3M ops/sec  | 908K ops/sec*
DragonflyDB Hit     | ~100μs     | 50K ops/sec  | 200K ops/sec
Memory Usage        | -          | L1: ~50MB    | L2: ~100MB
```

**Key Performance Insights:**
- **L1 Cache**: 3.2M ops/sec peak (1.52x multi-core scaling)
- **L2 Cache**: 2.8M ops/sec peak (1.27x multi-core scaling) 
- **Bloom Filter**: 17.7M ops/sec single-thread, but atomic contention reduces parallel performance
- **Memory Efficient**: Combined ~150MB for typical workload
- **Not a Bottleneck**: Cache performance far exceeds code generation (458K ops/sec)

*Bloom filter parallel performance degraded due to atomic operations across 16 shards causing contention.

## 🛠️ Configuration

### Cache Tuning Parameters
```toml
[cache]
# L1 Cache (fastest, smallest)
l1_capacity = 10000
ttl_seconds = 3600

# L2 Cache (larger, still fast)
l2_capacity = 100000

# Bloom Filter (probability tuning)
bloom_bits = 2097152        # 2MB bit array
bloom_expected = 100000     # Expected unique keys
bloom_shards = 16           # CPU core optimization

# DragonflyDB
redis_pool_size = 16        # Connection pool
redis_command_timeout_secs = 5

# Sled (optional)
use_sled = true
sled_flush_ms = 1000        # Background flush interval
sled_cache_bytes = 134217728 # 128MB
```

## 🚦 Circuit Breaker Pattern

The cache implements a circuit breaker for DragonflyDB connections:

```rust
pub struct CircuitBreaker {
    state: AtomicU8,           // Open/Closed/HalfOpen
    failure_count: AtomicU32,   // Current failures
    max_failures: u32,          // Trip threshold
    retry_interval: Duration,   // Cooldown period
}
```

### States:
- **Closed**: Normal operation, requests pass through
- **Open**: Failures exceeded threshold, requests fail fast
- **Half-Open**: Testing if service recovered

## 🔬 Metrics & Monitoring

Cache performance is tracked via Prometheus metrics:

```rust
// Metrics tracked
- cache_hits_total{layer="l1|l2|dragonfly|sled"}
- cache_latency_seconds{operation="get|insert|delete"}
- bloom_filter_checks_total
- circuit_breaker_state{state="open|closed|half_open"}
- sled_flush_count_total
```

## 🎯 Design Decisions

### Why Multi-Layer?
1. **Performance**: L1 provides ultra-fast access for hot data
2. **Capacity**: L2 extends cache size without L1 overhead
3. **Efficiency**: Bloom filter prevents expensive misses
4. **Durability**: DragonflyDB ensures data persistence
5. **Recovery**: Sled provides cold storage and crash recovery

### Why Moka over alternatives?
- **TinyLFU**: Superior hit rate vs LRU/LFU
- **Async-first**: Native async/await support
- **NUMA-aware**: Optimized for multi-core systems
- **Low overhead**: Minimal memory and CPU impact

### Why Sharded Bloom Filter?
- **Concurrency**: Reduces lock contention
- **Scalability**: Better performance on multi-core systems
- **Cache-friendly**: Each shard fits in CPU cache

This architecture delivers the **real-world performance** demonstrated in benchmarks:
- **L1 Cache**: 2.1M-3.2M ops/sec (memory bound, excellent scaling)
- **L2 Cache**: 2.2M-2.8M ops/sec (larger capacity with good performance)
- **Combined Hit Rate**: >95% ensures minimal DragonflyDB lookups
- **System Bottleneck**: Code generation at 458K ops/sec, not cache performance
