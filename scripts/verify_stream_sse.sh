#!/usr/bin/env bash
# Shared SSE checks for Cursor-compatible streaming (no reasoning_content in body).
# Usage: verify_stream_sse.sh <GATEWAY_BASE_URL> <CLIENT_API_KEY> [model]
# GATEWAY_BASE_URL examples: http://127.0.0.1:8080 or https://domain:18000
set -euo pipefail

BASE="${1:?base URL required}"
KEY="${2:?CLIENT_API_KEY required}"
MODEL="${3:-deepseek-v4-pro}"
CURL_EXTRA="${CURL_EXTRA:-}"

if [[ "${BASE}" == https://* ]]; then
  CURL_EXTRA="${CURL_EXTRA} -sk"
fi

STREAM_FILE=$(mktemp)
trap 'rm -f "${STREAM_FILE}"' EXIT

echo "==> stream SSE (${MODEL})"
http_code=$(curl ${CURL_EXTRA} -m 90 -N -o "${STREAM_FILE}" -w "%{http_code}" \
  -X POST "${BASE}/v1/chat/completions" \
  -H "Authorization: Bearer ${KEY}" \
  -H "Content-Type: application/json" \
  -d "{\"model\":\"${MODEL}\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"max_tokens\":16,\"stream\":true}" || echo 000)

if [[ "${http_code}" != "200" ]]; then
  echo "FAIL stream chat (HTTP ${http_code})" >&2
  exit 1
fi

if grep -q 'reasoning_content' "${STREAM_FILE}"; then
  echo "FAIL stream body contains reasoning_content (rebuild gateway after crab-reasoning fixes)" >&2
  head -5 "${STREAM_FILE}" >&2
  exit 1
fi
echo "OK  stream_sse_no_reasoning_field"

if ! grep -q '\[DONE\]' "${STREAM_FILE}"; then
  echo "FAIL stream missing data: [DONE]" >&2
  exit 1
fi
echo "OK  stream_has_done"

if ! python3 -c "
import json, sys
path = sys.argv[1]
found = False
for line in open(path):
    if not line.startswith('data: '):
        continue
    p = line[6:].strip()
    if p == '[DONE]':
        continue
    o = json.loads(p)
    for ch in o.get('choices', []):
        c = (ch.get('delta') or {}).get('content') or ''
        if c:
            found = True
            break
    if found:
        break
sys.exit(0 if found else 1)
" "${STREAM_FILE}"; then
  echo "FAIL stream has no non-empty delta.content" >&2
  exit 1
fi
echo "OK  stream_first_content"
