#!/usr/bin/env bash
# Verify ReasoningStore + runtime reasoning config (deepseek-cursor-proxy parity).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MGMT_URL="${CRABCACHE_GATEWAY_CONTROL_URL:-http://127.0.0.1:9080}"
ADMIN_KEY="${CRABCACHE_GATEWAY_ADMIN_KEY:-change-me-in-production}"

echo "== ReasoningStore / runtime check =="
code=$(curl -s -o /dev/null -w "%{http_code}" "${MGMT_URL}/v1/health" || echo "000")
if [[ "${code}" != "200" ]]; then
  echo "FAIL: management API not reachable at ${MGMT_URL} (HTTP ${code})" >&2
  exit 1
fi
echo "OK management health"

runtime=$(curl -sf "${MGMT_URL}/v1/reasoning/runtime" \
  -H "x-gateway-admin-key: ${ADMIN_KEY}" 2>/dev/null || true)
if [[ -n "${runtime}" ]]; then
  strategy=$(echo "${runtime}" | jq -r '.missing_reasoning_strategy // empty')
  echo "    missing_reasoning_strategy=${strategy}"
  if [[ "${strategy}" != "recover" && "${strategy}" != "reject" ]]; then
    echo "WARN: expected recover or reject (got ${strategy})" >&2
  fi
fi

echo "Done."
