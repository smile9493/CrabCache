FROM rust:1.95-slim AS builder

RUN apt-get update && apt-get install -y \
   build-essential \
   pkg-config \
   libssl-dev \
   cmake \
   && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /usr/local/cargo && \
    printf '[source.crates-io]\nreplace-with = "tuna"\n[source.tuna]\nregistry = "https://mirrors.tuna.tsinghua.edu.cn/git/crates.io-index.git"' \
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
COPY crates/crab-admin/Cargo.toml crates/crab-admin/Cargo.toml
COPY crates/crab-dashboard/Cargo.toml crates/crab-dashboard/Cargo.toml

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
    mkdir -p crates/crab-capture/src && echo "" > crates/crab-capture/src/lib.rs && \
    mkdir -p crates/crab-admin/src && echo "fn main() {}" > crates/crab-admin/src/main.rs && \
    mkdir -p crates/crab-dashboard/src && echo "" > crates/crab-dashboard/src/lib.rs

# Pre-build deps with BuildKit cache mount (registry + target persist across builds)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p crab-gateway 2>/dev/null || true

COPY third_party/pingora-proxy third_party/pingora-proxy
COPY crates/crab-metrics/src crates/crab-metrics/src
COPY crates/crab-route/src crates/crab-route/src
COPY crates/crab-cache/src crates/crab-cache/src
COPY crates/crab-semantic/src crates/crab-semantic/src
COPY crates/crab-proxy/src crates/crab-proxy/src
COPY crates/crab-gateway/src crates/crab-gateway/src
COPY crates/crab-reasoning/src crates/crab-reasoning/src
COPY crates/crab-control/src crates/crab-control/src
COPY crates/crab-pipeline/src crates/crab-pipeline/src
COPY crates/crab-state/src crates/crab-state/src
COPY crates/crab-composition/src crates/crab-composition/src
COPY crates/crab-admin-types/src crates/crab-admin-types/src
COPY crates/crab-capture/src crates/crab-capture/src
COPY config config

# Final build with cache mount — only recompiles changed crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p crab-gateway \
    && cargo tree -p crab-proxy -i pingora-proxy | head -5 \
    && cp /app/target/release/crab-gateway /app/crab-gateway

FROM ubuntu:latest

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3t64 \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

RUN mkdir -p /app/data /app/logs /app/config

COPY --from=builder /app/crab-gateway /app/crab-gateway
COPY config/gateway.docker.toml /app/config/gateway.toml

EXPOSE 8080 9080 9090

CMD ["/app/crab-gateway", "/app/config/gateway.toml"]
