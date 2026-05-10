#!/bin/bash

set -e

echo "🦀 CrabCache Admin Dashboard - HTTP Mode"
echo ""

echo "🚀 Starting CrabCache Admin Dashboard..."
echo ""
echo "📍 Access URLs:"
echo "   Local:   http://localhost:3000"
echo "   Network: http://$(hostname -I | awk '{print $1}'):3000"
echo ""

cargo run --release --bin crab-admin
