#!/usr/bin/env bash
# End-to-end Cursor-oriented checks: TLS domain, SSE shape, optional admin upstream pool.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

DOMAIN="${DOMAIN:-your-domain.example.com}"
API_PORT="${API_PORT:-18000}"
CLIENT_API_KEY="${CLIENT_API_KEY:-}"
ADMIN_KEY="${CRABCACHE_GATEWAY_ADMIN_KEY:-${CRABCACHE_ADMIN_KEY:-admin}}"
API_BASE="https://${DOMAIN}:${API_PORT}"
MGMT_BASE="${MGMT_BASE:-http://127.0.0.1:9080}"

if [[ -z "${CLIENT_API_KEY}" ]]; then
  echo "ERROR: set CLIENT_API_KEY (sk-cc-* from Management)" >&2
  exit 1
fi

fail=0

echo "==> [1/4] Domain + port (TLS strict where curl supports it)"
if ! bash "${SCRIPT_DIR}/verify_domain_port.sh"; then
  fail=1
fi

ALIAS_MODEL="${ALIAS_MODEL:-gpt-4o}"

echo "==> [2/5] Non-stream chat (deepseek-v4-flash)"
BODY='{"model":"deepseek-v4-flash","messages":[{"role":"user","content":"cursor-e2e-verify"}],"max_tokens":8,"stream":false}'
code=$(curl -sk -m 120 -o /tmp/crabcache_cursor_nostream.json -w "%{http_code}" \
  -X POST "${API_BASE}/v1/chat/completions" \
  -H "Authorization: Bearer ${CLIENT_API_KEY}" \
  -H "Content-Type: application/json" \
  -d "${BODY}" || echo 000)
if [[ "${code}" == "200" ]] && python3 -c "import json; json.load(open('/tmp/crabcache_cursor_nostream.json'))" 2>/dev/null; then
  echo "OK  non-stream chat (${code})"
else
  echo "FAIL non-stream chat (got ${code})" >&2
  fail=1
fi

echo "==> [3/5] Non-stream chat (alias model ${ALIAS_MODEL})"
BODY_ALIAS=$(printf '{"model":"%s","messages":[{"role":"user","content":"cursor-alias-e2e"}],"max_tokens":8,"stream":false}' "${ALIAS_MODEL}")
code_alias=$(curl -sk -m 120 -o /tmp/crabcache_cursor_alias.json -w "%{http_code}" \
  -X POST "${API_BASE}/v1/chat/completions" \
  -H "Authorization: Bearer ${CLIENT_API_KEY}" \
  -H "Content-Type: application/json" \
  -d "${BODY_ALIAS}" || echo 000)
if [[ "${code_alias}" == "200" ]] && python3 -c "import json; d=json.load(open('/tmp/crabcache_cursor_alias.json')); assert d.get('model')=='${ALIAS_MODEL}'" 2>/dev/null; then
  echo "OK  alias model non-stream (${code_alias}, model=${ALIAS_MODEL})"
else
  echo "FAIL alias model (got ${code_alias}) — configure [gateway.cursor_models.aliases] on gateway" >&2
  fail=1
fi

echo "==> [4/5] Model suffix deepseek-v4-flash-max (stream)"
export CURL_EXTRA="-sk"
if ! bash "${SCRIPT_DIR}/verify_stream_sse.sh" "${API_BASE}" "${CLIENT_API_KEY}" "deepseek-v4-flash-max"; then
  fail=1
fi

echo "==> [5/5] Upstream key pool (local Management)"
if curl -sf -m 5 "${MGMT_BASE}/v1/health" >/dev/null 2>&1; then
  pool_json=$(curl -sf -m 10 "${MGMT_BASE}/v1/upstream/keys" \
    -H "x-gateway-admin-key: ${ADMIN_KEY}" || echo "{}")
  count=$(echo "${pool_json}" | python3 -c "import json,sys; d=json.load(sys.stdin); print(len(d.get('keys',[])))" 2>/dev/null || echo 0)
  if [[ "${count}" -ge 1 ]]; then
    echo "OK  upstream keys listed (count=${count})"
  else
    echo "WARN upstream key pool empty — set CRABCACHE_UPSTREAM_KEYS for production" >&2
  fi
else
  echo "SKIP upstream/keys (Management not reachable at ${MGMT_BASE})"
fi

if [[ "${fail}" -ne 0 ]]; then
  echo "Cursor E2E verification FAILED" >&2
  exit 1
fi
echo "All Cursor E2E checks passed."
