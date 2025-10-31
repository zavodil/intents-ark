# test a swap USDC->NEAR (details in test-swap-usdc-wnear.json)
# - 1. deposit USDC to SWAP_CONTRACT_ID
# - 2. run test
# - 3. find wNEAR on sender_id

cd wasi-examples/intents-ark
./build.sh

cd wasi-examples/wasi-test-runner
cargo run --release -- \
    --wasm ../intents-ark/target/wasm32-wasip2/release/intents-ark.wasm \
    --input-file ../intents-ark/test-swap-usdc-wnear.json \
    --env "SWAP_CONTRACT_ID=v1.publishintent.near" \
    --env "SWAP_CONTRACT_PRIVATE_KEY=ed25519:..." \
    --env "NEAR_RPC_URL=https://rpc.mainnet.near.org" \
    --max-instructions 100000000000 --verbose