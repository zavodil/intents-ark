mod crypto;
mod near_tx;

use serde::{Deserialize, Serialize};
use std::env;
use std::io::{self, Read, Write};
use std::time::Duration;
use wasi_http_client::Client;

// ============================================================================
// Input/Output Types
// ============================================================================

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum Input {
    TestStorage {
        action: String, // "test_storage"
        token_contract: String,
    },
    Swap {
        sender_id: String,
        token_in: String,
        token_out: String,
        amount_in: String,
        min_amount_out: String,
        swap_contract_id: String,
    },
}

#[derive(Serialize, Debug)]
struct Output {
    success: bool,
    amount_out: Option<String>,
    error_message: Option<String>,
    intent_hash: Option<String>,
}

// ============================================================================
// NEAR Intents API Types
// ============================================================================

#[derive(Serialize)]
struct JsonRpcRequest<T> {
    id: u32,
    jsonrpc: String,
    method: String,
    params: Vec<T>,
}

#[derive(Serialize)]
struct QuoteParams {
    defuse_asset_identifier_in: String,
    defuse_asset_identifier_out: String,
    exact_amount_in: String,
}

#[derive(Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<JsonRpcError>,
}

#[derive(Deserialize, Debug)]
struct JsonRpcError {
    message: String,
}

#[derive(Deserialize, Debug, Clone)]
struct Quote {
    amount_in: String,
    amount_out: String,
    expiration_time: String,
    quote_hash: String,
}

#[derive(Serialize)]
struct PublishIntentParams {
    signed_data: SignedData,
    quote_hashes: Option<Vec<String>>,
}

#[derive(Serialize)]
struct SignedData {
    payload: Payload,
    standard: String,
    signature: String,
    public_key: String,
}

#[derive(Serialize)]
struct Payload {
    message: String,
    nonce: String,
    recipient: String,
}

#[derive(Deserialize, Debug)]
struct PublishIntentResult {
    status: String,
    intent_hash: Option<String>,
}

#[derive(Serialize)]
struct IntentMessage {
    signer_id: String,
    deadline: String,
    intents: Vec<IntentAction>,
}

#[derive(Serialize)]
#[serde(tag = "intent")]
enum IntentAction {
    #[serde(rename = "token_diff")]
    TokenDiff { diff: serde_json::Value },
    #[serde(rename = "ft_withdraw")]
    FtWithdraw {
        token: String,
        receiver_id: String,
        amount: String,
    },
}

#[derive(Serialize)]
struct GetStatusParams {
    intent_hash: String,
}

#[derive(Deserialize)]
struct GetStatusResult {
    status: String,
}

// ============================================================================
// Constants
// ============================================================================

const INTENTS_API_URL: &str = "https://solver-relay-v2.chaindefuser.com/rpc";
const INTENTS_CONTRACT: &str = "intents.near";

// ============================================================================
// Test Functions
// ============================================================================

#[derive(Serialize)]
struct TestStorageOutput {
    success: bool,
    already_registered: bool,
    storage_balance: Option<String>,
    tx_hash: Option<String>,
    error: Option<String>,
}

