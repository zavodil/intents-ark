# Integration Testing Guide

## Prerequisites

1. **NEAR Testnet Account** with some balance
2. **Private Key** for the account
3. **Rust toolchain** installed

## Setup Test Environment

### 1. Create Test Account (if needed)

```bash
# Install NEAR CLI
npm install -g near-cli

# Create testnet account
near create-account test-swap-$(date +%s).testnet --useFaucet
```

### 2. Export Test Credentials

```bash
# Export account ID
export TEST_ACCOUNT_ID="your-account.testnet"

# Export private key (get from ~/.near-credentials/testnet/your-account.testnet.json)
export TEST_PRIVATE_KEY="ed25519:..."

# Optional: specify token contract for FT tests
export TEST_TOKEN_CONTRACT="wrap.testnet"
```

## Running Tests

### Test 1: Read Access Key (Check RPC + Key Derivation)

```bash
cargo test --test near_tx_integration test_get_access_key_testnet -- --ignored --nocapture
```

**Expected output:**
```
🔍 Testing access key query for account: test-swap.testnet
📍 RPC: https://rpc.testnet.near.org
🔑 Public key: ed25519:...
📦 RPC Response: {
  "result": {
    "nonce": 12345,
    "block_hash": "abc123...",
    ...
  }
}
✅ Access key found!
   Nonce: 12345
   Block hash: abc123...
```

**What this tests:**
- ✅ Private key decoding (base58)
- ✅ Public key derivation (ed25519-dalek)
- ✅ NEAR RPC connection (testnet)
- ✅ Access key query

### Test 2: View Account Balance

```bash
cargo test --test near_tx_integration test_view_account_testnet -- --ignored --nocapture
```

**Expected output:**
```
🔍 Testing view account: test-swap.testnet
📦 RPC Response: ...
✅ Account found!
   Balance: 5000000000000000000000000 yoctoNEAR
   Storage: 182 bytes
```

**What this tests:**
- ✅ Account existence on testnet
- ✅ Balance reading via RPC

### Test 3: Check Token Balance

```bash
cargo test --test near_tx_integration test_ft_balance_of_testnet -- --ignored --nocapture
```

**Expected output:**
```
🔍 Testing FT balance for test-swap.testnet on wrap.testnet
📦 RPC Response: ...
✅ Token balance: 1000000000000000000000000 ("1000000000000000000000000")
```

**What this tests:**
- ✅ FT contract view call
- ✅ Args base64 encoding
- ✅ Result parsing

### Test 4: Transaction Structure Validation

```bash
cargo test --test near_tx_integration test_send_near_transfer_testnet -- --ignored --nocapture
```

**What this tests:**
- ✅ Transaction parameters validation
- ⚠️  Note: This is a dry-run, doesn't send actual transaction

## Full Test Suite

Run all integration tests:

```bash
cargo test --test near_tx_integration -- --ignored --nocapture
```

## Troubleshooting

### Error: "Account not found"

Your test account doesn't exist on testnet. Create one:

```bash
near create-account test-swap-$(date +%s).testnet --useFaucet
```

### Error: "Failed to decode private key"

Check that `TEST_PRIVATE_KEY` is in correct format:
- With prefix: `ed25519:base58string`
- Without prefix: `base58string`

Find your private key:
```bash
cat ~/.near-credentials/testnet/your-account.testnet.json | jq -r '.private_key'
```

### Error: "No access key found"

The public key derived from your private key doesn't have access to the account. Make sure you're using the correct private key for the account.

### Error: "RPC returned status 429"

Rate limit exceeded. Wait a few seconds and try again.

## Testing with Real WASI Binary

To test the actual WASI binary (requires wasmtime with WASI P2):

```bash
# Build WASI
cargo build --target wasm32-wasip2 --release

# Prepare test input
cat > /tmp/test_input.json <<EOF
{
  "sender_id": "user.near",
  "token_in": "nep141:wrap.testnet",
  "token_out": "nep141:usdc.testnet",
  "amount_in": "1000000000000000000000000",
  "min_amount_out": "900000",
  "swap_contract_id": "test-swap.testnet"
}
EOF

# Set environment (secrets)
export SWAP_CONTRACT_PRIVATE_KEY="ed25519:..."
export SWAP_CONTRACT_ID="test-swap.testnet"
export NEAR_RPC_URL="https://rpc.testnet.near.org"

# Run WASI (requires wasmtime 15+ with WASI P2 support)
wasmtime run \
  --wasi preview2 \
  --env SWAP_CONTRACT_PRIVATE_KEY \
  --env SWAP_CONTRACT_ID \
  --env NEAR_RPC_URL \
  target/wasm32-wasip2/release/intents-example.wasm < /tmp/test_input.json
```

## Next Steps

Once tests pass, you can:

1. ✅ Deploy swap contract to testnet
2. ✅ Store secrets in OutLayer (testnet)
3. ✅ Test end-to-end swap flow
4. ✅ Deploy to mainnet

## Test Checklist

- [ ] `test_get_access_key_testnet` - RPC works, keys derive correctly
- [ ] `test_view_account_testnet` - Can read account data
- [ ] `test_ft_balance_of_testnet` - Can query token balances
- [ ] WASI binary compiles for `wasm32-wasip2`
- [ ] Contract compiles for `wasm32-unknown-unknown`
- [ ] End-to-end test on testnet (manual)
