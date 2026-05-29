#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

usage() {
  cat <<'EOF'
Usage:
  scripts/hot_update.sh gateway
  scripts/hot_update.sh admin
  scripts/hot_update.sh all

What it does:
  1) Builds the release binary on the host (cargo build --release -p <crate>)
  2) Copies the binary into the running container at /app/<binary>
  3) Restarts the service via docker compose

Notes:
  - `admin` service is under docker compose profile "admin". Start it with:
      docker compose --profile admin up -d admin postgres
EOF
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || { echo "missing required command: $1" >&2; exit 1; }
}

need_cmd docker
need_cmd cargo

copy_into_container() {
  local service="$1"
  local container_id="$2"
  local host_bin="$3"
  local container_path="$4"

  echo "[hot_update] copy $service -> $container_path"
  docker cp "$host_bin" "${container_id}:${container_path}"
  docker exec "$container_id" chmod +x "$container_path"
}

hot_update_gateway() {
  echo "[hot_update] build crab-gateway (release)"
  cargo build --release -p crab-gateway

  local cid
  cid="$(docker compose ps -q gateway)"
  if [[ -z "$cid" ]]; then
    echo "gateway container not running" >&2
    exit 1
  fi

  copy_into_container "gateway" "$cid" "./.cargo-target/release/crab-gateway" "/app/crab-gateway"

  echo "[hot_update] restart gateway"
  docker compose restart gateway
}

hot_update_admin() {
  echo "[hot_update] build crab-admin (release)"
  cargo build --release -p crab-admin

  local cid
  cid="$(docker compose ps -q admin)"
  if [[ -z "$cid" ]]; then
    echo "admin container not running (start with: docker compose --profile admin up -d admin postgres)" >&2
    exit 1
  fi

  copy_into_container "admin" "$cid" "./.cargo-target/release/crab-admin" "/app/crab-admin"

  echo "[hot_update] restart admin"
  docker compose restart admin
}

main() {
  if [[ $# -ne 1 ]]; then
    usage
    exit 2
  fi
  case "$1" in
    gateway) hot_update_gateway ;;
    admin) hot_update_admin ;;
    all)
      hot_update_gateway
      # Only attempt admin if it's running.
      if [[ -n "$(docker compose ps -q admin 2>/dev/null || true)" ]]; then
        hot_update_admin
      else
        echo "[hot_update] admin not running; skipped"
      fi
      ;;
    -h|--help|help) usage ;;
    *)
      echo "unknown target: $1" >&2
      usage
      exit 2
      ;;
  esac
}

main "$@"