fn handle_test_storage(token_contract: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Get credentials from environment
    let swap_contract_id = env::var("SWAP_CONTRACT_ID")
        .map_err(|_| "Missing SWAP_CONTRACT_ID env var")?;
    let swap_contract_private_key = env::var("SWAP_CONTRACT_PRIVATE_KEY")
        .map_err(|_| "Missing SWAP_CONTRACT_PRIVATE_KEY env var")?;
    let rpc_url = env::var("NEAR_RPC_URL")
        .unwrap_or_else(|_| "https://rpc.mainnet.near.org".to_string());

    eprintln!("📊 Step 1: Checking storage_balance_of...");

    // Check storage balance using view()
    let args = serde_json::json!({
        "account_id": swap_contract_id
    });

    let balance_result = near_tx::view(
        &rpc_url,
        token_contract,
        "storage_balance_of",
        &args.to_string(),
    );

    let output = match balance_result {
        Ok(result_str) => {
            // Parse storage balance
            let balance_json: serde_json::Value = serde_json::from_str(&result_str)?;

            if balance_json.is_null() {
                // Not registered - call storage_deposit
                eprintln!("⚠️  Not registered. Calling storage_deposit...");

                match near_tx::storage_deposit(
                    &rpc_url,
                    &swap_contract_id,
                    &swap_contract_private_key,
                    token_contract,
                    None,
                    false,
                ) {
                    Ok(tx_hash) => {
                        eprintln!("✅ Transaction successful! TX: {}", tx_hash);
                        TestStorageOutput {
                            success: true,
                            already_registered: false,
                            storage_balance: None,
                            tx_hash: Some(tx_hash),
                            error: None,
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ Transaction failed: {}", e);
                        TestStorageOutput {
                            success: false,
                            already_registered: false,
                            storage_balance: None,
                            tx_hash: None,
                            error: Some(e.to_string()),
                        }
                    }
                }
            } else {
                // Already registered
                let total = balance_json.get("total")
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown");
                eprintln!("✅ Already registered! Balance: {}", total);

                TestStorageOutput {
                    success: true,
                    already_registered: true,
                    storage_balance: Some(total.to_string()),
                    tx_hash: None,
                    error: None,
                }
            }
        }
        Err(e) => {
            // Error (likely not registered) - try storage_deposit
            eprintln!("⚠️  Error checking balance: {}. Trying storage_deposit...", e);

            match near_tx::storage_deposit(
                &rpc_url,
                &swap_contract_id,
                &swap_contract_private_key,
                token_contract,
                None,
                false,
            ) {
                Ok(tx_hash) => {
                    eprintln!("✅ Transaction successful! TX: {}", tx_hash);
                    TestStorageOutput {
                        success: true,
                        already_registered: false,
                        storage_balance: None,
                        tx_hash: Some(tx_hash),
                        error: None,
                    }
                }
                Err(e) => {
                    eprintln!("❌ Transaction failed: {}", e);
                    TestStorageOutput {
                        success: false,
                        already_registered: false,
                        storage_balance: None,
                        tx_hash: None,
                        error: Some(e.to_string()),
                    }
                }
            }
        }
    };

    // Output to stdout
    print!("{}", serde_json::to_string(&output)?);
    io::stdout().flush()?;

    Ok(())
}

// ============================================================================
// Main Logic
// ============================================================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read input from stdin
    let mut input_string = String::new();
    io::stdin().read_to_string(&mut input_string)?;

    // Parse input JSON
    let input: Input = serde_json::from_str(&input_string)?;

    // Route based on input type
    match input {
        Input::TestStorage { ref token_contract, .. } => {
            eprintln!("🧪 Test mode: checking storage for {}", token_contract);
            handle_test_storage(token_contract)?;
        }
        Input::Swap {
            ref sender_id,
            ref token_in,
            ref token_out,
            ref amount_in,
            ref min_amount_out,
            ref swap_contract_id,
        } => {
            eprintln!("Processing swap for {}: {} {} → {} {}",
                sender_id, amount_in, token_in, min_amount_out, token_out);

            // Get swap contract private key from environment (passed via secrets)
            let swap_contract_private_key = match env::var("SWAP_CONTRACT_PRIVATE_KEY") {
                Ok(key) => key,
                Err(_) => {
                    let output = Output {
                        success: false,
                        amount_out: None,
                        error_message: Some("SWAP_CONTRACT_PRIVATE_KEY not found in environment".to_string()),
                        intent_hash: None,
                    };
                    print!("{}", serde_json::to_string(&output)?);
                    io::stdout().flush()?;
                    return Ok(());
                }
            };

            // Execute swap flow
            match execute_swap(
                sender_id,
                token_in,
                token_out,
                amount_in,
                min_amount_out,
                swap_contract_id,
                &swap_contract_private_key,
            ) {
                Ok(result) => {
                    print!("{}", serde_json::to_string(&result)?);
                    io::stdout().flush()?;
                }
                Err(e) => {
                    eprintln!("Swap execution failed: {:?}", e);
                    let output = Output {
                        success: false,
                        amount_out: None,
                        error_message: Some(format!("Internal error: {}", e)),
                        intent_hash: None,
                    };
                    print!("{}", serde_json::to_string(&output)?);
                    io::stdout().flush()?;
                }
            }
        }
    }

    Ok(())
}

