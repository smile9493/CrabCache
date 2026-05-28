#!/usr/bin/env bash
# Build Leptos dashboard static assets into crates/crab-dashboard/dist/
#
# Currently uses Trunk (Phase 1/2 delivery). When cargo-leptos --split is
# ready (Phase 3), switch to:
#   cargo leptos build --split --release
# and remove the Trunk post-build steps below (wasm preload, wasm-opt
# flags move into [package.metadata.leptos] in Cargo.toml).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}/crates/crab-dashboard"

# Some toolchains convert NO_COLOR=1 into `--no-color=1`, but Trunk expects
# boolean true/false. Normalize to avoid CLI parsing failure in CI/shell envs.
if [ "${NO_COLOR:-}" = "1" ]; then
  export NO_COLOR=true
fi

if ! command -v trunk >/dev/null 2>&1; then
  echo "Installing trunk..."
  cargo install trunk --locked
fi

rustup target add wasm32-unknown-unknown 2>/dev/null || true

trunk build --release

# Post-build: inject wasm preload link and record wasm size
WASM_FILE=$(ls dist/*_bg.wasm 2>/dev/null || true)
if [ -n "$WASM_FILE" ]; then
  WASM_BASENAME=$(basename "$WASM_FILE")
  WASM_SIZE=$(stat --printf="%s" "$WASM_FILE" 2>/dev/null || stat -f%z "$WASM_FILE" 2>/dev/null || echo "unknown")
  echo ""
  echo "WASM size: $WASM_SIZE bytes ($(( WASM_SIZE / 1024 )) KB)"
  echo "WASM preload: $WASM_BASENAME"

  # Insert <link rel="preload"> before </head> in the generated index.html
  PRELOAD_LINK="    <link rel=\"preload\" as=\"fetch\" crossorigin href=\"/${WASM_BASENAME}\" type=\"application/wasm\" />"
  # Use | as sed delimiter since paths contain /
  sed -i "s|</head>|${PRELOAD_LINK}\n</head>|" dist/index.html
  echo "Preload link injected into dist/index.html"

  # Baseline WASM size for comparison with future --split builds
  WASM_SIZE_KB=$(( WASM_SIZE / 1024 ))
  echo "---"
  echo "WASM baseline: single blob ${WASM_SIZE_KB}KB (before code-splitting)"
else
  echo "Warning: no _bg.wasm found in dist/"
fi

echo "Dashboard built to crates/crab-dashboard/dist/"
