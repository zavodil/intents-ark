# Deployment Guide for NEAR Intents Swap

## Prerequisites

1. **NEAR CLI installed**: `npm install -g near-cli`
2. **Rust toolchain**: `rustup` with `wasm32-wasip2` and `wasm32-unknown-unknown` targets
3. **OutLayer contract deployed**: `outlayer.near` or your own instance
4. **Operator account**: NEAR account with private key for signing intents

## Step 1: Build Components

### Build WASI Binary

```bash
cd /path/to/intents-example

# Add target if not already added
rustup target add wasm32-wasip2

# Build release
cargo build --target wasm32-wasip2 --release

# Output: target/wasm32-wasip2/release/intents-example.wasm (572KB)
```

### Build Contract

```bash
cd intents-contract

# Run build script
./build.sh

# Output: res/intents_contract.wasm (244KB)
```

## Step 2: Push WASI Code to GitHub

OutLayer compiles code from GitHub repositories. Push your WASI code:

```bash
# Create new repo or use existing
git init
git add .
git commit -m "Initial commit: NEAR Intents swap WASI"
git remote add origin https://github.com/YOUR_USERNAME/intents-example
git push -u origin main
```

**Important**: Update the `WASI_PROJECT_ID` constant in `intents-contract/src/lib.rs`:

```rust
const WASI_PROJECT_ID: &str = "your-account.near/intents-swap";
```

The contract resolves the WASM through an **OutLayer project**, not through a GitHub URL, so this
is a project id (`owner/name`) and it does not change when the repository is renamed or forked.
Create the project in the dashboard and upload the built WASM to it; the contract then always runs
the project's active version.

Rebuild the contract after changing the constant.

## Step 3: Store Operator Secrets

The operator account needs to sign NEAR Intents transactions. Store the private key encrypted in OutLayer:

### Option A: Via Dashboard (Recommended)

1. Open http://localhost:3000/secrets (or your OutLayer dashboard URL)
2. Connect wallet
3. Fill in the form:
   - **Project**: `your-account.near/intents-swap` — bind to the project, not the repository,
     so the secrets keep resolving if the repo is renamed or forked
   - **Profile**: `production`
   - **Secrets JSON**:
     ```json
     {
       "OPERATOR_PRIVATE_KEY": "ed25519:YOUR_PRIVATE_KEY_HERE",
       "OPERATOR_ACCOUNT_ID": "operator.testnet"
     }
     ```
   - **Access Condition**: Select `AllowAll` (or whitelist your contract)
4. Click "Encrypt & Store Secrets"
5. Confirm transaction (requires ~0.1 NEAR for storage)

### Option B: Via CLI

Encrypt with the CLI (`outlayer secrets set`) or the dashboard rather than by hand: the keystore
expects ECIES v1 — ephemeral X25519 ECDH, HKDF-SHA256 with info `outlayer-keystore-v1`, then
ChaCha20-Poly1305 — and only the TEE holds the key that can decrypt it. Then store the blob:

```bash
near call outlayer.near store_secrets '{
  "accessor": { "Project": { "project_id": "your-account.near/intents-swap" } },
  "profile": "production",
  "encrypted_secrets_base64": "<blob returned by the keystore>",
  "access": "AllowAll",
  "vault_id": null
}' --accountId operator.testnet --deposit 0.1
```

`vault_id` must be present even when null — near-sdk rejects JSON that omits a required
`Option` field.

## Step 4: Deploy Swap Contract

```bash
cd intents-contract

near contract deploy intents-swap.testnet \
  use-file res/intents_contract.wasm \
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

**Key parameters:**
- `owner_id`: Admin account (can whitelist tokens, pause contract)
- `operator_id`: Account whose secrets are used (must match secrets storage)
- `secrets_profile`: Profile name used in OutLayer secrets (`production` by default)

## Step 5: Whitelist Tokens

Each token needs configuration with its Defuse asset ID. Common tokens:

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

# Whitelist LONK
near call intents-swap.testnet whitelist_token '{
  "token_id": "token.lonkingnearbackto2024.near",
  "symbol": "LONK",
  "decimals": 8,
  "defuse_asset_id": "nep141:token.lonkingnearbackto2024.near"
}' --accountId owner.testnet

# More tokens...
```

**Find Defuse asset IDs**: https://defuse.org/ or check `config.py` in Python implementation.

## Step 6: Test Swap

### Small Test Swap

