# NEAR Intents Solver Relay API Examples

Source: https://docs.near-intents.org/near-intents/market-makers/bus/solver-relay

## Example 1: Token Diff + Transfer + FT Withdraw

### Request
```json
{
  "id": 1,
  "jsonrpc": "2.0",
  "method": "publish_intent",
  "params": [
    {
      "quote_hashes": ["00000000000000000000000000000000"],
      "signed_data": {
        "standard": "nep413",
        "payload": {
          "message": "{\"signer_id\":\"user.near\",\"deadline\":\"2024-10-14T12:53:40.000Z\",\"intents\":[{\"intent\":\"token_diff\",\"diff\":{\"nep141:ft1.near\":\"300\",\"nep141:ft2.near\":\"-500\"}},{\"intent\":\"transfer\",\"receiver_id\":\"referral.near\",\"tokens\":{\"nep141:ft1.near\":\"1\"}},{\"intent\":\"ft_withdraw\",\"token\":\"ft1.near\",\"receiver_id\":\"ft1.near\",\"amount\":\"299\",\"memo\":\"WITHDRAW_TO:address_on_target_chain\"}]}",
          "nonce": "bacFZfjWD8lm4mwAZ/TScL8HrrapeXlTSyAeD4i8Lfs=",
          "recipient": "intents.near"
        },
        "signature": "ed25519:2yJ1ANYAL1yRoXk8uiDZygyH3TeRpVucwBMpUh1bsvcCLL3BBoJzqAojQNN4mxz9v5fSzbwqz7p9MFtZKNKW81Cg",
        "public_key": "ed25519:4vyWshm6BE4uoHk7fot2iij7tFXrjWp4wDnNEJx2W4sf"
      }
    }
  ]
}
```

### Response
```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "result": {
        "status": "FAILED",
        "reason": "expired",
        "intent_hash": "00000000000000000000000000000000"
    }
}
```

## Intent Format Breakdown

### Token Diff Intent
```json
{
  "intent": "token_diff",
  "diff": {
    "nep141:ft1.near": "300",      // WITH nep141: prefix
    "nep141:ft2.near": "-500"      // WITH nep141: prefix
  }
}
```

### Transfer Intent
```json
{
  "intent": "transfer",
  "receiver_id": "referral.near",
  "tokens": {
    "nep141:ft1.near": "1"         // WITH nep141: prefix
  }
}
```

### FT Withdraw Intent
```json
{
  "intent": "ft_withdraw",
  "token": "ft1.near",             // WITHOUT nep141: prefix ⚠️
  "receiver_id": "ft1.near",
  "amount": "299",
  "memo": "WITHDRAW_TO:address_on_target_chain"
}
```

## Key Differences

| Intent Type | Token Format | Example |
|-------------|--------------|---------|
| `token_diff` | **WITH** `nep141:` prefix | `"nep141:wrap.near"` |
| `transfer` | **WITH** `nep141:` prefix | `"nep141:wrap.near"` |
| `ft_withdraw` | **WITHOUT** `nep141:` prefix | `"wrap.near"` ⚠️ |

## Common Message Structure

```json
{
  "signer_id": "user.near",
  "deadline": "2024-10-14T12:53:40.000Z",
  "intents": [
    // Array of intent objects
  ]
}
```
