#!/bin/bash

set -e

CERT_DIR="./certs"
DOMAIN="${1:-localhost}"
DAYS="${2:-365}"

mkdir -p "$CERT_DIR"

echo "🔐 Generating self-signed certificate for $DOMAIN..."

openssl req -x509 \
    -newkey rsa:4096 \
    -keyout "$CERT_DIR/key.pem" \
    -out "$CERT_DIR/cert.pem" \
    -days "$DAYS" \
    -nodes \
    -subj "/C=CN/ST=Beijing/L=Beijing/O=CrabCache/OU=Development/CN=$DOMAIN" \
    -addext "subjectAltName=DNS:$DOMAIN,DNS:localhost,IP:127.0.0.1"

chmod 600 "$CERT_DIR/key.pem"
chmod 644 "$CERT_DIR/cert.pem"

echo "✅ Certificate generated successfully!"
echo ""
echo "📁 Certificate files:"
echo "   - Certificate: $CERT_DIR/cert.pem"
echo "   - Private Key: $CERT_DIR/key.pem"
echo ""
echo "📅 Valid for: $DAYS days"
echo ""
echo "💡 Next steps:"
echo "   1. Import certificate to your browser/system trust store"
echo "   2. Start CrabCache with HTTPS enabled"
echo ""
echo "🔧 To import certificate (Linux):"
echo "   sudo cp $CERT_DIR/cert.pem /usr/local/share/ca-certificates/crabcache.crt"
echo "   sudo update-ca-certificates"
echo ""
echo "🔧 To import certificate (macOS):"
echo "   sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain $CERT_DIR/cert.pem"
echo ""
echo "🔧 To import certificate (Windows):"
echo "   certutil -addstore -f \"ROOT\" $CERT_DIR/cert.pem"
