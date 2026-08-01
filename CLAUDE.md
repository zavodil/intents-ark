# CLAUDE.md - Implementation Notes for intents-example

## 🎯 Purpose
This document describes critical implementation details and common pitfalls when working with NEAR Intents API and NEAR blockchain transactions. **READ THIS BEFORE MAKING CHANGES** to avoid repeating mistakes.

---

## 🏗️ System Architecture (FINAL)

### Complete Flow: User → Swap → Callback

```
┌─────────────────────────────────────────────────────────────────┐
│ 1. user.near → ft_transfer_call → v1.publishintent.near        │
│    - Tokens stay in v1.publishintent.near (liquidity source)   │
│    - msg: {"Swap": {"token_out": "usdc.near", ...}}           │
└─────────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────────┐
│ 2. v1.ft_on_transfer:                                           │
│    - Check whitelist (token_in, token_out)                      │
│    - Get defuse_asset_id from whitelist                         │
│    - Call OutLayer.request_execution() with NEAR deposit        │
│      input_data: {                                              │
│        sender_id: "user.near",                                  │
│        token_in: "nep141:wrap.near",  ← WITH prefix            │
│        token_out: "nep141:usdc.near", ← WITH prefix            │
│        amount_in: "100",                                        │
│        min_amount_out: "50",                                    │
│        swap_contract_id: "v1.publishintent.near"               │
│      }                                                           │
│    - Returns U128(0) to keep all tokens                        │
└─────────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────────┐
│ 3. OutLayer.request_execution:                                  │
│    - Does NOT receive tokens (only metadata)                    │
│    - Creates yield/resume promise                               │
│    - Emits ExecutionRequested event                             │
└─────────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────────┐
│ 4. Worker (event monitor):                                      │
│    - Filters: contract_id == outlayer.near ✅ CRITICAL         │
│    - Compiles WASI (intents-example)                                │
│    - Executes with secrets (SWAP_CONTRACT_PRIVATE_KEY)          │
└─────────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────────┐
│ 5. WASI execution (intents-example):                                │
│    - Reads swap_contract_id from input_data                     │
│    - Uses SWAP_CONTRACT_PRIVATE_KEY to sign as v1              │
│    - ft_transfer_call: v1 → intents.near (deposit)             │
│    - publish_intent to NEAR Intents API                         │
│    - Wait for settlement                                        │
│    - withdraw: intents.near → v1.publishintent.near            │
│    - Returns SwapResponse JSON                                  │
└─────────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────────┐
│ 6. Worker → OutLayer.resolve_execution                          │
│    - OutLayer resumes yield promise                             │
└─────────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────────┐
│ 7. OutLayer → v1.on_execution_response (callback):             │
│    - Parses ExecutionResponse                                   │
│    - Extracts SwapResponse from output.data JSON                │
└─────────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────────┐
│ 8. v1.on_execution_response:                                    │
│    - If success: ft_transfer token_out → user.near             │
│    - If failure: panic (auto-refund token_in → user.near)      │
│    - Future: Take fee from amount_out                           │
└─────────────────────────────────────────────────────────────────┘
```

### Key Points:

✅ **Tokens never leave v1.publishintent.near** until swap completes
✅ **OutLayer doesn't handle tokens** - only coordinates WASI execution
✅ **Worker filters by contract_id** - only processes outlayer.near events
✅ **WASI signs transactions** - using v1's private key from secrets
✅ **On-chain whitelist** - defuse_asset_id stored in contract, not hardcoded

---

## 📋 Token Format Rules (CRITICAL!)

### Token Identifiers in Different Contexts

NEAR Intents API uses **different token formats** depending on the intent type:

| Context | Format | Example | Why |
|---------|--------|---------|-----|
| **token_diff** intent | `nep141:<contract>` | `"nep141:wrap.near"` | Defuse asset identifier format |
| **transfer** intent | `nep141:<contract>` | `"nep141:wrap.near"` | Defuse asset identifier format |
| **ft_withdraw** intent | `<contract>` | `"wrap.near"` | Plain NEAR account ID |
| **ft_transfer_call** args | `<contract>` | `"wrap.near"` | NEAR function call parameter |

