# Test Withdraw from intents.near

This test withdraws tokens from `intents.near` internal balance back to the swap contract.

## Prerequisites

1. The swap contract (`v1.publishintent.near`) must have deposited tokens to `intents.near`
2. You can check the balance on nearblocks.io or using NEAR CLI

## Test Steps

### 1. Check current balance on intents.near

```bash
# View internal balance of wrap.near for v1.publishintent.near on intents.near
# (This is done via intents.near contract view methods or nearblocks.io)
```

### 2. Build the WASI binary

```bash
cd wasi-examples/intents-example
./build.sh
```

### 3. Run the withdraw test

```bash
cd wasi-examples/wasi-test-runner
cargo run --release -- \
    --wasm ../intents-example/target/wasm32-wasip2/release/intents-example.wasm \
    --input-file ../intents-example/test-withdraw-wnear.json \
    --env "SWAP_CONTRACT_ID=v1.publishintent.near" \
    --env "SWAP_CONTRACT_PRIVATE_KEY=ed25519:..." \
    --env "NEAR_RPC_URL=https://rpc.mainnet.near.org" \
    --max-instructions 100000000000 --verbose
```

### 4. Verify the result

- Check that the output shows `"success": true`
- Verify on nearblocks.io that tokens were transferred from intents.near to v1.publishintent.near
- Look for `ft_withdraw` event in the transaction logs

## Example Output

```json
{
  "success": true,
  "amount_out": "1000000000000000000000",
  "error_message": null,
  "intent_hash": null
}
```

## Test File Details

**File**: `test-withdraw-wnear.json`

```json
{
  "action": "withdraw",
  "token": "wrap.near",
  "receiver_id": "v1.publishintent.near",
  "amount": "1000000000000000000000",
  "swap_contract_id": "v1.publishintent.near"
}
```

**Parameters**:
- `action`: "withdraw" - triggers withdraw mode
- `token`: "wrap.near" - token contract address (WITHOUT nep141: prefix)
- `receiver_id`: "v1.publishintent.near" - where to send tokens (swap contract)
- `amount`: "1000000000000000000000" - amount to withdraw (1000 wNEAR)
- `swap_contract_id`: "v1.publishintent.near" - the signer account

## Notes

- The `token` field should be the raw contract address (e.g., "wrap.near"), NOT the defuse format ("nep141:wrap.near")
- The withdraw will create a `ft_withdraw` intent on intents.near
- Settlement typically takes 1-10 seconds
- The WASI binary will wait up to 30 seconds for settlement
