#!/usr/bin/env bash
# Reproduce Codex key pool loss after hot-update with NDJSON debug logs.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_PATH="${ROOT}/.cursor/debug-b4b459.log"
TARGET="${CRABCACHE_DEPLOY_TARGET:-crabcache-deploy}"

export CRABCACHE_DEBUG_LOG_PATH="/app/.cursor/debug-b4b459.log"
export CRABCACHE_DEBUG_RUN_ID="${CRABCACHE_DEBUG_RUN_ID:-repro-pre-fix}"
export CRABCACHE_DEBUG_SESSION_ID="b4b459"

rm -f "${LOG_PATH}"
mkdir -p "$(dirname "${LOG_PATH}")"

echo "== Codex key pool persist repro =="
echo "    target=${TARGET}"
echo "    log=${LOG_PATH}"
echo ""
echo "Building admin + gateway with debug instrumentation..."
cd "${ROOT}"
python3 scripts/hot_update.py --target "${TARGET}" --skip-build 2>/dev/null || true
python3 scripts/hot_update.py --target "${TARGET}"

echo ""
echo "Done. Next:"
echo "  1. Import Codex JSON on Admin upstream page (Keys tab)"
echo "  2. Immediately hot-update again: CRABCACHE_DEBUG_RUN_ID=repro-post-restart python3 scripts/hot_update.py --target ${TARGET}"
echo "  3. Check if keys disappeared in UI"
echo "  4. Press Proceed in Cursor debug UI"
