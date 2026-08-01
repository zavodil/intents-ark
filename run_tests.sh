#!/bin/bash
set -e

echo "🧪 NEAR Integration Tests for intents-example"
echo "=========================================="
echo ""

# Check if credentials are set
if [ -z "$TEST_ACCOUNT_ID" ]; then
    echo "❌ TEST_ACCOUNT_ID not set!"
    echo ""
    echo "Please set test credentials:"
    echo "  export TEST_ACCOUNT_ID=\"your-account.testnet\""
    echo "  export TEST_PRIVATE_KEY=\"ed25519:...\""
    echo ""
    echo "To create a testnet account:"
    echo "  near create-account test-swap-\$(date +%s).testnet --useFaucet"
    echo ""
    exit 1
fi

if [ -z "$TEST_PRIVATE_KEY" ]; then
    echo "❌ TEST_PRIVATE_KEY not set!"
    echo ""
    echo "Find your private key:"
    echo "  cat ~/.near-credentials/testnet/$TEST_ACCOUNT_ID.json | jq -r '.private_key'"
    echo ""
    exit 1
fi

echo "✅ Test account: $TEST_ACCOUNT_ID"
echo "✅ Private key: ${TEST_PRIVATE_KEY:0:15}..."
echo ""

# Run tests
echo "📍 Test 1: Access Key Query (RPC + Key Derivation)"
echo "---------------------------------------------------"
cargo test --test near_tx_integration test_get_access_key_testnet -- --ignored --nocapture
echo ""

echo "📍 Test 2: View Account Balance"
echo "---------------------------------------------------"
cargo test --test near_tx_integration test_view_account_testnet -- --ignored --nocapture
echo ""

echo "📍 Test 3: FT Balance Query"
echo "---------------------------------------------------"
cargo test --test near_tx_integration test_ft_balance_of_testnet -- --ignored --nocapture
echo ""

echo "📍 Test 4: Transaction Structure"
echo "---------------------------------------------------"
cargo test --test near_tx_integration test_send_near_transfer_testnet -- --ignored --nocapture
echo ""

echo "=========================================="
echo "✅ All tests completed!"
echo ""
echo "Next steps:"
echo "  1. Deploy contract: cd intents-contract && ./build.sh"
echo "  2. Store secrets in OutLayer"
echo "  3. Test end-to-end swap"
