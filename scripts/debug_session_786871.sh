#!/usr/bin/env bash
# Enable NDJSON debug for session 786871 (Codex+MiMo stream disconnect).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${1:-wuming}"
LOG_BASENAME="debug-786871.log"
HOST_LOG="${ROOT}/.cursor/${LOG_BASENAME}"
CONTAINER_LOG="/app/data/${LOG_BASENAME}"

mkdir -p "${ROOT}/.cursor"

echo "[debug-786871] Hot-update gateway to ${TARGET}..."
python3 "${ROOT}/scripts/hot_update.py" --target "${TARGET}" --gateway-only

echo "[debug-786871] Enabling CRABCACHE_DEBUG_* on ${TARGET}..."
ssh "${TARGET}" bash -s <<EOF
set -euo pipefail
cd /opt/projct/CrabCache 2>/dev/null || cd ~/CrabCache
touch .env
grep -q '^CRABCACHE_DEBUG_LOG_PATH=' .env && \
  sed -i 's|^CRABCACHE_DEBUG_LOG_PATH=.*|CRABCACHE_DEBUG_LOG_PATH=${CONTAINER_LOG}|' .env || \
  echo 'CRABCACHE_DEBUG_LOG_PATH=${CONTAINER_LOG}' >> .env
grep -q '^CRABCACHE_DEBUG_SESSION_ID=' .env && \
  sed -i 's|^CRABCACHE_DEBUG_SESSION_ID=.*|CRABCACHE_DEBUG_SESSION_ID=786871|' .env || \
  echo 'CRABCACHE_DEBUG_SESSION_ID=786871' >> .env
grep -q '^CRABCACHE_DEBUG_RUN_ID=' .env && \
  sed -i 's|^CRABCACHE_DEBUG_RUN_ID=.*|CRABCACHE_DEBUG_RUN_ID=pre-fix|' .env || \
  echo 'CRABCACHE_DEBUG_RUN_ID=pre-fix' >> .env
docker compose up -d --force-recreate gateway
for i in 1 2 3 4 5 6 7 8 9 10; do
  curl -sf http://127.0.0.1:9080/v1/ready >/dev/null && break
  sleep 3
done
curl -sf http://127.0.0.1:9080/v1/ready >/dev/null
echo "[debug-786871] Gateway ready; container log=${CONTAINER_LOG}"
EOF

echo "[debug-786871] Pull existing log (if any) to ${HOST_LOG}"
ssh "${TARGET}" "docker exec crabcache-gateway-1 cat ${CONTAINER_LOG} 2>/dev/null" >"${HOST_LOG}" || : >"${HOST_LOG}"

echo "[debug-786871] Done. Reproduce Codex+mimo-v2.5-pro, then run:"
echo "  ssh ${TARGET} docker exec crabcache-gateway-1 cat ${CONTAINER_LOG} > ${HOST_LOG}"
