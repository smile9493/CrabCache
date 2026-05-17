#!/usr/bin/env bash
# Build Leptos dashboard static assets into crates/crab-dashboard/dist/
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}/crates/crab-dashboard"

if ! command -v trunk >/dev/null 2>&1; then
  echo "Installing trunk..."
  cargo install trunk --locked
fi

rustup target add wasm32-unknown-unknown 2>/dev/null || true

trunk build --release

echo "Dashboard built to crates/crab-dashboard/dist/"
