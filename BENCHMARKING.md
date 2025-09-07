# 🚀 Benchmarking Guide

## Quick Performance Tests

### 1. Code Generation Benchmark
Test the BASE62 encoding performance:

```bash
cargo bench codegen
```

**Expected Results (i3 8th gen):**
- Single-threaded: ~416,000 ops/sec
- Multi-threaded: Scales with CPU cores

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

| Test Type | Performance | Notes |
|-----------|-------------|-------|
| Code Generation | 416K ops/sec | Single-threaded benchmark |
| HTTP GET (cached) | 50K RPS | Real-world sustained load |
| HTTP POST (new URLs) | 25K RPS | With full processing |
| Cache Hit Rate | >95% | L1 + L2 combined |
| Memory Usage | ~200MB | Typical working set |

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
