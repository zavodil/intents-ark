# NEAR Intents Swap via OutLayer

This project implements token swaps using **NEAR Intents** protocol, executed off-chain via **NEAR OutLayer** platform.

## Architecture

```
User → ft_transfer_call → Intents Contract
                              ↓
                    request_execution → outlayer.near
                              ↓
                    OutLayer Worker (compiles + runs WASI)
                              ↓
                        WASI Binary (Rust + HTTP)
                    1. Get quote from NEAR Intents API
                    2. Publish signed swap intent
                    3. Wait for settlement
                    4. Withdraw tokens to swap contract
                              ↓
                    Callback → Intents Contract
                              ↓
                    ft_transfer → User (tokens out)
```

## Components

### 1. WASI Binary (`src/main.rs`)

- **Target**: `wasm32-wasip2` (requires HTTP support)
- **Input**: JSON with swap parameters (tokens, amounts, sender)
- **Output**: JSON with swap result (success, amount_out, intent_hash)
- **Secrets**: Requires `OPERATOR_PRIVATE_KEY` and `OPERATOR_ACCOUNT_ID` from env vars
- **HTTP**: Makes requests to NEAR Intents API (`https://solver-relay-v2.chaindefuser.com/rpc`)

### 2. Smart Contract (`intents-contract/`)

