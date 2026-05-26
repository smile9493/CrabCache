#!/bin/bash
set -e

DOCKER_BUILDKIT=1

CACHE_DIR="/opt/projct/CrabCache/.docker-cache"
mkdir -p "$CACHE_DIR"

build_with_cache() {
    local dockerfile="$1"
    local image="$2"
    local target="$3"
    
    docker buildx build \
        --file "$dockerfile" \
        --tag "$image" \
        --cache-from type=local,src="$CACHE_DIR" \
        --cache-to type=local,dest="$CACHE_DIR",mode=max \
        --progress=plain \
        ${target:-} \
        .
}

if [ "$1" = "gateway" ]; then
    build_with_cache "Dockerfile" "crabcache-gateway:latest"
elif [ "$1" = "admin" ]; then
    build_with_cache "Dockerfile.admin" "crabcache-admin:latest"
elif [ "$1" = "all" ]; then
    build_with_cache "Dockerfile" "crabcache-gateway:latest"
    build_with_cache "Dockerfile.admin" "crabcache-admin:latest"
else
    echo "Usage: $0 {gateway|admin|all}"
    echo ""
    echo "Features:"
    echo "  - Uses BuildKit persistent cache at $CACHE_DIR"
    echo "  - Cache is reused across builds, reducing rebuild time"
    echo "  - Cache layers are shared between gateway and admin builds"
    exit 1
fi

echo ""
echo "Build complete! Cache stored at: $CACHE_DIR"
echo "To clean cache: rm -rf $CACHE_DIR"