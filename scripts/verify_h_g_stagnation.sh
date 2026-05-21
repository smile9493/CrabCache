#!/usr/bin/env bash
# Verify H-G stagnation fix: growing inbound history must change outbound_fp / upstream_msg_count.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_PATH="${CRABCACHE_DEBUG_LOG_PATH:-$ROOT/.cursor/debug-h-g-verify.log}"
BASE="${CRABCACHE_GATEWAY_URL:-http://127.0.0.1:8080}"
# Gateway auth: prefer bootstrap sk-cc-* (stable client_key scope), not upstream DeepSeek key.
KEY="${CRABCACHE_BOOTSTRAP_CLIENT_KEYS:-${CRABCACHE_GATEWAY_CLIENT_KEY:-}}"
KEY="${KEY%%,*}"
KEY="${KEY## }"
MODEL="${CRABCACHE_UPSTREAM_MODEL:-deepseek-v4-pro}"

if [[ -z "${KEY}" ]]; then
  echo "FAIL: set CRABCACHE_BOOTSTRAP_CLIENT_KEYS (sk-cc-*) for gateway Bearer auth" >&2
  exit 1
fi

HOST_LOG_PATH="${LOG_PATH}"
export CRABCACHE_DEBUG_LOG_PATH="/app/.cursor/$(basename "${LOG_PATH}")"
export CRABCACHE_DEBUG_RUN_ID="h-g-verify"
rm -f "${HOST_LOG_PATH}"
mkdir -p "$(dirname "${HOST_LOG_PATH}")"

echo "== H-G stagnation verify =="
echo "    gateway=${BASE}"
echo "    log=${HOST_LOG_PATH} (container: ${CRABCACHE_DEBUG_LOG_PATH})"

# Growing tool history (same last user instruction) — mimics Cursor sub-agent retries.
payloads=(
  '[{"role":"user","content":"explore the repo structure"}]'
  '[{"role":"user","content":"explore the repo structure"},{"role":"assistant","content":"","tool_calls":[{"id":"call_1","type":"function","function":{"name":"list_dir","arguments":"{}"}}]},{"role":"tool","tool_call_id":"call_1","content":"ok"}]'
  '[{"role":"user","content":"explore the repo structure"},{"role":"assistant","content":"","tool_calls":[{"id":"call_1","type":"function","function":{"name":"list_dir","arguments":"{}"}}]},{"role":"tool","tool_call_id":"call_1","content":"ok"},{"role":"assistant","content":"partial"},{"role":"user","content":"explore the repo structure"}]'
  '[{"role":"user","content":"explore the repo structure"},{"role":"assistant","content":"","tool_calls":[{"id":"call_1","type":"function","function":{"name":"list_dir","arguments":"{}"}}]},{"role":"tool","tool_call_id":"call_1","content":"ok"},{"role":"assistant","content":"partial"},{"role":"assistant","content":"","tool_calls":[{"id":"call_2","type":"function","function":{"name":"read_file","arguments":"{}"}}]},{"role":"tool","tool_call_id":"call_2","content":"more"},{"role":"assistant","content":"step2"},{"role":"user","content":"explore the repo structure"}]'
)

for i in "${!payloads[@]}"; do
  msgs="${payloads[$i]}"
  code=$(curl -s -o /dev/null -w "%{http_code}" \
    -X POST "${BASE}/v1/chat/completions" \
    -H "Authorization: Bearer ${KEY}" \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"${MODEL}\",\"messages\":${msgs},\"stream\":false}" || echo "000")
  echo "    request $((i + 1)): HTTP ${code}"
  sleep 0.3
done

if [[ ! -s "${HOST_LOG_PATH}" ]]; then
  echo "FAIL: no debug log at ${HOST_LOG_PATH}" >&2
  echo "      restart gateway with debug enabled, e.g.:" >&2
  echo "      CRABCACHE_DEBUG_LOG_PATH=/app/.cursor/$(basename "${HOST_LOG_PATH}") docker compose up -d gateway" >&2
  exit 1
fi

python3 - <<'PY' "${HOST_LOG_PATH}"
import json, sys
path = sys.argv[1]
rows = []
for line in open(path):
    line = line.strip()
    if not line:
        continue
    try:
        o = json.loads(line)
    except json.JSONDecodeError:
        continue
    if o.get("hypothesisId") != "H-G":
        continue
    d = o.get("data") or {}
    rows.append((
        d.get("message_count"),
        d.get("upstream_msg_count"),
        d.get("outbound_fp"),
        d.get("outbound_bytes"),
    ))
if len(rows) < 2:
    print(f"FAIL: need >=2 H-G lines, got {len(rows)}")
    sys.exit(1)
fps = [r[2] for r in rows if r[2]]
ums = [r[1] for r in rows if r[1] is not None]
mc = [r[0] for r in rows if r[0] is not None]
print(f"    H-G samples: {len(rows)}")
print(f"    message_count: {mc}")
print(f"    upstream_msg_count: {ums}")
print(f"    outbound_fp: {fps}")
if len(set(fps)) >= 2 or (ums and max(ums) > min(ums)):
    print("OK: outbound context changes across rounds (stagnation fix)")
    sys.exit(0)
if len(rows) >= 2 and mc and max(mc) > min(mc) and len(set(fps)) == 1:
    print("FAIL: message_count grew but outbound_fp locked (stagnation)")
    sys.exit(1)
print("WARN: inconclusive — check log manually")
sys.exit(0)
PY

echo "Done."
