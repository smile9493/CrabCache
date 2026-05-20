#!/usr/bin/env bash
# Smoke test for Agent middleware deployment (Docker or bare metal).
set -euo pipefail

GATEWAY_URL="${GATEWAY_URL:-http://127.0.0.1:8080}"
METRICS_URL="${METRICS_URL:-http://127.0.0.1:9090/metrics}"
MGMT_URL="${MGMT_URL:-http://127.0.0.1:9080}"
MGMT_KEY="${CRABCACHE_GATEWAY_ADMIN_KEY:-}"
CLIENT_API_KEY="${CLIENT_API_KEY:-}"

# Auto-create sk-cc-* client key when Management is reachable and CLIENT_API_KEY unset.
if [[ -z "${CLIENT_API_KEY}" && -n "${MGMT_KEY}" ]]; then
  echo "==> CLIENT_API_KEY unset; creating sk-cc-* via Management API"
  create_resp=$(curl -s -w "\n%{http_code}" \
    -X POST "${MGMT_URL}/v1/keys" \
    -H "x-gateway-admin-key: ${MGMT_KEY}" \
    -H "Content-Type: application/json" \
    -d '{"name":"verify-deployment","enabled":true}' || true)
  http_code=$(echo "${create_resp}" | tail -1)
  body=$(echo "${create_resp}" | sed '$d')
  if [[ "${http_code}" == "200" || "${http_code}" == "201" ]]; then
    CLIENT_API_KEY=$(echo "${body}" | sed -n 's/.*"key_full"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)
    if [[ -z "${CLIENT_API_KEY}" ]] && command -v jq >/dev/null 2>&1; then
      CLIENT_API_KEY=$(echo "${body}" | jq -r '.key_full // empty')
    fi
    if [[ -n "${CLIENT_API_KEY}" ]]; then
      echo "OK created client key (preview): ${CLIENT_API_KEY:0:12}..."
    fi
  else
    echo "WARN: could not create client key (HTTP ${http_code}); set CLIENT_API_KEY manually" >&2
  fi
fi

if [[ -z "${CLIENT_API_KEY}" ]]; then
  echo "ERROR: set CLIENT_API_KEY to an sk-cc-* client key (create via Management API)." >&2
  echo "       DeepSeek upstream keys (CRABCACHE_API_KEY / CRABCACHE_UPSTREAM_KEYS) are not valid client tokens." >&2
  exit 1
fi

if [[ "${CLIENT_API_KEY}" == sk-cc-* ]]; then
  echo "OK client key format (sk-cc-*)"
elif [[ -n "${ALLOW_LEGACY_CLIENT_KEY:-}" ]]; then
  echo "WARN: using non sk-cc-* client key (legacy mode)" >&2
else
  echo "ERROR: CLIENT_API_KEY should be sk-cc-* (set ALLOW_LEGACY_CLIENT_KEY=1 to override)" >&2
  exit 1
fi

if [[ -n "${MGMT_KEY}" ]]; then
  echo "==> GET ${MGMT_URL}/v1/status"
  status_body=$(curl -sf -H "x-gateway-admin-key: ${MGMT_KEY}" "${MGMT_URL}/v1/status" || true)
  if [[ -n "${status_body}" ]]; then
    if command -v jq >/dev/null 2>&1; then
      uk_count=$(echo "${status_body}" | jq -r '.upstream_key_count // 0')
      uk_avail=$(echo "${status_body}" | jq -r '.upstream_keys_available // 0')
      echo "    upstream_key_count=${uk_count} upstream_keys_available=${uk_avail}"
      if [[ "${uk_avail}" == "0" && "${uk_count}" != "0" ]]; then
        echo "WARN: all upstream keys unavailable (cooldown or disabled)" >&2
      fi
    else
      echo "OK /v1/status"
    fi
  else
    echo "WARN: /v1/status unreachable" >&2
  fi

  echo "==> GET ${MGMT_URL}/v1/upstream/keys"
  pool_code=$(curl -s -o /dev/null -w "%{http_code}" \
    -H "x-gateway-admin-key: ${MGMT_KEY}" \
    "${MGMT_URL}/v1/upstream/keys" || true)
  if [[ "${pool_code}" == "200" ]]; then
    echo "OK upstream key pool endpoint"
  else
    echo "WARN: /v1/upstream/keys returned ${pool_code}" >&2
  fi
fi

echo "==> GET ${GATEWAY_URL}/ready"
code=$(curl -s -o /dev/null -w "%{http_code}" "${GATEWAY_URL}/ready")
if [[ "${code}" != "200" ]]; then
  echo "FAIL: /ready returned ${code} (expected 200; check Redis and gateway logs)" >&2
  exit 1
fi
echo "OK /ready"

echo "==> GET ${GATEWAY_URL}/health"
code=$(curl -s -o /dev/null -w "%{http_code}" "${GATEWAY_URL}/health")
if [[ "${code}" != "200" ]]; then
  echo "FAIL: /health returned ${code}" >&2
  exit 1
fi
echo "OK /health"

BODY='{"model":"deepseek-v4-pro","messages":[{"role":"user","content":"ping"}],"stream":false,"temperature":0}'
HDR_FILE=$(mktemp)
trap 'rm -f "${HDR_FILE}"' EXIT

echo "==> POST ${GATEWAY_URL}/v1/chat/completions (first request)"
http_code=$(curl -s -D "${HDR_FILE}" -o /dev/null -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/v1/chat/completions" \
  -H "Authorization: Bearer ${CLIENT_API_KEY}" \
  -H "Content-Type: application/json" \
  -d "${BODY}")

if [[ "${http_code}" != "200" ]]; then
  echo "FAIL: chat completions returned ${http_code}" >&2
  exit 1
fi

cache_status=$(grep -i '^x-cache-status:' "${HDR_FILE}" 2>/dev/null | tail -1 | awk '{print $2}' | tr -d '\r' || true)
upstream_key_id=$(grep -i '^x-upstream-key-id:' "${HDR_FILE}" 2>/dev/null | tail -1 | awk '{print $2}' | tr -d '\r' || true)
echo "    x-cache-status: ${cache_status:-<missing>}"
if [[ -n "${upstream_key_id}" ]]; then
  echo "    x-upstream-key-id: ${upstream_key_id}"
else
  echo "    x-upstream-key-id: <missing> (expected on cache miss)"
fi

echo "==> POST ${GATEWAY_URL}/v1/chat/completions (cache repeat)"
http_code=$(curl -s -D "${HDR_FILE}" -o /dev/null -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/v1/chat/completions" \
  -H "Authorization: Bearer ${CLIENT_API_KEY}" \
  -H "Content-Type: application/json" \
  -d "${BODY}")

if [[ "${http_code}" != "200" ]]; then
  echo "FAIL: second request returned ${http_code}" >&2
  exit 1
fi

cache_status=$(grep -i '^x-cache-status:' "${HDR_FILE}" 2>/dev/null | tail -1 | awk '{print $2}' | tr -d '\r' || true)
echo "    x-cache-status: ${cache_status:-<missing>}"

case "${cache_status}" in
  HIT_L0|HIT_L1|HIT_L2)
    echo "OK cache hit on second request"
    ;;
  miss|MISS)
    echo "WARN: second request still miss (cache may need identical body/key; check Redis)" >&2
    ;;
  *)
    echo "WARN: unexpected x-cache-status: ${cache_status}" >&2
    ;;
