#!/bin/bash
set -e

cd "$(dirname $0)"

echo "🔨 Building intents-example WASI binary..."
echo ""

# Add target
rustup target add wasm32-wasip2

# Build release with required flags
echo "📦 Building with RUSTFLAGS for WASI P2 + HTTP support..."
cargo build --target wasm32-wasip2 --release

echo ""
echo "✅ Build complete!"
echo ""
echo "📦 Binary location:"
echo "   target/wasm32-wasip2/release/intents-example.wasm"
echo ""
ls -lh target/wasm32-wasip2/release/intents-example.wasm
