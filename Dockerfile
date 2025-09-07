# Multi-stage build for Rust application
FROM rust:1.86-slim AS builder

# Install required system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the project files
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
COPY benches/ ./benches/
COPY geo/ ./geo/

# Build the application in release mode
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /app/target/release/Hyperlinkr /app/hyperlinkr

# Copy configuration files
COPY config.*.toml ./

# Create data directory for sled
RUN mkdir -p ./data

# Set environment variables
ENV ENVIRONMENT=production
ENV RUST_LOG=info

# Expose the application port
EXPOSE 3000

# Run the application
CMD ["./hyperlinkr"]