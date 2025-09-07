# ⚡ Hyper## 🚀 Features

* 🧐 **Multi-layer caching**: L1 (in-memory) + L2 (Redis-compatible) with Bloom filters
* ⚙️ **BASE62 encoding** with optimized code generation (416K ops/sec single-thread)
* 📊 **Real-time analytics** with device detection and geo-location
* 🔧 **Rate limiting** with IP-based controls
* 🗄️ **DragonflyDB** as high-performance Redis-compatible storage
* 📈 **Comprehensive benchmarking** tools included
* 🐳 **Docker Compose** setup for easy local developmentigh-Performance Rust URL Shortener

**Hyperlinkr** is a blazing-fast URL shortener built with [Rust](https://www.rust-lang.org/), [Axum](https://github.com/tokio-rs/axum), and [DragonflyDB](https://dragonflydb.io/).
Optimized for local development and high-performance testing — achieving **50K RPS** in real-world tests with **416K codegen ops/sec** (single-threaded on i3 8th gen).

> Fast, efficient, and built for learning system design principles.

---

## 🚀 Features

* 🧐 **Multi-layer caching**: L1 (in-memory) + L2 (Redis-compatible) with Bloom filters
* ⚙️ **BASE62 encoding** with optimized code generation (416K ops/sec single-thread)
* 📊 **Real-time analytics** with device detection and geo-location
* 🔧 **Rate limiting** with IP-based controls
* �️ **DragonflyDB** as high-performance Redis-compatible storage
* � **Comprehensive benchmarking** tools included
* � **Docker Compose** setup for easy local development

---

## 📦 Quick Start

### Prerequisites

* [Rust (stable)](https://rustup.rs/)
* [Docker & Docker Compose](https://docs.docker.com/get-docker/)

### 1. Clone and Setup

```bash
git clone https://github.com/ankitkr-04/hyperlinkr.git
cd hyperlinkr
```

### 2. Start Services

```bash
# Start DragonflyDB
docker compose up -d dragonfly

# Run the application
cargo run --release
```

The server will be available at `http://localhost:3000`

### 3. Test It Out

```bash
# Shorten a URL
curl -X POST http://localhost:3000/v1/shorten \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com"}'

# Visit the short URL (get the code from above response)
curl http://localhost:3000/{short_code}
```

---

## 📖 API Reference

| Endpoint              | Method | Description                                    |
| --------------------- | ------ | ---------------------------------------------- |
| `/v1/shorten`         | `POST` | Create short URL from long URL                |
| `/{code}`             | `GET`  | Redirect to original URL                      |
| `/v1/analytics/{code}`| `GET`  | Get click analytics for short URL             |
| `/health`             | `GET`  | Health check endpoint                         |

### Shorten URL

```bash
POST /v1/shorten
Content-Type: application/json

{
  "url": "https://example.com/very/long/url"
}
```

Response:
```json
{
  "short_url": "http://localhost:3000/oH",
  "original_url": "https://example.com/very/long/url"
}
```

---

## 📊 Benchmark Results

**Test Environment**: i3 8th Gen, 8GB RAM, Manjaro Linux

| Component                    | Performance              |
| ---------------------------- | ------------------------ |
| **HTTP RPS (Real-world)**    | 50,000 requests/sec     |
| **Code Generation**          | 416,000 ops/sec (1 core)|
| **BASE62 Encoding**          | Sub-microsecond latency |
| **Cache Hit Rate**           | >95% L1 cache hits      |
| **Memory Usage**             | ~200MB typical          |

### Run Your Own Benchmarks

```bash
# Code generation benchmark
cargo bench codegen

# HTTP load testing
ENVIRONMENT=benchmark cargo run --release
# Then in another terminal:
ab -n 10000 -c 100 -p data.json -T application/json http://localhost:3000/v1/shorten
```

📖 **[Complete Benchmarking Guide →](./BENCHMARKING.md)**

---

## 🧱 Architecture

```txt
HTTP Request
     ↓
Rate Limiter → Device Detection
     ↓              ↓
L1 Cache ← → Code Generator (BASE62)
     ↓              ↓
L2 Cache ← → DragonflyDB  
     ↓              ↓
Analytics ← → Sled Storage (Optional)
```

### Key Components

* **Code Generation**: BASE62 encoding with atomic counter (416K ops/sec)
* **4-Layer Cache**: L1 (Moka) → Bloom Filter → L2 (Moka) → DragonflyDB → Sled
* **NUMA-aware**: Optimized memory allocation for multi-core systems
* **Circuit Breaker**: Fault tolerance for database connections
* **Rate Limiting**: IP-based with configurable limits
* **Analytics**: Real-time click tracking with device/geo data

📖 **[Read detailed cache architecture →](./CACHE.md)**

---

## 🛠️ Configuration

Edit `config.development.toml` for local settings:

```toml
app_port = 3000
base_url = "http://localhost:3000"

[cache]
l1_capacity = 10000
l2_capacity = 100000
ttl_seconds = 3600

[rate_limit]
requests_per_minute = 100
burst_size = 10
```

For benchmarking, use `config.benchmark.toml` with disabled rate limits.

---

## 🔧 Development

### Project Structure

```
src/
├── main.rs              # Application entry point
├── handlers/            # HTTP request handlers
├── services/            # Business logic (analytics, cache, etc.)
├── middleware/          # Rate limiting, auth, device detection
├── config/              # Configuration management
└── types.rs             # Shared types and structures

benches/
└── codegen.rs           # Code generation benchmarks

config.*.toml            # Environment-specific configs
docker-compose.yml       # Local development services
```

### Running Tests

```bash
# Unit tests
cargo test

# Benchmarks
cargo bench

# With logging
RUST_LOG=debug cargo run
```

---

## 📈 Performance Tips

1. **Use release builds**: `cargo run --release` for production performance
2. **Tune cache sizes**: Adjust L1/L2 capacity based on your traffic
3. **Monitor hit rates**: Check cache performance in logs
4. **Rate limit tuning**: Balance protection vs. performance
5. **Connection pooling**: DragonflyDB connection pool optimization

---

---

## 📚 Documentation

### Deep Dive Technical Docs
- **[Cache Architecture](./CACHE.md)** - Detailed analysis of the 4-layer caching system
- **[Benchmarking Guide](./BENCHMARKING.md)** - Performance testing and optimization  
- **[Configuration Reference](./config.development.toml)** - All configuration options

### Key Implementation Details
- **Multi-layer Caching**: L1 (Moka) → Bloom Filter → L2 (Moka) → DragonflyDB → Sled
- **NUMA-aware Memory**: Optimized allocation for multi-core systems  
- **Circuit Breaker**: Fault tolerance for database connections
- **Sharded Bloom Filters**: 16 shards for better concurrency
- **Async-first Design**: Built with Tokio for maximum performance

---

## 🤝 Contributing

This project is focused on learning system design and high-performance Rust development. Feel free to experiment with:

* Cache strategies and algorithms
* Load testing and benchmarking  
* Database optimization
* Rate limiting algorithms
* Performance profiling

### Development Workflow
```bash
# Run tests
cargo test

# Run benchmarks
cargo bench

# Check formatting
cargo fmt --check

# Run lints
cargo clippy
```

---

## � License

MIT © [Ankit Kumar](https://github.com/ankitkr-04)

---

## 💬 Contact

**Ankit Kumar** - System Design & High-Performance Rust

* 📧 Email: [ak0182274@gmail.com](mailto:ak0182274@gmail.com)
* 💼 LinkedIn: [linkedin.com/in/ankit-kumar-2143412a3](https://www.linkedin.com/in/ankit-kumar-2143412a3/)
* 🐙 GitHub: [github.com/ankitkr-04](https://github.com/ankitkr-04/)

---

> "Performance is a feature. Optimization is an art." – Hyperlinkr
├── docs/
│   └── architecture.md  # In-depth system breakdown
├── Cargo.toml
└── README.md
```

---

## 🚀 Production Deployment

This project is Docker-ready. For production, use:

* **Rust** compiled with `--release`
* **Dragonfly** with `--proactor_threads` optimized per CPU
* **Cloudflare** for CDN and caching
* **Nginx Unit** or `SO_REUSEPORT` with systemd sockets

> Use [`tailscale`](https://tailscale.com/) for secure P2P mesh communication between edge nodes.

---

## 📡 Monitoring

* **/metrics** exposed for Prometheus scraping
* Add [Grafana Cloud](https://grafana.com/) for dashboards
* Optional: Alerting via UptimeRobot or PagerDuty

---

## 🤝 Contributing

We welcome contributors!

```bash
git checkout -b feature/awesome-thing
git commit -m "feat: add awesome thing"
git push origin feature/awesome-thing
```

Then open a PR 🎉

---

## 📜 License

MIT © Ankit Kumar

---

## 💬 Contact

Have questions or want to collaborate?

* Email: [ankit@example.com](mailto:ankit@example.com)
* Twitter: [@ankit\_handle](https://twitter.com/ankit_handle)

---

> “Shorten smarter. Deliver faster.” – Hyperlinkr