esac

if [[ "${VERIFY_METRICS:-0}" == "1" ]]; then
  echo "==> GET ${METRICS_URL} (VERIFY_METRICS=1)"
  metrics=$(curl -sf "${METRICS_URL}" || true)
  if echo "${metrics}" | grep -q 'gateway_upstream_key_requests_total'; then
    echo "OK gateway_upstream_key_requests_total present"
    echo "${metrics}" | grep 'gateway_upstream_key_requests_total' | head -5 || true
  else
    echo "WARN: gateway_upstream_key_requests_total not found (no upstream miss yet?)" >&2
  fi
fi

if [[ "${VERIFY_STREAM:-0}" == "1" ]]; then
  echo "==> POST stream smoke (VERIFY_STREAM=1)"
  if ! curl -sf -N -X POST "${GATEWAY_URL}/v1/chat/completions" \
    -H "Authorization: Bearer ${CLIENT_API_KEY}" \
    -H "Content-Type: application/json" \
    -d '{"model":"deepseek-v4-pro","messages":[{"role":"user","content":"hi"}],"stream":true,"max_tokens":8}' \
    | head -c 256 | grep -q .; then
    echo "FAIL: streaming response empty" >&2
    exit 1
  fi
  echo "OK stream smoke"
fi

echo "All deployment checks passed."