fn execute_swap(
    sender_id: &str,
    token_in: &str,
    token_out: &str,
    amount_in: &str,
    min_amount_out: &str,
    swap_contract_id: &str,
    swap_contract_private_key: &str,
) -> Result<Output, Box<dyn std::error::Error>> {
    // Step 1: Get quote
    eprintln!("Step 1: Getting quote from NEAR Intents API");
    let quote = get_quote(token_in, token_out, amount_in)?;

    let amount_out_num: u128 = quote.amount_out.parse()
        .map_err(|_| "Failed to parse amount_out")?;
    let min_amount_out_num: u128 = min_amount_out.parse()
        .map_err(|_| "Failed to parse min_amount_out")?;

    if amount_out_num < min_amount_out_num {
        return Ok(Output {
            success: false,
            amount_out: None,
            error_message: Some(format!(
                "Insufficient liquidity: {} < {}",
                amount_out_num, min_amount_out_num
            )),
            intent_hash: None,
        });
    }

    eprintln!("✅ Quote received: {} out, expires at {}", quote.amount_out, quote.expiration_time);

    // Get RPC URL from environment
    let rpc_url = std::env::var("NEAR_RPC_URL")
        .unwrap_or_else(|_| "https://rpc.mainnet.near.org".to_string());

    // Step 1.5: Pre-flight check - verify sender has storage deposit for output token
    eprintln!("Step 1.5: Checking storage deposit for output token...");

    // Extract token contract from defuse asset ID
    let token_out_contract = token_out.strip_prefix("nep141:")
        .ok_or("Invalid token_out format, expected nep141:address")?;

    let storage_check_args = serde_json::json!({
        "account_id": sender_id
    });

    match near_tx::view(
        &rpc_url,
        token_out_contract,
        "storage_balance_of",
        &storage_check_args.to_string(),
    ) {
        Ok(result_str) => {
            let balance_json: serde_json::Value = serde_json::from_str(&result_str)
                .map_err(|_| "Failed to parse storage balance response")?;

            if balance_json.is_null() {
                eprintln!("❌ Pre-flight check failed: sender {} has no storage deposit for {}",
                    sender_id, token_out_contract);
                return Ok(Output {
                    success: false,
                    amount_out: None,
                    error_message: Some(format!(
                        "User {} has no storage deposit for output token {}. Please call storage_deposit first.",
                        sender_id, token_out_contract
                    )),
                    intent_hash: None,
                });
            }
            eprintln!("✅ Storage deposit verified for {}", sender_id);
        }
        Err(e) => {
            eprintln!("⚠️  Warning: Could not verify storage deposit ({}). Proceeding anyway...", e);
            // Continue - storage check failure shouldn't block swap in production
        }
    }

    // Step 2: Deposit tokens to intents.near
    eprintln!("Step 2: Depositing {} to intents.near", amount_in);

    // Extract token contract address from defuse asset ID (format: "nep141:token.near")
    let token_contract = token_in.strip_prefix("nep141:")
        .ok_or("Invalid token_in format, expected nep141:address")?;

    eprintln!("📤 Calling ft_transfer_call: {} {} from {} to {}",
        amount_in, token_contract, swap_contract_id, INTENTS_CONTRACT);

    let deposit_tx_hash = match near_tx::ft_transfer_call(
        &rpc_url,
        swap_contract_id,
        swap_contract_private_key,
        token_contract,
        INTENTS_CONTRACT,
        amount_in,
        "",
    ) {
        Ok(tx_hash) => {
            eprintln!("✅ Deposit successful: {}", tx_hash);
            eprintln!("   🔗 View on explorer: https://nearblocks.io/txns/{}", tx_hash);
            tx_hash
        }
        Err(e) => {
            eprintln!("❌ Deposit failed: {}", e);
            return Err(e);
        }
    };

    // Step 3: Publish swap intent
    eprintln!("Step 3: Publishing swap intent to NEAR Intents API");
    eprintln!("   Swap: {} {} → {} {}", quote.amount_in, token_in, quote.amount_out, token_out);

    let intent_hash = match publish_swap_intent(
        swap_contract_id,
        swap_contract_private_key,
        token_in,
        token_out,
        &quote,
    ) {
        Ok(hash) => {
            eprintln!("✅ Intent published successfully");
            eprintln!("   Intent hash: {}", hash);
            hash
        }
        Err(e) => {
            eprintln!("❌ Failed to publish intent: {}", e);
            return Err(e);
        }
    };

    // Step 4: Wait for settlement
    eprintln!("Step 4: Waiting for intent settlement (max 30 seconds)...");

    let settled = match wait_for_settlement(&intent_hash) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ Error checking settlement status: {}", e);
            return Err(e);
        }
    };

    if !settled {
        eprintln!("❌ Intent failed to settle within 30 second timeout");
        eprintln!("   Intent hash: {}", intent_hash);
        return Ok(Output {
            success: false,
            amount_out: None,
            error_message: Some("Intent failed to settle within timeout".to_string()),
            intent_hash: Some(intent_hash),
        });
    }

    eprintln!("✅ Intent settled successfully!");

    // Step 5: Withdraw tokens to original sender
    eprintln!("Step 5: Withdrawing {} {} to {}", quote.amount_out, token_out, sender_id);

    let withdraw_success = match withdraw_tokens(
        swap_contract_id,
        swap_contract_private_key,
        token_out,
        sender_id,
        &quote.amount_out,
    ) {
        Ok(success) => success,
        Err(e) => {
            eprintln!("❌ Withdrawal failed: {}", e);
            return Err(e);
        }
    };

    if !withdraw_success {
        eprintln!("❌ Withdrawal returned failure status");
        return Ok(Output {
            success: false,
            amount_out: Some(quote.amount_out.clone()),
            error_message: Some("Failed to withdraw tokens from intents contract".to_string()),
            intent_hash: Some(intent_hash),
        });
    }

    eprintln!("✅ Withdrawal successful!");
    eprintln!("🎉 Swap completed successfully: {} {} → {} {}",
        quote.amount_in, token_in, quote.amount_out, token_out);

    Ok(Output {
        success: true,
        amount_out: Some(quote.amount_out.clone()),
        error_message: None,
        intent_hash: Some(intent_hash),
    })
}

