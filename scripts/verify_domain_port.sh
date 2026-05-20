#!/usr/bin/env bash
# Smoke test for CrabCache published via domain:port (no public 80/443).
# Usage:
#   export DOMAIN=v4.wumingaicg.website
#   export API_PORT=18000
#   export ADMIN_PORT=18010
#   export CLIENT_API_KEY=sk-...
#   bash scripts/verify_domain_port.sh
set -euo pipefail

DOMAIN="${DOMAIN:-v4.wumingaicg.website}"
API_PORT="${API_PORT:-18000}"
ADMIN_PORT="${ADMIN_PORT:-18010}"
CLIENT_API_KEY="${CLIENT_API_KEY:-}"
ADMIN_KEY="${CRABCACHE_ADMIN_KEY:-admin}"
API_BASE="https://${DOMAIN}:${API_PORT}"
ADMIN_BASE="https://${DOMAIN}:${ADMIN_PORT}"

if [[ -z "${CLIENT_API_KEY}" ]]; then
  echo "ERROR: set CLIENT_API_KEY (gateway client Bearer, not upstream DeepSeek key)" >&2
  exit 1
fi

fail=0
check() {
  local name="$1" code="$2" expect="${3:-200}"
  if [[ "${code}" == "${expect}" ]]; then
    echo "OK  ${name} (${code})"
  else
    echo "FAIL ${name} (got ${code}, want ${expect})" >&2
    fail=1
  fi
}

echo "==> ${API_BASE}/ready"
code=$(curl -sk -m 15 -o /dev/null -w "%{http_code}" "${API_BASE}/ready" || echo 000)
check ready "${code}"

echo "==> ${API_BASE}/v1/models"
code=$(curl -sk -m 15 -o /dev/null -w "%{http_code}" \
  -H "Authorization: Bearer ${CLIENT_API_KEY}" \
  "${API_BASE}/v1/models" || echo 000)
check models "${code}"

BODY='{"model":"deepseek-v4-pro","messages":[{"role":"user","content":"domain-port-verify"}],"max_tokens":6,"stream":false}'
echo "==> POST ${API_BASE}/v1/chat/completions"
code=$(curl -sk -m 120 -o /tmp/crabcache_verify_chat.json -w "%{http_code}" \
  -X POST "${API_BASE}/v1/chat/completions" \
  -H "Authorization: Bearer ${CLIENT_API_KEY}" \
  -H "Content-Type: application/json" \
  -d "${BODY}" || echo 000)
check chat "${code}"
if python3 -c "import json; json.load(open('/tmp/crabcache_verify_chat.json'))" 2>/dev/null; then
  echo "OK  chat response is single JSON"
else
  echo "FAIL chat response is not valid single JSON" >&2
  fail=1
fi

echo "==> ${ADMIN_BASE}/"
code=$(curl -sk -m 15 -o /dev/null -w "%{http_code}" "${ADMIN_BASE}/" || echo 000)
check dashboard "${code}"

echo "==> ${ADMIN_BASE}/api/admin/gateway/health"
code=$(curl -sk -m 15 -o /dev/null -w "%{http_code}" \
  -H "x-admin-key: ${ADMIN_KEY}" \
  "${ADMIN_BASE}/api/admin/gateway/health" || echo 000)
check admin_health "${code}"

if [[ "${fail}" -ne 0 ]]; then
  echo "Domain/port verification FAILED" >&2
  exit 1
fi
echo "All domain/port checks passed."