- **Based on**: [near-intents-onchain](https://github.com/yourusername/near-intents-onchain)
- **Modified for**: OutLayer integration instead of direct yield/resume
- **Key features**:
  - Token whitelist with defuse asset IDs
  - `ft_on_transfer` - accepts tokens and initiates swap
  - `request_execution` - calls OutLayer with secrets
  - `on_swap_result` - callback with result or refund

## Build Instructions

### Build WASI Binary

```bash
# Add target
rustup target add wasm32-wasip2

# Build release
cargo build --target wasm32-wasip2 --release

# Output: target/wasm32-wasip2/release/intents-ark.wasm
```

### Build Contract

```bash
cd intents-contract
./build.sh

# Output: intents-contract/res/intents_contract.wasm
```

## Deployment

### 1. Store Operator Secrets

Store the operator's private key in OutLayer secrets:

```bash
# Using dashboard (http://localhost:3000/secrets)
# Or via contract call:

near call outlayer.near store_secrets '{
  "repo": "github.com/zavodil/intents-ark",
  "branch": "main",
  "profile": "production",
  "encrypted_secrets": [1,2,3,...],  # Encrypted JSON
  "access_condition": {"AllowAll": {}}
}' --accountId operator.testnet --deposit 0.1

# Encrypted JSON format:
# {
#   "OPERATOR_PRIVATE_KEY": "ed25519:...",
#   "OPERATOR_ACCOUNT_ID": "operator.testnet"
# }
```

### 2. Deploy Contract

```bash
near contract deploy intents-swap.testnet \
  use-file intents-contract/res/intents_contract.wasm \
  with-init-call new \
  json-args '{
    "owner_id": "owner.testnet",
    "operator_id": "operator.testnet",
    "secrets_profile": "production"
  }' \
  prepaid-gas '100.0 Tgas' \
  attached-deposit '0 NEAR' \
  network-config testnet \
  sign-with-keychain \
  send
```

### 3. Whitelist Tokens

```bash
# Whitelist WNEAR
near call intents-swap.testnet whitelist_token '{
  "token_id": "wrap.near",
  "symbol": "WNEAR",
  "decimals": 24,
  "defuse_asset_id": "nep141:wrap.near"
}' --accountId owner.testnet

# Whitelist USDC
near call intents-swap.testnet whitelist_token '{
  "token_id": "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1",
  "symbol": "USDC",
  "decimals": 6,
  "defuse_asset_id": "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1"
}' --accountId owner.testnet
```

## Usage

### Execute a Swap

```bash
# User transfers tokens to swap contract with message
near call wrap.near ft_transfer_call '{
  "receiver_id": "intents-swap.testnet",
  "amount": "1000000000000000000000000",
  "msg": "{\"Swap\":{\"token_out\":\"17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1\",\"min_amount_out\":\"900000\"}}"
}' --accountId user.testnet --depositYocto 1 --gas 300000000000000

# Contract will:
# 1. Call OutLayer with WASI repo
# 2. WASI binary executes swap via NEAR Intents
# 3. Callback transfers output tokens to user
```

### Check Configuration

```bash
# Get contract config
near view intents-swap.testnet get_config

# Check if token is whitelisted
near view intents-swap.testnet is_token_whitelisted '{"token_id":"wrap.near"}'

# Get token configuration
near view intents-swap.testnet get_token_config '{"token_id":"wrap.near"}'
```

## Supported Tokens

Based on [NEAR Intents supported tokens](https://defuse.org/):

- **WNEAR**: `wrap.near` → `nep141:wrap.near`
- **USDC**: `17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1`
- **LONK**: `token.lonkingnearbackto2024.near`
- **NEKO**: `ftv2.nekotoken.near`
- **BLACKDRAGON**: `blackdragon.tkn.near`
- **SHITZU**: `token.0xshitzu.near`
- **NEARVIDIA**: `nearnvidia.near`

## Technical Details

### WASI Binary

- **HTTP Client**: `wasi-http-client` crate for API requests
- **Cryptography**: `ed25519-dalek` for NEP-413 signing
- **Borsh**: NEP-413 payload serialization
- **Base58**: Key encoding/decoding (`bs58` crate)

### Contract

- **Gas**: 50 TGas for callback, reserves most gas for OutLayer execution
- **Deposit**: 0.05 NEAR minimum to cover OutLayer costs (refunded to user)
- **Refunds**: Automatic refund on failure (WASI error, insufficient liquidity, etc.)
- **Storage**: Tracks pending swaps until callback completes

### Security

- **Secrets**: Operator private key stored encrypted in OutLayer
- **Access Control**: Only whitelisted tokens can be swapped
- **Pause**: Owner can pause contract in emergency
- **Refunds**: All failed swaps automatically refund input tokens

## Testing

### Local Test (without OutLayer)

```bash
# Build WASI
cargo build --target wasm32-wasip2 --release

# Test with wasmtime (requires secrets as env vars)
export OPERATOR_PRIVATE_KEY="ed25519:..."
export OPERATOR_ACCOUNT_ID="operator.testnet"

echo '{
  "sender_id": "user.testnet",
  "token_in": "nep141:wrap.near",
  "token_out": "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1",
  "amount_in": "1000000000000000000000000",
  "min_amount_out": "900000",
  "swap_contract_id": "intents-swap.testnet"
}' | wasmtime run --wasi preview2 target/wasm32-wasip2/release/intents-ark.wasm
```

### End-to-End Test

1. Deploy contract and whitelist tokens
2. Store operator secrets in OutLayer
3. Execute test swap with small amount
4. Check logs for swap execution
5. Verify user received output tokens

## Troubleshooting

### "OPERATOR_PRIVATE_KEY not found"

- Ensure secrets are stored in OutLayer contract
- Check `secrets_profile` matches in contract config
- Verify `operator_id` is correct

### "Token is not whitelisted"

- Call `whitelist_token` for both input and output tokens
- Check token addresses match exactly

### "Insufficient liquidity"

- Quote from NEAR Intents API returned amount < `min_amount_out`
- Try reducing `min_amount_out` or increasing `amount_in`
- Check liquidity on NEAR Intents platform

### "Intent failed to settle"

- NEAR Intents API timeout (30 seconds)
- Check solver availability on NEAR Intents
- Retry swap later

## Testing

### Test 1: Storage Deposit Check (Testnet)

Tests storage registration for fungible tokens:

```bash
cd ../wasi-test-runner

cargo run --release -- \
  --wasm ../intents-ark/target/wasm32-wasip2/release/intents-ark.wasm \
  --input-file ../intents-ark/test-storage.json \
  --env "SWAP_CONTRACT_ID=your-account.testnet" \
  --env "SWAP_CONTRACT_PRIVATE_KEY=ed25519:YOUR_KEY" \
  --env "NEAR_RPC_URL=https://rpc.testnet.near.org" \
  --max-instructions 50000000000
```

**What it does**:
- Checks if account is registered with `wrap.testnet`
- If not registered: calls `storage_deposit` (costs ~0.00125 NEAR)
- If registered: shows current balance and skips transaction

**Input**: `test-storage.json`
```json
{
  "action": "test_storage",
  "token_contract": "wrap.testnet"
}
```

### Test 2: Full Swap Flow (Mainnet)

Tests complete USDC → WNEAR swap using NEAR Intents API:

```bash
cd ../wasi-test-runner

cargo run --release -- \
  --wasm ../intents-ark/target/wasm32-wasip2/release/intents-ark.wasm \
  --input-file ../intents-ark/test-swap-usdc-wnear.json \
  --env "SWAP_CONTRACT_ID=publishintent.near" \
  --env "SWAP_CONTRACT_PRIVATE_KEY=ed25519:YOUR_KEY" \
  --env "NEAR_RPC_URL=https://rpc.mainnet.near.org" \
  --max-instructions 100000000000
```

**⚠️ Prerequisites**:
1. Swap contract must have at least 0.01 USDC
2. Sender must be registered with wrap.near (storage deposit)
3. Uses **mainnet** (NEAR Intents only works on mainnet)

**What it does**:
1. Pre-flight: Get quote + check storage
2. Deposit 0.01 USDC to intents.near
3. Publish swap intent to NEAR Intents API
4. Wait for settlement (max 30 seconds)
5. Withdraw WNEAR to sender

**Input**: `test-swap-usdc-wnear.json`
```json
{
  "sender_id": "publishintent.near",
  "token_in": "nep141:17208628...36133a1",
  "token_out": "nep141:wrap.near",
  "amount_in": "10000",
  "min_amount_out": "1000000000000000000000",
  "swap_contract_id": "publishintent.near"
}
```

**See**: [TEST_SWAP_FLOW.md](TEST_SWAP_FLOW.md) for detailed testing guide.

## References

- [NEAR OutLayer](https://github.com/your-outlayer-repo)
- [NEAR Intents](https://defuse.org/)
- [WASI Tutorial](../WASI_TUTORIAL.md)
- [Original Python Implementation](https://github.com/yourusername/near-intents-onchain)
- [Full Swap Test Guide](TEST_SWAP_FLOW.md)

## License

MIT