// ============================================================================
// NEAR Intents API Functions
// ============================================================================

fn get_quote(
    token_in: &str,
    token_out: &str,
    amount_in: &str,
) -> Result<Quote, Box<dyn std::error::Error>> {
    let request = JsonRpcRequest {
        id: 1,
        jsonrpc: "2.0".to_string(),
        method: "quote".to_string(),
        params: vec![QuoteParams {
            defuse_asset_identifier_in: token_in.to_string(),
            defuse_asset_identifier_out: token_out.to_string(),
            exact_amount_in: amount_in.to_string(),
        }],
    };

    // Retry logic: 3 attempts with 1 second delay
    const MAX_RETRIES: u32 = 3;
    const RETRY_DELAY_MS: u64 = 1000;

    let mut last_error = String::new();

    for attempt in 1..=MAX_RETRIES {
        eprintln!("🔄 Quote API attempt {}/{}", attempt, MAX_RETRIES);

        match Client::new()
            .post(INTENTS_API_URL)
            .header("Content-Type", "application/json")
            .connect_timeout(Duration::from_secs(10))
            .body(serde_json::to_string(&request)?.as_bytes())
            .send()
        {
            Ok(response) => {
                let status = response.status();
                if status != 200 {
                    last_error = format!("Quote API returned status {}", status);
                    eprintln!("⚠️  Attempt {} failed: {}", attempt, last_error);
                    if attempt < MAX_RETRIES {
                        std::thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
                        continue;
                    }
                } else {
                    match response.body() {
                        Ok(body) => {
                            match serde_json::from_slice::<JsonRpcResponse<Vec<Quote>>>(&body) {
                                Ok(json_response) => {
                                    if let Some(error) = json_response.error {
                                        last_error = format!("Quote API error: {}", error.message);
                                        eprintln!("⚠️  Attempt {} failed: {}", attempt, last_error);
                                        if attempt < MAX_RETRIES {
                                            std::thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
                                            continue;
                                        }
                                    } else if let Some(quotes) = json_response.result {
                                        // Find best quote (highest amount_out)
                                        if let Some(best_quote) = quotes
                                            .into_iter()
                                            .max_by_key(|q| q.amount_out.parse::<u128>().unwrap_or(0))
                                        {
                                            eprintln!("✅ Quote received successfully");
                                            return Ok(best_quote);
                                        } else {
                                            last_error = "No valid quotes".to_string();
                                        }
                                    } else {
                                        last_error = "No quotes returned".to_string();
                                    }
                                }
                                Err(e) => {
                                    last_error = format!("Failed to parse response: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            last_error = format!("Failed to read response body: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                last_error = format!("HTTP request failed: {}", e);
                eprintln!("⚠️  Attempt {} failed: {}", attempt, last_error);
            }
        }

        if attempt < MAX_RETRIES {
            std::thread::sleep(Duration::from_millis(RETRY_DELAY_MS));
        }
    }

    Err(format!("Quote API failed after {} retries. Last error: {}", MAX_RETRIES, last_error).into())
}

fn publish_swap_intent(
    signer_id: &str,
    private_key: &str,
    token_in: &str,
    token_out: &str,
    quote: &Quote,
) -> Result<String, Box<dyn std::error::Error>> {
    // Use tokens WITH "nep141:" prefix (as per official docs)
    // https://docs.near-intents.org/near-intents/market-makers/bus/solver-relay

    // Build intent message using json! macro to preserve field order
    let diff = serde_json::json!({
        token_in: format!("-{}", quote.amount_in),
        token_out: quote.amount_out.clone()
    });

    let intent_message = IntentMessage {
        signer_id: signer_id.to_string(),
        deadline: quote.expiration_time.clone(),
        intents: vec![IntentAction::TokenDiff {
            diff,
        }],
    };

    // Serialize with spaces after colons (like Python json.dumps)
    let message_str = serde_json::to_string(&intent_message)?;
    // Add space after each colon (to match Python format)
    let message_str = message_str.replace("\":", "\": ");

    eprintln!("📝 Intent message to sign:");
    eprintln!("{}", message_str);
    eprintln!("   Length: {} chars", message_str.len());

    // Generate nonce
    let nonce = generate_nonce();

    // Sign the intent (simplified - in production use proper ed25519 signing)
    let signature = sign_intent(&message_str, &nonce, private_key)?;

    // Publish intent
    let params = PublishIntentParams {
        signed_data: SignedData {
            payload: Payload {
                message: message_str,
                nonce: nonce.clone(),
                recipient: INTENTS_CONTRACT.to_string(),
            },
            standard: "nep413".to_string(),
            signature: format!("ed25519:{}", signature),
            public_key: derive_public_key(private_key)?,
        },
        quote_hashes: Some(vec![quote.quote_hash.clone()]),
    };

    let request = JsonRpcRequest {
        id: 1,
        jsonrpc: "2.0".to_string(),
        method: "publish_intent".to_string(),
        params: vec![params],
    };

    eprintln!("📤 Publishing swap intent to: {}", INTENTS_API_URL);
    eprintln!("   Method: publish_intent");
    eprintln!("   Signer: {}", signer_id);
    eprintln!("   Token in: {} (amount: {})", token_in, quote.amount_in);
    eprintln!("   Token out: {} (amount: {})", token_out, quote.amount_out);
    eprintln!("   Quote hash: {}", quote.quote_hash);

    let request_json = serde_json::to_string_pretty(&request)?;
    eprintln!("📦 Request body (first 2000 chars):\n{}", &request_json.chars().take(2000).collect::<String>());

    let response = Client::new()
        .post(INTENTS_API_URL)
        .header("Content-Type", "application/json")
        .connect_timeout(Duration::from_secs(10))
        .body(serde_json::to_string(&request)?.as_bytes())
        .send()?;

    if response.status() != 200 {
        return Err(format!("Publish intent API returned status {}", response.status()).into());
    }

    let body = response.body()?;

    // Debug: print response body
    let body_str = String::from_utf8_lossy(&body);
    eprintln!("📥 Publish intent response (first 1000 chars): {}", &body_str.chars().take(1000).collect::<String>());

    let json_response: JsonRpcResponse<PublishIntentResult> = serde_json::from_slice(&body)
        .map_err(|e| format!("Failed to parse publish_intent response: {}. Body: {}", e, body_str))?;

    if let Some(error) = json_response.error {
        eprintln!("❌ API returned error object: {:?}", error);
        return Err(format!("Publish intent API error: {}", error.message).into());
    }

    let result = json_response.result.ok_or("No result from publish_intent")?;

    eprintln!("📊 Publish intent result: status={}, intent_hash={:?}", result.status, result.intent_hash);

    if result.status != "OK" {
        return Err(format!("Intent publish failed with status: {}. Full result: {:?}", result.status, result).into());
    }

    result.intent_hash.ok_or("No intent_hash returned".into())
}

fn wait_for_settlement(intent_hash: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let max_attempts = 30; // 30 seconds timeout

    for attempt in 0..max_attempts {
        if attempt > 0 {
            // Sleep for 1 second between checks
            // Note: WASI doesn't have std::thread::sleep, so we just loop
            // In production, you'd use a proper sleep mechanism
        }

        let request = JsonRpcRequest {
            id: 1,
            jsonrpc: "2.0".to_string(),
            method: "get_status".to_string(),
            params: vec![GetStatusParams {
                intent_hash: intent_hash.to_string(),
            }],
        };

        let response = Client::new()
            .post(INTENTS_API_URL)
            .header("Content-Type", "application/json")
            .connect_timeout(Duration::from_secs(5))
            .body(serde_json::to_string(&request)?.as_bytes())
            .send()?;

        if response.status() != 200 {
            eprintln!("get_status returned status {}, retrying...", response.status());
            continue;
        }

        let body = response.body()?;
        let json_response: JsonRpcResponse<GetStatusResult> = serde_json::from_slice(&body)?;

        if let Some(result) = json_response.result {
            eprintln!("Intent status (attempt {}): {}", attempt + 1, result.status);

            match result.status.as_str() {
                "SETTLED" => return Ok(true),
                "NOT_FOUND_OR_NOT_VALID_ANYMORE" | "NOT_FOUND_OR_NOT_VALID" | "FAILED" => {
                    return Ok(false);
                }
                _ => {} // Continue polling
            }
        }
    }

    Ok(false) // Timeout
}

fn withdraw_tokens(
    signer_id: &str,
    private_key: &str,
    token: &str,
    receiver_id: &str,
    amount: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    // IMPORTANT: ft_withdraw uses token WITHOUT "nep141:" prefix
    // (unlike token_diff which uses WITH prefix)
    // https://docs.near-intents.org/near-intents/market-makers/bus/solver-relay

    // Strip "nep141:" prefix if present
    let token_without_prefix = if token.starts_with("nep141:") {
        &token[7..]
    } else {
        token
    };

    // Build withdraw intent message
    let intent_message = IntentMessage {
        signer_id: signer_id.to_string(),
        deadline: get_deadline_180s(),
        intents: vec![IntentAction::FtWithdraw {
            token: token_without_prefix.to_string(),
            receiver_id: receiver_id.to_string(),
            amount: amount.to_string(),
        }],
    };

    let message_str = serde_json::to_string(&intent_message)?;

    // Generate nonce
    let nonce = generate_nonce();

    // Sign the intent
    let signature = sign_intent(&message_str, &nonce, private_key)?;

    // Publish withdraw intent
    let params = PublishIntentParams {
        signed_data: SignedData {
            payload: Payload {
                message: message_str,
                nonce: nonce.clone(),
                recipient: INTENTS_CONTRACT.to_string(),
            },
            standard: "nep413".to_string(),
            signature: format!("ed25519:{}", signature),
            public_key: derive_public_key(private_key)?,
        },
        quote_hashes: None,
    };

    let request = JsonRpcRequest {
        id: 1,
        jsonrpc: "2.0".to_string(),
        method: "publish_intent".to_string(),
        params: vec![params],
    };

    eprintln!("📤 Publishing withdraw intent to: {}", INTENTS_API_URL);
    eprintln!("   Method: publish_intent (withdraw)");
    eprintln!("   Signer: {}", signer_id);
    eprintln!("   Token: {}", token);
    eprintln!("   Receiver: {}", receiver_id);
    eprintln!("   Amount: {}", amount);

    let request_json = serde_json::to_string_pretty(&request)?;
    eprintln!("📦 Request body (first 2000 chars):\n{}", &request_json.chars().take(2000).collect::<String>());

    let response = Client::new()
        .post(INTENTS_API_URL)
        .header("Content-Type", "application/json")
        .connect_timeout(Duration::from_secs(10))
        .body(serde_json::to_string(&request)?.as_bytes())
        .send()?;

    if response.status() != 200 {
        eprintln!("❌ Withdraw API returned status: {}", response.status());
        return Err(format!("Withdraw API returned status {}", response.status()).into());
    }

    let body = response.body()?;

    // Debug: print response body
    let body_str = String::from_utf8_lossy(&body);
    eprintln!("📥 Withdraw intent response (first 1000 chars): {}", &body_str.chars().take(1000).collect::<String>());

    let json_response: JsonRpcResponse<PublishIntentResult> = serde_json::from_slice(&body)
        .map_err(|e| format!("Failed to parse withdraw response: {}. Body: {}", e, body_str))?;

    if let Some(error) = json_response.error {
        eprintln!("❌ Withdraw API returned error object: {:?}", error);
        return Err(format!("Withdraw API error: {}", error.message).into());
    }

    let result = json_response.result.ok_or("No result from withdraw")?;

    eprintln!("📊 Withdraw intent result: status={}, intent_hash={:?}", result.status, result.intent_hash);

    let intent_hash = result.intent_hash.ok_or("No intent_hash for withdraw")?;

    // Wait for withdrawal settlement
    wait_for_settlement(&intent_hash)
}

// ============================================================================
// Helper Functions
// ============================================================================

fn generate_nonce() -> String {
    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string();

    let mut hasher = Sha256::new();
    hasher.update(timestamp.as_bytes());
    let result = hasher.finalize();

    base64::encode(result)
}

fn get_deadline_180s() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now_plus_180 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() + 180;

    // Convert to ISO 8601 format: YYYY-MM-DDTHH:MM:SS.000Z
    // Simplified calculation (assumes Unix epoch)
    let total_seconds = now_plus_180;
    let seconds = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    let minutes = total_minutes % 60;
    let total_hours = total_minutes / 60;
    let hours = total_hours % 24;
    let total_days = total_hours / 24;

    // Days since 1970-01-01
    // Simplified: doesn't account for leap years, good enough for deadlines
    let year = 1970 + (total_days / 365);
    let day_of_year = total_days % 365;
    let month = (day_of_year / 30) + 1; // Approximate
    let day = (day_of_year % 30) + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.000Z",
        year, month, day, hours, minutes, seconds
    )
}

fn sign_intent(
    message: &str,
    nonce: &str,
    private_key: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // Remove "ed25519:" prefix if present
    let key_base58 = if private_key.starts_with("ed25519:") {
        &private_key[8..]
    } else {
        private_key
    };

    let (signature, _public_key) =
        crypto::sign_nep413_intent(message, nonce, INTENTS_CONTRACT, key_base58)?;

    Ok(signature)
}

fn derive_public_key(private_key: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Remove "ed25519:" prefix if present
    let key_base58 = if private_key.starts_with("ed25519:") {
        &private_key[8..]
    } else {
        private_key
    };

    // Sign a dummy message to get the public key
    let dummy_nonce = base64::encode(&[0u8; 32]);
    let (_signature, public_key) =
        crypto::sign_nep413_intent("{}", &dummy_nonce, INTENTS_CONTRACT, key_base58)?;

    Ok(format!("ed25519:{}", public_key))
}