```bash
# Swap 0.001 WNEAR → USDC
near call wrap.near ft_transfer_call '{
  "receiver_id": "intents-swap.testnet",
  "amount": "1000000000000000000000",
  "msg": "{\"Swap\":{\"token_out\":\"17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1\",\"min_amount_out\":\"900\"}}"
}' --accountId user.testnet --depositYocto 1 --gas 300000000000000
```

### Check Logs

```bash
# View transaction in explorer
near tx-status YOUR_TX_HASH --accountId user.testnet

# Or use NEAR explorer: https://testnet.nearblocks.io/txns/YOUR_TX_HASH
```

Expected flow:
1. User transfers tokens → swap contract
2. Swap contract calls `request_execution` → OutLayer
3. OutLayer worker compiles WASI binary from GitHub
4. OutLayer worker executes WASI with decrypted secrets
5. WASI makes HTTP requests to NEAR Intents API
6. WASI returns result to OutLayer
7. OutLayer calls callback → swap contract
8. Swap contract transfers output tokens → user

## Troubleshooting

### 1. "Token is not whitelisted"

**Solution**: Call `whitelist_token` for both input and output tokens.

```bash
# Check if token is whitelisted
near view intents-swap.testnet is_token_whitelisted '{"token_id":"wrap.near"}'
```

### 2. "OPERATOR_PRIVATE_KEY not found in environment"

**Cause**: Secrets not stored or wrong profile/account.

**Solution**:
- Check secrets are stored: `near view outlayer.near secrets_exist '{"repo":"github.com/...","branch":"main","profile":"production"}'`
- Verify `operator_id` matches account in secrets
- Verify `secrets_profile` matches profile name

### 3. "Insufficient liquidity"

**Cause**: NEAR Intents API quote returned less than `min_amount_out`.

**Solution**:
- Reduce `min_amount_out` (accept more slippage)
- Increase `amount_in` (larger swaps get better rates)
- Try different token pair with more liquidity

### 4. "Intent failed to settle within timeout"

**Cause**: NEAR Intents solvers didn't settle intent within 30 seconds.

**Solution**:
- Retry swap later (solver availability issue)
- Check NEAR Intents platform status
- Reduce swap size

### 5. "Compilation failed" or "OutLayer execution failed"

**Cause**: the project has no active version, or the WASI binary failed to compile.

**Solution**:
- Verify `WASI_PROJECT_ID` in the contract matches an existing project with an active version
- Check the project in the dashboard: a project with no uploaded version has nothing to run
- Test compilation locally: `cargo build --target wasm32-wasip2 --release`
- Check OutLayer worker logs

## Monitoring

### Check Contract Configuration

```bash
near view intents-swap.testnet get_config
```

### View Token Config

```bash
near view intents-swap.testnet get_token_config '{"token_id":"wrap.near"}'
```

### Check Pending Swaps

```bash
near view intents-swap.testnet get_pending_swap '{"request_id":0}'
```

## Mainnet Deployment

1. **Build with mainnet settings**:
   - Update `OUTLAYER_CONTRACT_ID` in contract (use mainnet OutLayer instance)
   - Update token addresses for mainnet
   - Update secrets with mainnet operator account

2. **Deploy contract** to mainnet account

3. **Store secrets** with mainnet operator private key

4. **Whitelist mainnet tokens**:
   - WNEAR: `wrap.near`
   - USDC: `17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1`
   - Others: check defuse.org

5. **Test with small amounts** before announcing

## Cost Estimates

- **Contract deployment**: ~1 NEAR (storage)
- **Secrets storage**: ~0.1 NEAR per secrets entry
- **Per swap execution**: ~0.05 NEAR (OutLayer cost, refunded if unused)
- **Token whitelisting**: ~0.01 NEAR per token

## Security Considerations

1. **Operator private key**: Stored encrypted in OutLayer, only decrypted during WASI execution
2. **Access control**: Only whitelisted tokens can be swapped
3. **Automatic refunds**: Failed swaps automatically refund input tokens
4. **Pause mechanism**: Owner can pause contract in emergency
5. **No custody**: Contract doesn't hold user funds (except during swap execution)

## Updating WASI Code

To update the WASI binary logic:

1. Make changes to `src/main.rs` or `src/crypto.rs`
2. Rebuild: `./build.sh`
3. Upload the new WASM as a version of the project and make it active
4. The contract picks up the active version on the next swap

**Note**: No need to redeploy the contract if only WASI code changed — that is the point of
going through a project rather than pinning a commit.

## Support

- GitHub Issues: https://github.com/YOUR_USERNAME/intents-example/issues
- OutLayer Docs: /path/to/outlayer/docs
- NEAR Intents: https://defuse.org/
