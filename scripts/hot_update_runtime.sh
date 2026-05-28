#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

GATEWAY_CONTAINER="${GATEWAY_CONTAINER:-crabcache-gateway-1}"
ADMIN_CONTAINER="${ADMIN_CONTAINER:-crabcache-admin}"
GATEWAY_BIN=".cargo-target/release/crab-gateway"
ADMIN_BIN=".cargo-target/release/crab-admin"
DASHBOARD_DIST="crates/crab-dashboard/dist"

log() {
  printf "[hot-update] %s\n" "$*"
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf "Missing required command: %s\n" "$1" >&2
    exit 1
  fi
}

require_cmd docker
require_cmd cargo
require_cmd sha256sum
require_cmd tar

log "Building gateway/admin release binaries"
cargo build --release -p crab-gateway -p crab-admin

if [ ! -x "${GATEWAY_BIN}" ] || [ ! -x "${ADMIN_BIN}" ]; then
  printf "Build finished but binaries are missing under .cargo-target/release\n" >&2
  exit 1
fi

log "Building dashboard static assets"
"${ROOT}/scripts/build_dashboard.sh"

if [ ! -f "${DASHBOARD_DIST}/index.html" ]; then
  printf "Dashboard dist missing index.html: %s\n" "${DASHBOARD_DIST}" >&2
  exit 1
fi

if ! grep -Eq "theme-midnight|theme-ocean|theme-sand" "${DASHBOARD_DIST}/index.html"; then
  printf "Dashboard dist/index.html does not contain expected theme classes\n" >&2
  exit 1
fi

log "Packaging dashboard dist"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT
DIST_TAR="${TMP_DIR}/dashboard-dist.tar"
tar -C "${DASHBOARD_DIST}" -cf "${DIST_TAR}" .

HOST_GW_SHA="$(sha256sum "${GATEWAY_BIN}" | awk '{print $1}')"
HOST_ADMIN_SHA="$(sha256sum "${ADMIN_BIN}" | awk '{print $1}')"

log "Copying artifacts to containers"
docker cp "${GATEWAY_BIN}" "${GATEWAY_CONTAINER}:/app/crab-gateway.new"
docker cp "${ADMIN_BIN}" "${ADMIN_CONTAINER}:/app/crab-admin.new"
docker cp "${DIST_TAR}" "${ADMIN_CONTAINER}:/tmp/dashboard-dist.tar"

log "Swapping gateway binary in container"
docker exec "${GATEWAY_CONTAINER}" sh -lc '
  cp /app/crab-gateway /app/crab-gateway.bak &&
  mv /app/crab-gateway.new /app/crab-gateway &&
  chmod +x /app/crab-gateway
'

log "Swapping admin binary and refreshing dashboard dist"
docker exec "${ADMIN_CONTAINER}" sh -lc '
  cp /app/crab-admin /app/crab-admin.bak &&
  mv /app/crab-admin.new /app/crab-admin &&
  chmod +x /app/crab-admin &&
  rm -rf /app/crates/crab-dashboard/dist &&
  mkdir -p /app/crates/crab-dashboard/dist &&
  tar -C /app/crates/crab-dashboard/dist -xf /tmp/dashboard-dist.tar &&
  rm -f /tmp/dashboard-dist.tar
'

log "Restarting containers"
docker restart "${GATEWAY_CONTAINER}" >/dev/null
docker restart "${ADMIN_CONTAINER}" >/dev/null

log "Waiting for gateway ready endpoint"
READY_OK="false"
for _ in $(seq 1 30); do
  if curl -fsS "http://127.0.0.1:9080/v1/ready" >/dev/null 2>&1; then
    READY_OK="true"
    break
  fi
  sleep 1
done

if [ "${READY_OK}" != "true" ]; then
  printf "Gateway did not become ready in time\n" >&2
  exit 1
fi

log "Verifying runtime checksums"
CTR_GW_SHA="$(docker exec "${GATEWAY_CONTAINER}" sha256sum /app/crab-gateway | awk '{print $1}')"
CTR_ADMIN_SHA="$(docker exec "${ADMIN_CONTAINER}" sha256sum /app/crab-admin | awk '{print $1}')"

if [ "${HOST_GW_SHA}" != "${CTR_GW_SHA}" ]; then
  printf "Gateway checksum mismatch: host=%s container=%s\n" "${HOST_GW_SHA}" "${CTR_GW_SHA}" >&2
  exit 1
fi

if [ "${HOST_ADMIN_SHA}" != "${CTR_ADMIN_SHA}" ]; then
  printf "Admin checksum mismatch: host=%s container=%s\n" "${HOST_ADMIN_SHA}" "${CTR_ADMIN_SHA}" >&2
  exit 1
fi

log "Verifying dashboard homepage"
if ! curl -fsS "http://127.0.0.1:18001/" | grep -Eq "theme-dark|theme-midnight|theme-ocean|theme-sand"; then
  printf "Admin homepage does not contain expected theme markers\n" >&2
  exit 1
fi

log "Hot update completed successfully"
printf "gateway_sha256=%s\nadmin_sha256=%s\n" "${HOST_GW_SHA}" "${HOST_ADMIN_SHA}"
