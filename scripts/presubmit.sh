#!/usr/bin/env bash
# Local presubmit checks. Keep this aligned with .github/workflows/ci.yml.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

CORE_TEST_PACKAGES=(
  crab-control
  crab-cache
  crab-route
  crab-proxy
  crab-gateway
  crab-metrics
  crab-admin
  crab-state
  crab-pipeline
  crab-reasoning
  crab-semantic
  crab-composition
  crab-admin-types
  crab-capture
  crab-auth
)

echo "==> cargo fmt"
cargo fmt --all --check

echo "==> cargo clippy"
cargo clippy --workspace --all-targets -- -D warnings

if ! cargo deny --version >/dev/null 2>&1; then
  echo "ERROR: cargo-deny is required. Install it with: cargo install cargo-deny --locked" >&2
  exit 1
fi

echo "==> cargo deny"
cargo deny check --all-features

echo "==> cargo test"
test_args=()
for package in "${CORE_TEST_PACKAGES[@]}"; do
  test_args+=("-p" "${package}")
done
cargo test "${test_args[@]}" --no-fail-fast

echo "==> dashboard wasm check"
cargo check -p crab-dashboard --target wasm32-unknown-unknown

echo "Presubmit checks passed."
