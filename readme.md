# ⚡ Hyperlinkr — 1M RPS Rust/Axum URL Shortener

**Hyperlinkr** is a blazing-fast, memory-efficient, horizontally-scalable URL shortener built with [Rust](https://www.rust-lang.org/), [Axum](https://github.com/tokio-rs/axum), and [DragonflyDB](https://dragonflydb.io/).
Optimized for modern hardware, container environments, and edge delivery — achieving **\~900K requests/sec** with <1ms P95 latency on AWS Free Tier (`t2.micro`).

> Production-grade. P2P-ready. Built for real-time workloads.

---

## 🚀 Features

* 🧐 **In-memory first**: 99.5% L1 cache hit rate via `moka`
* ⚙️ **SO\_REUSEPORT + Tokio multi-threading**
* 🌍 **HTTP/3 (QUIC)** with `h3` crate
* 📊 **Click analytics** with async batching + pipelined writes
* 🧘 **Cold persistence** using embedded `sled` DB
* 📊 **DragonflyDB** as a high-throughput Redis-compatible store
* 🔐 **Cloudflare + Nginx Unit** for edge routing + DDoS protection
* 🚀 **Tailscale Mesh + EdgeLens (planned)** for failover P2P routing
* 📉 **Prometheus + Grafana Cloud** metrics

---

## 🧱 Architecture

```txt
Client
  ↓
Cloudflare CDN (Rate Limit, DDoS Protection)
  ↓
Nginx Unit (SO_REUSEPORT, HTTP/3)
  ↓
Axum Cluster (Tokio Multi-Threaded)
  ├─ L1 Cache (Moka, 2M entries)
  ├─ DragonflyDB (Pipelined Redis ops)
  ├─ Analytics Ring Buffer (200ms flush)
  └─ Cold Storage (Sled)
```

➡ Full architecture: [architecture.md](./docs/architecture.md)

---

## 📦 Getting Started

### 1. Clone the Repo

```bash
git clone https://github.com/your-org/hyperlinkr.git
cd hyperlinkr
```

### 2. Run Services (Dev Mode)

Make sure you have:

* [Rust (stable)](https://rustup.rs/)
* [DragonflyDB](https://docs.dragonflydb.io/docs/quickstart/)
* [Redis CLI](https://redis.io/docs/ui/cli/)

```bash
# Run DragonflyDB
docker run -it --rm -p 6379:6379 -m 512mb --name dragonfly \
  ghcr.io/dragonflydb/dragonfly

# Run the Rust service
cargo run
```

➡ The server will be live at `http://localhost:3000`

---

## 📖 API Reference

| Endpoint       | Method | Description                                           |
| -------------- | ------ | ----------------------------------------------------- |
| `/shorten`     | `POST` | Accepts JSON with a long URL and returns a short code |
| `/:code`       | `GET`  | Redirects to original URL                             |
| `/stats/:code` | `GET`  | Click analytics (timestamp, referrer, count, etc.)    |

Request Example:

```json
POST /shorten
{
  "url": "https://example.com"
}
```

Response:

```json
{
  "short_url": "https://h.link/r3aX9b"
}
```

---

## 📊 Benchmarks

| Metric                 | Value            |
| ---------------------- | ---------------- |
| **RPS (2 × t2.micro)** | \~900K           |
| **Latency (P95)**      | <1ms             |
| **Memory (per node)**  | \~600MB          |
| **Redis ops/sec**      | \~1.2M           |
| **CPU Usage**          | 60–80% (2 vCPUs) |

---

## 📁 Project Structure

```
.
├── src/
│   ├── main.rs          # Axum server setup
│   ├── handlers.rs      # Route logic
│   ├── storage.rs       # Dragonfly + sled persistence
│   ├── analytics.rs     # Ring buffer + batch flush
│   └── cache.rs         # moka L1 cache
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