### Code Implementation

```rust
// ✅ CORRECT: token_diff uses WITH prefix
let diff = serde_json::json!({
    token_in: format!("-{}", quote.amount_in),  // token_in = "nep141:wrap.near"
    token_out: quote.amount_out.clone()         // token_out = "nep141:17208628..."
});

// ✅ CORRECT: ft_withdraw STRIPS prefix
fn withdraw_tokens(token: &str, ...) {
    let token_without_prefix = if token.starts_with("nep141:") {
        &token[7..]  // "nep141:wrap.near" → "wrap.near"
    } else {
        token
    };

    IntentAction::FtWithdraw {
        token: token_without_prefix.to_string(),  // "wrap.near"
        // ...
    }
}
```

### Common Mistakes (DO NOT DO THIS!)

❌ **WRONG**: Using prefix in ft_withdraw
```rust
// This will cause: "Account ID contains an invalid character ':' at index 6"
IntentAction::FtWithdraw {
    token: "nep141:wrap.near",  // ❌ ERROR!
    // ...
}
```

❌ **WRONG**: Removing prefix from token_diff
```rust
// This will cause: "Matching variant not found" JSON parse error
let diff = serde_json::json!({
    "wrap.near": "-100"  // ❌ ERROR! Should be "nep141:wrap.near"
});
```

---

## 🔐 NEAR Transaction Result Parsing (CRITICAL!)

### The Problem

NEAR RPC's `broadcast_tx_commit` returns a complex `FinalExecutionOutcomeView` structure. **DO NOT** just extract the transaction hash and assume success!

### Historical Bug

**OLD CODE** (INCORRECT):
```rust
// ❌ This is WRONG - always returns success even when transaction fails!
fn send_transaction(...) -> Result<String, ...> {
    let response = /* send tx */;
    let tx_hash = response["transaction"]["hash"].as_str()?;
    eprintln!("✅ Transaction sent: {}", tx_hash);  // ❌ LIES!
    Ok(tx_hash)
}
```

**Example of hidden failure:**
- Transaction hash: `EC6fpanbrY9LUFHj4Ykiy2bfsBbvLqDkgTcmTgPGT3GT`
- Our logs: `✅ Deposit successful`
- **Reality**: `ActionError: Smart contract panicked: The account doesn't have enough balance`

### Solution: Proper Outcome Parsing

**NEW CODE** (CORRECT):
```rust
fn send_transaction(...) -> Result<String, ...> {
    let response = /* send tx */;

    // 1. Parse full FinalExecutionOutcomeView
    let outcome: FinalExecutionOutcomeView = serde_json::from_value(result)?;

    // 2. Check top-level status
    match &outcome.status {
        FinalExecutionStatus::Failure { failure: err } => {
            return Err(format_tx_error(err).into());
        }
        // ...
    }

    // 3. Check transaction_outcome.status
    if let ExecutionStatusView::Failure { failure: err } = &outcome.transaction_outcome.outcome.status {
        return Err(format_tx_error(err).into());
    }

    // 4. Check ALL receipts_outcome[].status
    for (i, receipt) in outcome.receipts_outcome.iter().enumerate() {
        if let ExecutionStatusView::Failure { failure: err } = &receipt.outcome.status {
            return Err(format!("Receipt {} failed: {}", i, format_tx_error(err)).into());
        }
    }

    // Only NOW we can say it succeeded
    Ok(tx_hash)
}
```

### WASI-Compatible near-primitives Types

Since we can't use `near-primitives` crate (WASI incompatibility), we recreated the essential types in `src/near_tx.rs` (lines 327-405):

