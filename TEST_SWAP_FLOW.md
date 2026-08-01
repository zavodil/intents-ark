# NEAR Intents Swap Flow Test

This test simulates a real USDC → WNEAR swap using NEAR Intents API, equivalent to the Python test in `/Users/alice/projects/near-intents-onchain/worker/test_swap_flow.py`.

## Test Scenario

**Swap**: 0.01 USDC → WNEAR (minimum)

- **Token In**: USDC (`17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1`)
- **Token Out**: Wrapped NEAR (`wrap.near`)
- **Amount In**: `10000` (0.01 USDC with 6 decimals)
- **Min Amount Out**: `1000000000000000000000` (minimum WNEAR expected with 24 decimals)
- **Sender**: `publishintent.near`

## Expected Flow

The test will execute these steps:

1. ✅ **Pre-flight checks**:
   - Get quote from NEAR Intents API
   - Verify quote amount_out >= min_amount_out
   - Check sender has storage deposit for wrap.near

2. ✅ **Deposit**: Transfer USDC from swap contract to intents.near
   - Calls `ft_transfer_call` on USDC contract
   - Sends to `intents.near`

3. ✅ **Publish Intent**: Sign and publish swap intent to NEAR Intents API
   - NEP-413 signing
   - Submit to solver relay API

4. ✅ **Wait for Settlement**: Poll intent status (max 30 seconds)
   - Status polling every 1 second
   - Success when status = "SETTLED"

5. ✅ **Withdraw**: Transfer WNEAR from intents.near back to sender
   - Calls withdraw intent on intents.near
   - Sends WNEAR to original sender (`publishintent.near`)

## Prerequisites

### 1. Swap Contract Must Have USDC

The swap contract (`publishintent.near` or your test account) must have at least 0.01 USDC deposited:

```bash
# Check USDC balance
near view 17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1 \
  ft_balance_of '{"account_id":"publishintent.near"}'

# If balance is 0, you need to deposit USDC first
# (Use a USDC faucet or transfer from another account)
```

### 2. Sender Must Have Storage for WNEAR

The sender must be registered with wrap.near:

```bash
# Check storage
near view wrap.near \
  storage_balance_of '{"account_id":"publishintent.near"}'

# If null, register:
near call usdt.tether-token.near storage_deposit \
  '{"account_id":"v1.publishintent.near"}' \
  --accountId v1.publishintent.near \
  --deposit 0.00125
```

near call v1.publishintent.near whitelist_token '{ "token_id": "usdt.tether-token.near", "min_swap_amount": "1"}' --accountId publishintent.near

swap

near call usdt.tether-token.near ft_transfer_call '{"receiver_id":"v1.publishintent.near", "amount":"10000","msg":"{\"Swap\":{\"token_out\":\"wrap.near\", \"min_amount_out\":\"1000000000000000000\"}}"}' --accountId zavodil.near --depositYocto 1 --gas 200000000000000

### 3. Environment Variables

Set these in your environment or pass via `--env`:

- `SWAP_CONTRACT_ID`: Your swap contract account (e.g., `publishintent.near`)
- `SWAP_CONTRACT_PRIVATE_KEY`: Private key for the swap contract
- `NEAR_RPC_URL`: NEAR RPC endpoint (default: `https://rpc.mainnet.near.org`)

## Running the Test

### Option 1: Using wasi-test-runner (Recommended)

```bash
cd /Users/alice/projects/near-offshore/wasi-examples/wasi-test-runner

cargo run --release -- \
  --wasm ../intents-example/target/wasm32-wasip2/release/intents-example.wasm \
  --input-file ../intents-example/test-swap-usdc-wnear.json \
  --env "SWAP_CONTRACT_ID=publishintent.near" \
  --env "SWAP_CONTRACT_PRIVATE_KEY=ed25519:YOUR_PRIVATE_KEY" \
  --env "NEAR_RPC_URL=https://rpc.mainnet.near.org" \
  --max-instructions 100000000000
```

**Note**: This test runs on **mainnet** because NEAR Intents only works on mainnet!

### Option 2: Direct Input via stdin

```bash
cd /Users/alice/projects/near-offshore/wasi-examples/intents-example

# Export env vars
export SWAP_CONTRACT_ID="publishintent.near"
export SWAP_CONTRACT_PRIVATE_KEY="ed25519:YOUR_KEY"
export NEAR_RPC_URL="https://rpc.mainnet.near.org"

# Run via wasmtime (if you have it installed)
cat test-swap-usdc-wnear.json | wasmtime run \
  --wasi preview2 \
  --env SWAP_CONTRACT_ID \
  --env SWAP_CONTRACT_PRIVATE_KEY \
  --env NEAR_RPC_URL \
  target/wasm32-wasip2/release/intents-example.wasm
```

## Expected Output

### Success Case

