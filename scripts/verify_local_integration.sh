#!/usr/bin/env bash
# Local smoke checks for gateway and Admin JSON boundaries.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GATEWAY_URL="${GATEWAY_URL:-http://127.0.0.1:8080}"
ADMIN_URL="${ADMIN_URL:-http://127.0.0.1:18001}"

echo "==> Gateway readiness"
curl -fsS "${GATEWAY_URL}/ready" >/dev/null
curl -fsS "${GATEWAY_URL}/health" >/dev/null
echo "OK gateway ready/health"

echo "==> Admin API JSON 404 boundary"
admin_404=$(curl -sS -o /tmp/crabcache-admin-json-404.out -w "%{http_code} %{content_type}" \
  "${ADMIN_URL}/api/admin/__missing_route__" || true)
admin_404_code="${admin_404%% *}"
admin_404_type="${admin_404#* }"
if [[ "${admin_404_code}" != "401" && "${admin_404_code}" != "404" ]]; then
  echo "FAIL: expected Admin API missing route to return 401 or 404, got ${admin_404}" >&2
  exit 1
fi
if [[ "${admin_404_type}" != application/json* ]]; then
  echo "FAIL: expected Admin API missing route Content-Type application/json, got ${admin_404_type}" >&2
  exit 1
fi
echo "OK Admin API missing route returns JSON (${admin_404})"

echo "==> Admin version endpoint JSON boundary"
version_probe=$(curl -sS -o /tmp/crabcache-admin-version.out -w "%{http_code} %{content_type}" \
  "${ADMIN_URL}/api/admin/system/version" || true)
version_code="${version_probe%% *}"
version_type="${version_probe#* }"
if [[ "${version_code}" != "200" && "${version_code}" != "401" ]]; then
  echo "FAIL: expected Admin version endpoint to return 200 or 401, got ${version_probe}" >&2
  exit 1
fi
if [[ "${version_type}" != application/json* ]]; then
  echo "FAIL: expected Admin version Content-Type application/json, got ${version_type}" >&2
  exit 1
fi
echo "OK Admin version endpoint returns JSON (${version_probe})"

if [[ -n "${CLIENT_API_KEY:-}" ]]; then
  echo "==> Full deployment verification"
  GATEWAY_URL="${GATEWAY_URL}" "${ROOT}/scripts/verify_deployment.sh"
else
  echo "SKIP full chat completion check: set CLIENT_API_KEY to run scripts/verify_deployment.sh"
fi

echo "Local integration checks passed."