```rust
// Structures matching near-primitives v0.26 behavior
enum TxExecutionError {
    ActionError { action_error: ActionError },
    InvalidTxError { invalid_tx_error: serde_json::Value },
}

struct ActionError {
    index: Option<u64>,
    kind: ActionErrorKind,
}

enum ActionErrorKind {
    FunctionCallError { function_call_error: FunctionCallErrorKind },
    Other(serde_json::Value),
}

enum FunctionCallErrorKind {
    ExecutionError { execution_error: String },
    Other(serde_json::Value),
}

enum ExecutionStatusView {
    Failure { failure: TxExecutionError },
    SuccessValue { success_value: String },
    SuccessReceiptId { success_receipt_id: String },
}

enum FinalExecutionStatus {
    Failure { failure: TxExecutionError },
    SuccessValue { success_value: String },
    NotStarted,
    Started,
}

struct ExecutionOutcomeView {
    logs: Vec<String>,
    // ... other fields
    status: ExecutionStatusView,
}

struct ExecutionOutcomeWithIdView {
    block_hash: String,
    id: String,
    outcome: ExecutionOutcomeView,
}

struct FinalExecutionOutcomeView {
    status: FinalExecutionStatus,
    transaction_outcome: ExecutionOutcomeWithIdView,
    receipts_outcome: Vec<ExecutionOutcomeWithIdView>,
}
```

### JSON Structure Example

Real RPC response when transaction fails:
```json
{
  "status": {
    "Failure": {
      "ActionError": {
        "index": 0,
        "kind": {
          "FunctionCallError": {
            "ExecutionError": "Smart contract panicked: The account doesn't have enough balance"
          }
        }
      }
    }
  },
  "transaction_outcome": { "outcome": { "status": { ... } } },
  "receipts_outcome": [
    { "outcome": { "status": { "Failure": { ... } } } }
  ]
}
```

---

## 📚 Reference Files Location

All reference documentation is stored in:
```
/Users/alice/projects/near-offshore/wasi-examples/intents-example/near-intents-reference/
/Users/alice/projects/near-offshore/wasi-examples/intents-example/near-primitives-reference/
```

### near-intents-reference/

**File: `solver-relay-examples.md`**
- **Purpose**: Official examples from NEAR Intents documentation
- **Source**: https://docs.near-intents.org/near-intents/market-makers/bus/solver-relay
- **Contains**:
  - Complete request/response examples
  - Intent format breakdown (token_diff, transfer, ft_withdraw)
  - **TOKEN FORMAT TABLE** - shows which intents use `nep141:` prefix
  - Common message structure

**Key sections:**
- ✅ `token_diff` example: `"nep141:ft1.near": "300"` (WITH prefix)
- ✅ `ft_withdraw` example: `"token": "ft1.near"` (WITHOUT prefix)
- ✅ Table showing prefix usage per intent type

**When to use:**
- Before adding new intent types
- When debugging JSON serialization errors
- To verify correct token identifier format

### near-primitives-reference/

**File: `views.rs`**
- **Purpose**: NEAR blockchain data structures for RPC responses
- **Source**: nearcore v0.26 (`/tmp/nearcore/core/primitives/src/views.rs`)
- **Contains**:
  - `FinalExecutionOutcomeView` (lines 2028-2041)
  - `ExecutionStatusView` (lines 1645-1659)
  - `FinalExecutionStatus` (lines 1591-1605)
  - `ExecutionOutcomeView` (lines 1844-1864)
  - `ExecutionOutcomeWithIdView` (lines 1928-1933)

**Key structures:**
```rust
pub enum ExecutionStatusView {
    Unknown = 0,
    Failure(TxExecutionError) = 1,      // ⚠️ Check this!
    SuccessValue(Vec<u8>) = 2,
    SuccessReceiptId(CryptoHash) = 3,
}

pub struct FinalExecutionOutcomeView {
    pub status: FinalExecutionStatus,              // ⚠️ Check this!
    pub transaction_outcome: ExecutionOutcomeWithIdView,  // ⚠️ And this!
    pub receipts_outcome: Vec<ExecutionOutcomeWithIdView>, // ⚠️ And all of these!
}
```

**When to use:**
- Before modifying transaction result parsing
- To understand NEAR RPC response structure
- To ensure our WASI-compatible types match the original

**File: `transaction.rs`**
- **Purpose**: Transaction signing and execution types
- **Source**: nearcore v0.26 (`/tmp/nearcore/core/primitives/src/transaction.rs`)
- **Contains**: Transaction, Action, ExecutionOutcome structures