```
Processing swap for publishintent.near: 10000 nep141:17208...133a1 → 1000000000000000000000 nep141:wrap.near

🔄 Quote API attempt 1/3
✅ Quote received successfully
✅ Quote received: 2500000000000000000000 out, expires at 2025-10-31T15:30:00.000Z

Step 1.5: Checking storage deposit for output token...
✅ Storage deposit verified for publishintent.near

Step 2: Depositing 10000 to intents.near
📤 Calling ft_transfer_call: 10000 17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1 from publishintent.near to intents.near
✅ Deposit successful: ABC123...DEF456
   🔗 View on explorer: https://nearblocks.io/txns/ABC123...DEF456

Step 3: Publishing swap intent to NEAR Intents API
   Swap: 10000 nep141:17208...133a1 → 2500000000000000000000 nep141:wrap.near
✅ Intent published successfully
   Intent hash: 0x789...abc

Step 4: Waiting for intent settlement (max 30 seconds)...
Intent 0x789...abc status: PENDING
Intent 0x789...abc status: SETTLED
✅ Intent settled successfully!

Step 5: Withdrawing 2500000000000000000000 nep141:wrap.near to publishintent.near
✅ Withdrawal successful!
🎉 Swap completed successfully: 10000 nep141:17208...133a1 → 2500000000000000000000 nep141:wrap.near

{"success":true,"amount_out":"2500000000000000000000","error_message":null,"intent_hash":"0x789...abc"}
```

### Failure Cases

#### Insufficient Liquidity
```
✅ Quote received: 500000000000000000000 out, expires at ...
❌ Pre-flight check failed: Insufficient liquidity: 500000000000000000000 < 1000000000000000000000

{"success":false,"amount_out":null,"error_message":"Insufficient liquidity: ...","intent_hash":null}
```

#### No Storage Deposit
```
Step 1.5: Checking storage deposit for output token...
❌ Pre-flight check failed: sender publishintent.near has no storage deposit for wrap.near

{"success":false,"amount_out":null,"error_message":"User publishintent.near has no storage deposit for output token wrap.near...","intent_hash":null}
```

#### Intent Settlement Timeout
```
Step 4: Waiting for intent settlement (max 30 seconds)...
Intent 0x789...abc status: PENDING
Intent 0x789...abc status: PENDING
...
❌ Intent failed to settle within 30 second timeout
   Intent hash: 0x789...abc

{"success":false,"amount_out":null,"error_message":"Intent failed to settle within timeout","intent_hash":"0x789...abc"}
```

## Verification After Test

### 1. Check Sender's WNEAR Balance

```bash
near view wrap.near \
  ft_balance_of '{"account_id":"publishintent.near"}'
```

Should show increased WNEAR balance (approximately +0.0025 WNEAR for 0.01 USDC)

### 2. Check Transaction on Explorer

Use the transaction hash from the output:
- Deposit TX: `https://nearblocks.io/txns/[deposit_tx_hash]`
- Check intent settlement on NEAR Intents API

### 3. Check USDC Balance (should decrease)

```bash
near view 17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1 \
  ft_balance_of '{"account_id":"publishintent.near"}'
```

Should show decreased USDC balance (-10000 = -0.01 USDC)

## Comparison with Python Test

| Aspect | Python Test | Rust WASI Test | Status |
|--------|-------------|----------------|---------|
| Swap tokens | USDC → WNEAR | USDC → WNEAR | ✅ Same |
| Amount | 0.01 USDC | 0.01 USDC | ✅ Same |
| Quote check | ✅ | ✅ | ✅ Same |
| Storage check | ✅ | ✅ | ✅ Same |
| Deposit to intents | ✅ | ✅ | ✅ Same |
| Publish intent | ✅ | ✅ | ✅ Same |
| Wait settlement | ✅ | ✅ | ✅ Same |
| Withdraw | ✅ | ✅ | ✅ Same |
| Skip resolve_swap | ✅ (test override) | ✅ (standalone WASI) | ✅ Same |

**Result**: 100% functional equivalence with Python test! 🎯

## Troubleshooting

### "No valid quotes"
- NEAR Intents API might be down or rate limiting
- Try again in a few seconds
- Check if tokens are supported by NEAR Intents

### "Failed to deposit to NEAR Intents"
- Check USDC balance is sufficient
- Verify USDC contract address is correct
- Check RPC connectivity

### "Intent failed to settle"
- Intents API might be slow or congested
- Check intent hash on NEAR Intents explorer (if available)
- Try again with different amount

### "Failed to withdraw"
- Settlement might have failed on solver side
- Check intents.near contract for your balance
- Manual withdrawal might be needed

## Notes

- This test uses **real tokens** on **mainnet**
- Small amounts are used to minimize cost (~$0.01 USD)
- Test does NOT call `resolve_swap` on the contract (WASI is standalone)
- In production, OutLayer coordinator would handle the full flow including contract resolution
