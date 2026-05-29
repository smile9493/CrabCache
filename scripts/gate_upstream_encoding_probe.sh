#!/usr/bin/env bash
# Gate check: upstream accepts gzip request bodies and may return compressed responses.
# Run from a host that can reach the gateway upstream base URLs (or via gateway loopback).
#
# Usage:
#   GATEWAY_URL=https://v4.example.com GATEWAY_KEY=sk-cc-... \
#     MIMO_UPSTREAM_URL=https://token-plan-sgp.xiaomimimo.com MIMO_KEY=tp-... \
#     ./scripts/gate_upstream_encoding_probe.sh
#
# Direct upstream (bypass gateway) for MiMo gzip upload compatibility:
#   MIMO_UPSTREAM_URL=https://... MIMO_KEY=tp-... ./scripts/gate_upstream_encoding_probe.sh --direct-mimo

set -euo pipefail

DIRECT_MIMO=false
if [[ "${1:-}" == "--direct-mimo" ]]; then
  DIRECT_MIMO=true
fi

probe_json() {
  local url="$1" key="$2" label="$3"
  local body='{"model":"mimo-v2-flash","messages":[{"role":"user","content":"ping"}],"stream":false,"max_tokens":8}'
  echo "=== $label (identity) ==="
  curl -sfS -o /tmp/gate_probe_out.json -w "http=%{http_code} size=%{size_download}\n" \
    -H "Authorization: Bearer ${key}" \
    -H "Content-Type: application/json" \
    -H "Accept-Encoding: gzip, deflate, br" \
    -d "$body" "${url}/v1/chat/completions" || true
  head -c 120 /tmp/gate_probe_out.json 2>/dev/null || true
  echo
}

probe_gzip_upload() {
  local url="$1" key="$2" label="$3"
  local body='{"model":"mimo-v2-flash","messages":[{"role":"user","content":"ping"}],"stream":false,"max_tokens":8}'
  local gz
  gz=$(echo -n "$body" | gzip -c | wc -c)
  echo "=== $label (gzip upload, ${gz} bytes) ==="
  curl -sfS -o /tmp/gate_probe_gz.json -w "http=%{http_code} size=%{size_download}\n" \
    -H "Authorization: Bearer ${key}" \
    -H "Content-Type: application/json" \
    -H "Content-Encoding: gzip" \
    --data-binary @<(echo -n "$body" | gzip -c) \
    "${url}/v1/chat/completions" || true
  head -c 120 /tmp/gate_probe_gz.json 2>/dev/null || true
  echo
}

if $DIRECT_MIMO; then
  : "${MIMO_UPSTREAM_URL:?}"
  : "${MIMO_KEY:?}"
  probe_json "$MIMO_UPSTREAM_URL" "$MIMO_KEY" "MiMo direct"
  probe_gzip_upload "$MIMO_UPSTREAM_URL" "$MIMO_KEY" "MiMo direct gzip"
  exit 0
fi

: "${GATEWAY_URL:?}"
: "${GATEWAY_KEY:?}"
probe_json "$GATEWAY_URL" "$GATEWAY_KEY" "via gateway"
echo "Enable [features] upstream_request_gzip after direct MiMo gzip probe passes."
