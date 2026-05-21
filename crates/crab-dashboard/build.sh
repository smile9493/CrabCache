#!/bin/bash
set -euo pipefail

echo "==> Building CrabCache Dashboard WASM bundle..."
cd "$(dirname "$0")"

# Check if trunk is installed
if ! command -v trunk &> /dev/null; then
    echo "==> Installing trunk..."
    cargo install trunk
fi

# Build in release mode
trunk build --release

echo "==> Build complete. Output in dist/"
