# 🚀 Benchmarking Guide

## Real-World Benchmark Results

All tests run on **i3 8th Gen, 8GB RAM, Manjaro Linux**:

```bash
# Complete benchmark suite
cargo bench

# Individual components
cargo bench --bench codegen      # 458K-768K ops/sec
cargo bench --bench cache        # 2.1M-3.2M ops/sec  
cargo bench --bench cache bloom  # 17.7M ops/sec (single), 908K ops/sec (parallel)
cargo bench --bench rate_limiting # 14.7M-1.6G ops/sec
cargo bench --bench url_processing # 2.5M-3.3M ops/sec
cargo bench --bench analytics    # Real-time processing
```

### Performance Summary

| Component | Single-Thread | Multi-Thread | Bottleneck Level |
|-----------|---------------|--------------|------------------|
| **Code Generation** | 458K ops/sec | 768K ops/sec | **PRIMARY** |
| **L1 Cache** | 2.1M ops/sec | 3.2M ops/sec | Low |
| **L2 Cache** | 2.2M ops/sec | 2.8M ops/sec | Low |
| **Bloom Filter** | 17.7M ops/sec | 908K ops/sec* | Negligible |
| **Rate Limiting** | 14.7M ops/sec | 1.6G ops/sec | Negligible |
| **URL Processing** | 3.3M ops/sec | 2.5M ops/sec | Low |

**Key Finding**: Code generation is the primary bottleneck at 458K ops/sec, making it the limiting factor for URL creation throughput.

*Bloom filter parallel insert shows contention due to atomic operations across 16 shards, but single-thread performance (17.7M ops/sec) is excellent.

### 1. Code Generation Benchmark
Test the BASE62 encoding performance:

```bash
cargo bench codegen
```

**Actual Results (i3 8th gen, 8GB RAM, Manjaro Linux):**
- Single-threaded: 458,000 ops/sec
- Multi-threaded: 768,000 ops/sec (1.68x scaling)
- Memory efficient: ~10MB working set

### 2. HTTP Load Testing

Start the server:
```bash
# Terminal 1: Start DragonflyDB
docker compose up -d dragonfly

# Terminal 2: Start application
ENVIRONMENT=benchmark cargo run --release
```

Run load tests:
```bash
# Terminal 3: Simple load test with curl
for i in {1..1000}; do
  curl -s -X POST http://localhost:3000/v1/shorten \
    -H "Content-Type: application/json" \
    -d '{"url": "https://example.com/test'$i'"}' &
done
wait
```

### 3. Apache Bench (if installed)
```bash
# Install Apache Bench
sudo pacman -S apache-tools  # Manjaro/Arch
sudo apt install apache2-utils  # Ubuntu/Debian

# Test shortening endpoint
ab -n 10000 -c 100 -p post_data.json -T application/json \
   http://localhost:3000/v1/shorten

# Create post_data.json:
echo '{"url": "https://example.com/benchmark"}' > post_data.json
```

### 4. wrk (Advanced Load Testing)
```bash
# Install wrk
sudo pacman -S wrk  # Manjaro/Arch

# Test GET requests (after creating some short URLs)
wrk -t8 -c100 -d30s http://localhost:3000/oH

# Test POST requests
wrk -t8 -c100 -d30s -s post.lua http://localhost:3000/v1/shorten
```

Create `post.lua`:
```lua
wrk.method = "POST"
wrk.body   = '{"url": "https://example.com/test"}'
wrk.headers["Content-Type"] = "application/json"
```

## Performance Monitoring

### Check Cache Performance
Monitor cache hit rates in application logs:
```bash
RUST_LOG=info cargo run --release 2>&1 | grep -E "(L1|L2|cache)"
```

### System Resource Usage
```bash
# Monitor CPU and memory
htop

# Monitor network connections
ss -tuln | grep 3000

# Monitor DragonflyDB
docker stats
```

## Expected Performance

**Hardware**: i3 8th Gen, 8GB RAM, Manjaro Linux

| Component | Single-Thread | Multi-Thread | Memory Usage | Bottleneck Level |
|-----------|---------------|--------------|--------------|------------------|
| Code Generation | 458K ops/sec | 768K ops/sec | ~10MB | **PRIMARY** |
| L1 Cache | 2.1M ops/sec | 3.2M ops/sec | ~50MB | Low |
| L2 Cache | 2.2M ops/sec | 2.8M ops/sec | ~100MB | Low |
| Bloom Filter | 17.7M ops/sec | 908K ops/sec* | ~2MB | Negligible |
| Rate Limiting | 14.7M ops/sec | 1.6G ops/sec | ~5MB | Negligible |
| URL Processing | 3.3M ops/sec | 2.5M ops/sec | ~15MB | Low |

**Real-World Throughput**:
- **Theoretical Max**: 768K URLs/sec (code generation limited)
- **Practical Throughput**: 100-200K requests/sec (network + serialization overhead)
- **Cache Hit Rate**: >95% (L1 + L2 combined)
- **Total Memory**: ~200MB typical working set

*Bloom filter shows atomic contention in parallel workloads but excellent single-thread performance.

## Tuning Tips

### 1. Cache Configuration
Edit `config.benchmark.toml`:
```toml
[cache]
l1_capacity = 50000      # Increase for more hot data
l2_capacity = 500000     # Larger L2 for better hit rates
bloom_bits = 4194304     # More bits = fewer false positives
```

### 2. Rate Limiting
For benchmarking, disable rate limits:
```toml
[rate_limit]
enabled = false          # Disable for max performance
```

### 3. Database Connections
Increase connection pool:
```toml
[cache]
redis_pool_size = 32     # More connections for higher load
```

### 4. System Tuning
```bash
# Increase file descriptor limits
ulimit -n 65536

# Optimize TCP settings for local testing
echo 'net.core.somaxconn = 65536' | sudo tee -a /etc/sysctl.conf
sudo sysctl -p
```

## Troubleshooting

### Common Issues

**Rate Limited**: 
- Use `config.benchmark.toml` with `enabled = false`
- Vary IP headers: `-H "X-Forwarded-For: 192.168.1.$RANDOM"`

**Connection Refused**:
- Check if DragonflyDB is running: `docker ps`
- Verify port 6379 is available: `ss -tuln | grep 6379`

**Low Performance**:
- Use release build: `cargo run --release`
- Check system load: `htop`
- Monitor cache hit rates in logs

**Memory Issues**:
- Reduce cache sizes in config
- Monitor with: `docker stats` and `htop`