**File: `errors.rs` (in `/tmp/nearcore/core/primitives/src/`)**
- **Purpose**: Error type definitions
- **Contains**:
  - `TxExecutionError` (lines 30-35)
  - `ActionError`, `InvalidTxError`
  - Error formatting and Display implementations

---

## 🔍 Common Debugging Steps

### 1. Token Format Issues

**Symptom:** `"Account ID contains an invalid character ':' at index 6"`
- **Cause:** Using `nep141:` prefix in ft_withdraw
- **Fix:** Strip prefix before creating IntentAction::FtWithdraw

**Symptom:** `"Matching variant not found"`
- **Cause:** Missing `nep141:` prefix in token_diff
- **Fix:** Use full `nep141:contract` format in diff object

### 2. Transaction Parsing Issues

**Symptom:** Shows "✅ Transaction successful" but block explorer shows failure
- **Cause:** Not checking ExecutionStatusView for Failure variants
- **Fix:** Implement full outcome parsing as shown above

**Symptom:** `"Failed to parse FinalExecutionOutcomeView"`
- **Cause:** Serde structure doesn't match RPC response
- **Fix:** Check `near-primitives-reference/views.rs` for correct structure
- **Debug:** Add `eprintln!("Response: {}", body_str)` to see raw JSON

### 3. JSON Format Issues

**Symptom:** Intent publish returns `"status": "FAILED"`
- **Debug:** Check request body in logs (`📦 Request body`)
- **Compare:** With examples in `near-intents-reference/solver-relay-examples.md`
- **Common issues:**
  - Missing spaces after colons (should be `": "` not `":"`)
  - Wrong token format (see token format rules above)
  - Missing fields (deadline, signer_id, etc.)

---

## ⚠️ DO NOT (Common Mistakes)

1. **DO NOT** add balance checks before sending transactions
   - Adds latency (extra RPC call)
   - Race condition (balance can change)
   - Contract will fail anyway if insufficient balance
   - Better to let it fail and show proper error

2. **DO NOT** trust transaction hash as success indicator
   - Transaction can be included in block but still fail
   - Always parse full FinalExecutionOutcomeView

3. **DO NOT** assume all intents use same token format
   - Check `near-intents-reference/solver-relay-examples.md`
   - token_diff: WITH prefix
   - ft_withdraw: WITHOUT prefix

4. **DO NOT** modify near-primitives structures without checking reference
   - Our WASI-compatible types MUST match nearcore behavior
   - Check `near-primitives-reference/views.rs` first

5. **DO NOT** remove debug logging
   - `eprintln!()` output is captured with `--verbose` flag
   - Critical for debugging production issues
   - Keep transaction response logging

---

## 📖 Related Documentation

- NEAR Intents Solver Relay API: https://docs.near-intents.org/near-intents/market-makers/bus/solver-relay
- NEAR RPC API: https://docs.near.org/api/rpc/introduction
- nearcore primitives: https://github.com/near/nearcore (v0.26)
- NEP-413 (Message Signing): https://github.com/near/NEPs/blob/master/neps/nep-0413.md

---

## 🎓 Testing

Always test with actual mainnet transactions:
```bash
cd /Users/alice/projects/near-offshore/wasi-examples

# Build
cd intents-example
env RUSTFLAGS="--cfg wasmedge --cfg tokio_unstable" cargo build --target wasm32-wasip2 --release

# Test with verbose output
cd ../wasi-test-runner
cargo build --release

cd ..
./wasi-test-runner/target/release/wasi-test \
  --wasm intents-example/target/wasm32-wasip2/release/intents-example.wasm \
  --input-file intents-example/test-swap-usdc-wnear.json \
  --env SWAP_CONTRACT_ID=v1.publishintent.near \
  --env 'SWAP_CONTRACT_PRIVATE_KEY=ed25519:...' \
  --env 'NEAR_RPC_URL=https://rpc.mainnet.near.org' \
  --max-instructions 100000000000 \
  --verbose
```

Check logs for:
- ✅ "Transaction successful" (only if all outcomes succeeded)
- ❌ "Transaction FAILED" with clear error message
- 📋 "Receipt logs" if contract emitted logs

---

**Last Updated:** 2025-10-31
**Status:** Production-ready, swap fully working end-to-end
