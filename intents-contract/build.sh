#!/bin/bash
set -e

cd "$(dirname $0)"

TARGET="${CARGO_TARGET_DIR:-../target}"

rustup target add wasm32-unknown-unknown

cargo build --target wasm32-unknown-unknown --release

mkdir -p res

cp $TARGET/wasm32-unknown-unknown/release/intents_contract.wasm res/

echo "✅ Contract built: res/intents_contract.wasm"
ls -lh res/intents_contract.wasm
