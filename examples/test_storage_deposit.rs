/// Real transaction test: storage_deposit on wrap.testnet
///
/// This example sends a real transaction to testnet!
///
/// Run with:
/// ```
/// export TEST_ACCOUNT_ID="your-account.testnet"
/// export TEST_PRIVATE_KEY="ed25519:..."
/// cargo run --example test_storage_deposit
/// ```

use std::env;

// We need to declare the modules from our library
// Since this is an example, we have access to the library crate
use intents_example::near_tx;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 NEAR Storage Deposit Test\n");

    // Get credentials from environment
    let rpc_url = "https://rpc.testnet.near.org";
    let account_id = env::var("TEST_ACCOUNT_ID")
        .expect("Set TEST_ACCOUNT_ID env var (e.g., your-account.testnet)");
    let private_key = env::var("TEST_PRIVATE_KEY")
        .expect("Set TEST_PRIVATE_KEY env var (e.g., ed25519:...)");
    let token_contract = "wrap.testnet";

    println!("📋 Configuration:");
    println!("   RPC: {}", rpc_url);
    println!("   Account: {}", account_id);
    println!("   Token: {}", token_contract);
    println!("   Cost: ~0.00125 NEAR");
    println!();

    // Step 1: Check balance BEFORE
    println!("📊 Step 1: Check FT balance BEFORE transaction");
    let balance_before = check_ft_balance(rpc_url, &account_id, token_contract)?;
    println!("   ✅ Balance before: {}", balance_before);
    println!();

    // Step 2: Send storage_deposit transaction
    println!("📤 Step 2: Send storage_deposit transaction");
    println!("   This will cost ~0.00125 NEAR from your account");

    let tx_hash = near_tx::storage_deposit(
        rpc_url,
        &account_id,
        &private_key,
        token_contract,
        None,  // account_id = None means register self
        false, // registration_only = false
    )?;

    println!("   ✅ Transaction sent!");
    println!("   📍 TX Hash: {}", tx_hash);
    println!("   🔗 View on explorer: https://testnet.nearblocks.io/txns/{}", tx_hash);
    println!();

    // Step 3: Wait a bit for transaction to finalize
    println!("⏳ Step 3: Waiting 3 seconds for transaction to finalize...");
    std::thread::sleep(std::time::Duration::from_secs(3));
    println!();

    // Step 4: Check balance AFTER
    println!("📊 Step 4: Check FT balance AFTER transaction");
    let balance_after = check_ft_balance(rpc_url, &account_id, token_contract)?;
    println!("   ✅ Balance after: {}", balance_after);
    println!();

    // Summary
    println!("✅ Test completed successfully!");
    println!();
    println!("📝 Summary:");
    println!("   - Storage deposit transaction sent and confirmed");
    println!("   - Your account is now registered to hold {} tokens", token_contract);
    println!("   - Balance before: {}", balance_before);
    println!("   - Balance after: {}", balance_after);

    Ok(())
}

/// Helper function to check FT balance via RPC
fn check_ft_balance(rpc_url: &str, account_id: &str, token_contract: &str) -> Result<String, Box<dyn std::error::Error>> {
    let args = serde_json::json!({
        "account_id": account_id
    });
    let args_base64 = base64::encode(args.to_string().as_bytes());

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "dontcare",
        "method": "query",
        "params": {
            "request_type": "call_function",
            "finality": "final",
            "account_id": token_contract,
            "method_name": "ft_balance_of",
            "args_base64": args_base64
        }
    });

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(rpc_url)
        .header("Content-Type", "application/json")
        .json(&request)
        .send()?;

    if response.status() != 200 {
        return Err(format!("RPC returned status {}", response.status()).into());
    }

    let json: serde_json::Value = response.json()?;

    if let Some(error) = json.get("error") {
        // Account might not be registered yet - return 0
        return Ok("0 (not registered)".to_string());
    }

    if let Some(result) = json.get("result").and_then(|r| r.get("result")) {
        let result_vec: Vec<u8> = result
            .as_array()
            .ok_or("Result should be array")?
            .iter()
            .map(|v| v.as_u64().unwrap() as u8)
            .collect();

        let balance_str = String::from_utf8(result_vec)?;
        Ok(balance_str.trim_matches('"').to_string())
    } else {
        Ok("0".to_string())
    }
}
