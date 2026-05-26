FROM rust:1.88-slim AS builder

RUN apt-get update && apt-get install -y \
   build-essential \
   pkg-config \
   libssl-dev \
   cmake \
   && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /usr/local/cargo && \
    printf '[source.crates-io]\nreplace-with = "ustc"\n[source.ustc]\nregistry = "sparse+https://mirrors.ustc.edu.cn/crates.io-index/"' \
    > /usr/local/cargo/config.toml

WORKDIR /app

# [patch.crates-io] pingora-proxy: required for >64KiB upstream body (see third_party/pingora-proxy/PATCH.md)
COPY Cargo.toml Cargo.lock ./
COPY third_party/pingora-proxy/Cargo.toml third_party/pingora-proxy/Cargo.toml
COPY third_party/pingora-proxy/src third_party/pingora-proxy/src
COPY crates/crab-metrics/Cargo.toml crates/crab-metrics/Cargo.toml
COPY crates/crab-route/Cargo.toml crates/crab-route/Cargo.toml
COPY crates/crab-cache/Cargo.toml crates/crab-cache/Cargo.toml
COPY crates/crab-semantic/Cargo.toml crates/crab-semantic/Cargo.toml
COPY crates/crab-proxy/Cargo.toml crates/crab-proxy/Cargo.toml
COPY crates/crab-gateway/Cargo.toml crates/crab-gateway/Cargo.toml
COPY crates/crab-reasoning/Cargo.toml crates/crab-reasoning/Cargo.toml
COPY crates/crab-control/Cargo.toml crates/crab-control/Cargo.toml
COPY crates/crab-pipeline/Cargo.toml crates/crab-pipeline/Cargo.toml
COPY crates/crab-state/Cargo.toml crates/crab-state/Cargo.toml
COPY crates/crab-composition/Cargo.toml crates/crab-composition/Cargo.toml
COPY crates/crab-admin-types/Cargo.toml crates/crab-admin-types/Cargo.toml
COPY crates/crab-capture/Cargo.toml crates/crab-capture/Cargo.toml

RUN mkdir -p crates/crab-metrics/src && echo "" > crates/crab-metrics/src/lib.rs && \
    mkdir -p crates/crab-route/src && echo "" > crates/crab-route/src/lib.rs && \
    mkdir -p crates/crab-cache/src && echo "" > crates/crab-cache/src/lib.rs && \
    mkdir -p crates/crab-semantic/src && echo "" > crates/crab-semantic/src/lib.rs && \
    mkdir -p crates/crab-proxy/src && echo "" > crates/crab-proxy/src/lib.rs && \
    mkdir -p crates/crab-gateway/src && echo "fn main() {}" > crates/crab-gateway/src/main.rs && \
    mkdir -p crates/crab-reasoning/src && echo "" > crates/crab-reasoning/src/lib.rs && \
    mkdir -p crates/crab-control/src && echo "" > crates/crab-control/src/lib.rs && \
    mkdir -p crates/crab-pipeline/src && echo "" > crates/crab-pipeline/src/lib.rs && \
    mkdir -p crates/crab-state/src && echo "" > crates/crab-state/src/lib.rs && \
    mkdir -p crates/crab-composition/src && echo "" > crates/crab-composition/src/lib.rs && \
    mkdir -p crates/crab-admin-types/src && echo "" > crates/crab-admin-types/src/lib.rs && \
    mkdir -p crates/crab-capture/src && echo "" > crates/crab-capture/src/lib.rs

RUN cargo build --release -p crab-gateway 2>/dev/null || true

COPY . .

# Rebuild with full sources (patched pingora-proxy + crab-proxy upstream body inject)
RUN cargo build --release -p crab-gateway \
    && cargo tree -p crab-proxy -i pingora-proxy | head -5

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

RUN mkdir -p /app/data /app/logs /app/config

COPY --from=builder /app/target/release/crab-gateway /app/crab-gateway
COPY config/gateway.docker.toml /app/config/gateway.toml

EXPOSE 8080 9080 9090

CMD ["/app/crab-gateway", "/app/config/gateway.toml"]
