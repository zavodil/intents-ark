#!/bin/bash
set -e

cd $(dirname $0)
mkdir -p res/local

echo "Building contract..."

# Build the contract
cargo near build non-reproducible-wasm

# Copy the WASM file to res/local/ (created in parent dir due to workspace)
cp ../target/near/intents_contract/intents_contract.wasm res/local/

# Show file info
echo "✅ Contract built: res/local/intents_contract.wasm"
ls -lh res/local/intents_contract.wasm
