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

python3 - <<'PY'
import hashlib
import json
import subprocess
import time
from pathlib import Path

dist = Path("dist")
tracked_suffixes = {".html", ".js", ".wasm", ".css"}

def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

def git_commit() -> str:
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "--short=12", "HEAD"],
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
    except Exception:
        return "unknown"

asset_hashes = {}
for path in sorted(dist.iterdir()):
    if path.is_file() and path.suffix in tracked_suffixes:
        asset_hashes[path.name] = sha256_file(path)

aggregate = hashlib.sha256()
for name, digest in asset_hashes.items():
    aggregate.update(name.encode("utf-8"))
    aggregate.update(b"\0")
    aggregate.update(digest.encode("ascii"))
    aggregate.update(b"\0")

build_info = {
    "git_commit": git_commit(),
    "built_at_unix": int(time.time()),
    "asset_hashes": asset_hashes,
    "dashboard_dist_hash": aggregate.hexdigest(),
}

(dist / "build-info.json").write_text(
    json.dumps(build_info, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
print(f"Dashboard build info: {build_info['dashboard_dist_hash']}")
PY

echo "Dashboard built to crates/crab-dashboard/dist/"
