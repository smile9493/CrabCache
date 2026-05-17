#!/usr/bin/env bash
# Smoke test for Agent middleware deployment (Docker or bare metal).
set -euo pipefail

GATEWAY_URL="${GATEWAY_URL:-http://127.0.0.1:8080}"
CLIENT_API_KEY="${CLIENT_API_KEY:-${CRABCACHE_API_KEY:-}}"

if [[ -z "${CLIENT_API_KEY}" ]]; then
  echo "ERROR: set CLIENT_API_KEY or CRABCACHE_API_KEY (Bearer token for chat API)" >&2
  exit 1
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

cache_status=$(grep -i '^x-cache-status:' "${HDR_FILE}" | tail -1 | awk '{print $2}' | tr -d '\r')
echo "    x-cache-status: ${cache_status:-<missing>}"

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

cache_status=$(grep -i '^x-cache-status:' "${HDR_FILE}" | tail -1 | awk '{print $2}' | tr -d '\r')
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
