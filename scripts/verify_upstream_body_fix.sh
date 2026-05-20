#!/usr/bin/env bash
# Verify >64KiB upstream body injection fix (unit + patched pingora-proxy).
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== crab-proxy unit tests (upstream_body + upstream_headers) =="
cargo test -p crab-proxy upstream_ --quiet

echo "== pingora-proxy patch present =="
grep -q 'retry_buffer_truncated' third_party/pingora-proxy/src/proxy_h1.rs
grep -q 'retry_buffer_truncated' third_party/pingora-proxy/src/proxy_h2.rs

echo "== workspace uses patched pingora-proxy =="
cargo tree -p crab-proxy -i pingora-proxy 2>/dev/null | head -3

echo "OK: build with patched pingora-proxy and upstream body helpers."
