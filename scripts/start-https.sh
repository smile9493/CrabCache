#!/bin/bash

set -e

echo "🦀 CrabCache Admin Dashboard - HTTPS Mode"
echo ""

CERT_DIR="./certs"
CERT_FILE="$CERT_DIR/cert.pem"
KEY_FILE="$CERT_DIR/key.pem"

if [ ! -f "$CERT_FILE" ] || [ ! -f "$KEY_FILE" ]; then
    echo "⚠️  SSL certificate not found. Generating self-signed certificate..."
    ./scripts/generate-cert.sh localhost
    echo ""
fi

echo "🚀 Starting CrabCache Admin Dashboard with HTTPS..."
echo ""
echo "📍 Access URLs:"
echo "   Local:   https://localhost:3000"
echo "   Network: https://$(hostname -I | awk '{print $1}'):3000"
echo ""
echo "⚠️  Note: Your browser will show a security warning because the certificate is self-signed."
echo "   Click 'Advanced' -> 'Proceed to localhost (unsafe)' to continue."
echo ""

export CRABCACHE_HTTPS=1
cargo run --release --bin crab-admin -- --https
