FROM rust:1.85-slim AS builder

RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    cmake \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates/crab-metrics/Cargo.toml crates/crab-metrics/Cargo.toml
COPY crates/crab-route/Cargo.toml crates/crab-route/Cargo.toml
COPY crates/crab-cache/Cargo.toml crates/crab-cache/Cargo.toml
COPY crates/crab-semantic/Cargo.toml crates/crab-semantic/Cargo.toml
COPY crates/crab-proxy/Cargo.toml crates/crab-proxy/Cargo.toml
COPY crates/crab-gateway/Cargo.toml crates/crab-gateway/Cargo.toml

RUN mkdir -p crates/crab-metrics/src && echo "" > crates/crab-metrics/src/lib.rs && \
    mkdir -p crates/crab-route/src && echo "" > crates/crab-route/src/lib.rs && \
    mkdir -p crates/crab-cache/src && echo "" > crates/crab-cache/src/lib.rs && \
    mkdir -p crates/crab-semantic/src && echo "" > crates/crab-semantic/src/lib.rs && \
    mkdir -p crates/crab-proxy/src && echo "" > crates/crab-proxy/src/lib.rs && \
    mkdir -p crates/crab-gateway/src && echo "fn main() {}" > crates/crab-gateway/src/main.rs

RUN cargo build --release -p crab-gateway 2>/dev/null || true

COPY . .

RUN cargo build --release -p crab-gateway

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/crab-gateway /app/crab-gateway
COPY config/gateway.example.toml /app/config/gateway.toml

EXPOSE 8080 9090

CMD ["/app/crab-gateway", "/app/config/gateway.toml"]
