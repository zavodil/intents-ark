#[allow(dead_code, deprecated)]
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
        #[allow(dead_code)]
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
        /// Slippage tolerance in basis points (100 = 1%), chosen by the caller and forwarded
        /// to the 1Click quote. Absent for callers that predate the field, in which case
        /// `DEFAULT_SLIPPAGE_TOLERANCE_BPS` reproduces the previously hardcoded behaviour.
        #[serde(default)]
        slippage_tolerance: Option<u32>,
    },
}

#[derive(Serialize, Debug)]
struct Output {
    success: bool,
    amount_out: Option<String>,
    error_message: Option<String>,
    /// deposit_address from 1Click (for tracking), or intent_hash from swap_details
    intent_hash: Option<String>,
    /// True once token_in has left the swap contract's NEP-141 balance into intents.near
    /// (i.e. step 3 succeeded). On failure this tells the contract NOT to pay a refund out
    /// of its NEP-141 balance (the funds now live in the contract's intents balance and must
    /// be recovered by the operator), avoiding a pool-funded double refund.
    #[serde(default)]
    funds_deposited: bool,
}

// ============================================================================
// 1Click API Types (matching coordinator's backend/mod.rs)
// ============================================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OneClickQuoteRequest {
    dry: bool,
    swap_type: String,
    slippage_tolerance: u32,
    origin_asset: String,
    deposit_type: String,
    destination_asset: String,
    amount: String,
    refund_to: String,
    refund_type: String,
    recipient: String,
    recipient_type: String,
    deadline: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OneClickQuoteResponse {
    #[allow(dead_code)]
    correlation_id: String,
    quote: OneClickQuote,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct OneClickQuote {
    deposit_address: String,
    #[allow(dead_code)]
    amount_in: String,
    amount_out: String,
    min_amount_out: String,
    #[allow(dead_code)]
    deadline: String,
    #[serde(default)]
    #[allow(dead_code)]
    time_estimate: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OneClickSubmitDeposit {
    tx_hash: String,
    deposit_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    near_sender_account: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OneClickStatusResponse {
    status: String,
    #[serde(default)]
    swap_details: Option<OneClickSwapDetails>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OneClickSwapDetails {
    #[serde(default)]
    amount_out: Option<String>,
    #[serde(default)]
    intent_hashes: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    near_tx_hashes: Vec<String>,
}

// ============================================================================
// Constants
// ============================================================================

const ONECLICK_BASE_URL: &str = "https://1click.chaindefuser.com";
const INTENTS_CONTRACT: &str = "intents.near";

/// Only this contract is allowed to invoke swaps. Checked against NEAR_PREDECESSOR_ID,
/// which the worker injects from the on-chain receipt (receiver == OutLayer contract) and
/// therefore cannot be forged via input. Set this to your deployed swap contract account.
const AUTHORIZED_CALLER: &str = "v1.publishintent.near";

/// Slippage tolerance (basis points, 100 = 1%) used when the input omits one. Matches the
/// value that was hardcoded before it became caller-supplied, so older inputs behave the same.
const DEFAULT_SLIPPAGE_TOLERANCE_BPS: u32 = 100;

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

/// Retained for reference only — the `TestStorage` input path that called this is disabled
/// (it signed `storage_deposit` with the swap contract's key against an input-supplied
/// contract, with no caller check).
#[allow(dead_code)]
fn handle_test_storage(token_contract: &str) -> Result<(), Box<dyn std::error::Error>> {
    let swap_contract_id = env::var("SWAP_CONTRACT_ID")
        .map_err(|_| "Missing SWAP_CONTRACT_ID env var")?;
    let swap_contract_private_key = env::var("SWAP_CONTRACT_PRIVATE_KEY")
        .map_err(|_| "Missing SWAP_CONTRACT_PRIVATE_KEY env var")?;
    let rpc_url = env::var("NEAR_RPC_URL")
        .unwrap_or_else(|_| "https://rpc.mainnet.fastnear.com".to_string());

    eprintln!("Step 1: Checking storage_balance_of...");

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
            let balance_json: serde_json::Value = serde_json::from_str(&result_str)?;

            if balance_json.is_null() {
                eprintln!("Not registered. Calling storage_deposit...");

                match near_tx::storage_deposit(
                    &rpc_url,
                    &swap_contract_id,
                    &swap_contract_private_key,
                    token_contract,
                    None,
                    false,
                ) {
                    Ok(tx_hash) => {
                        eprintln!("Transaction successful! TX: {}", tx_hash);
                        TestStorageOutput {
                            success: true,
                            already_registered: false,
                            storage_balance: None,
                            tx_hash: Some(tx_hash),
                            error: None,
                        }
                    }
                    Err(e) => {
                        eprintln!("Transaction failed: {}", e);
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
                let total = balance_json.get("total")
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown");
                eprintln!("Already registered! Balance: {}", total);

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
            eprintln!("Error checking balance: {}. Trying storage_deposit...", e);

            match near_tx::storage_deposit(
                &rpc_url,
                &swap_contract_id,
                &swap_contract_private_key,
                token_contract,
                None,
                false,
            ) {
                Ok(tx_hash) => {
                    eprintln!("Transaction successful! TX: {}", tx_hash);
                    TestStorageOutput {
                        success: true,
                        already_registered: false,
                        storage_balance: None,
                        tx_hash: Some(tx_hash),
                        error: None,
                    }
                }
                Err(e) => {
                    eprintln!("Transaction failed: {}", e);
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

    print!("{}", serde_json::to_string(&output)?);
    io::stdout().flush()?;

    Ok(())
}

// ============================================================================
// Main Logic
// ============================================================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input_string = String::new();
    io::stdin().read_to_string(&mut input_string)?;

    let input: Input = serde_json::from_str(&input_string)?;

    match input {
        Input::TestStorage { ref token_contract, .. } => {
            // DISABLED in prod
            eprintln!(
                "Rejected test_storage: path is disabled (token_contract='{}')",
                token_contract
            );
            let output = Output {
                success: false,
                amount_out: None,
                error_message: Some("test_storage is disabled".to_string()),
                intent_hash: None,
                funds_deposited: false,
            };
            print!("{}", serde_json::to_string(&output)?);
            io::stdout().flush()?;
            return Ok(());
        }
        Input::Swap {
            ref sender_id,
            ref token_in,
            ref token_out,
            ref amount_in,
            ref min_amount_out,
            ref swap_contract_id,
            slippage_tolerance,
        } => {
            eprintln!("Processing swap for {}: {} {} -> {} {}",
                sender_id, amount_in, token_in, min_amount_out, token_out);

            // SECURITY: only AUTHORIZED_CALLER may trigger a swap. NEAR_PREDECESSOR_ID is
            // injected by the worker from the on-chain receipt whose receiver is the OutLayer
            // contract, so it reflects the REAL caller of request_execution and cannot be
            // forged via input. A direct call to OutLayer by anyone else carries their own
            // predecessor and is rejected here — this stops an attacker from invoking this
            // exact (code-pinned) WASM with crafted input to move the contract's funds while
            // bypassing the contract's on-chain checks (token whitelist, deposit backing,
            // slippage). Compared against a trusted constant, never against input.
            let predecessor = env::var("NEAR_PREDECESSOR_ID").unwrap_or_default();
            if predecessor != AUTHORIZED_CALLER {
                eprintln!(
                    "Rejected unauthorized swap: predecessor='{}', expected '{}'",
                    predecessor, AUTHORIZED_CALLER
                );
                let output = Output {
                    success: false,
                    amount_out: None,
                    error_message: Some(format!(
                        "Unauthorized caller: this swap may only be invoked by {} (predecessor was '{}')",
                        AUTHORIZED_CALLER, predecessor
                    )),
                    intent_hash: None,
                    funds_deposited: false,
                };
                print!("{}", serde_json::to_string(&output)?);
                io::stdout().flush()?;
                return Ok(());
            }

            // SECURITY (defense-in-depth): swap_contract_id comes from input and is used as
            // the signing account (SWAP_CONTRACT_PRIVATE_KEY) and as refund_to/recipient in
            // 1Click. The private key belongs to AUTHORIZED_CALLER, so signing on behalf of
            // any other account is nonsensical and must never happen even if the predecessor
            // check is ever bypassed. Pin it to the same trusted constant.
            if swap_contract_id != AUTHORIZED_CALLER {
                eprintln!(
                    "Rejected swap: swap_contract_id='{}', expected '{}'",
                    swap_contract_id, AUTHORIZED_CALLER
                );
                let output = Output {
                    success: false,
                    amount_out: None,
                    error_message: Some(format!(
                        "Invalid swap_contract_id: expected {}, got '{}'",
                        AUTHORIZED_CALLER, swap_contract_id
                    )),
                    intent_hash: None,
                    funds_deposited: false,
                };
                print!("{}", serde_json::to_string(&output)?);
                io::stdout().flush()?;
                return Ok(());
            }

            let swap_contract_private_key = match env::var("SWAP_CONTRACT_PRIVATE_KEY") {
                Ok(key) => key,
                Err(_) => {
                    let output = Output {
                        success: false,
                        amount_out: None,
                        error_message: Some("SWAP_CONTRACT_PRIVATE_KEY not found in environment".to_string()),
                        intent_hash: None,
                        funds_deposited: false,
                    };
                    print!("{}", serde_json::to_string(&output)?);
                    io::stdout().flush()?;
                    return Ok(());
                }
            };

            match execute_swap(
                sender_id,
                token_in,
                token_out,
                amount_in,
                min_amount_out,
                swap_contract_id,
                &swap_contract_private_key,
                slippage_tolerance.unwrap_or(DEFAULT_SLIPPAGE_TOLERANCE_BPS),
            ) {
                Ok(result) => {
                    print!("{}", serde_json::to_string(&result)?);
                    io::stdout().flush()?;
                }
                Err(e) => {
                    eprintln!("Swap execution failed: {:?}", e);
                    // execute_swap only returns Err on PRE-deposit failures (see its body: every
                    // path after the intents deposit returns Ok(Output{funds_deposited:true})).
                    // So an Err here always means funds never left the contract → safe to refund.
                    let output = Output {
                        success: false,
                        amount_out: None,
                        error_message: Some(format!("Internal error: {}", e)),
                        intent_hash: None,
                        funds_deposited: false,
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
    slippage_tolerance: u32,
) -> Result<Output, Box<dyn std::error::Error>> {
    let oneclick_jwt = env::var("ONECLICK_JWT").ok().filter(|s| !s.is_empty());

    let rpc_url = env::var("NEAR_RPC_URL")
        .unwrap_or_else(|_| "https://rpc.mainnet.fastnear.com".to_string());

    // Step 1: Get 1Click quote
    eprintln!("Step 1: Getting 1Click quote {} -> {}", token_in, token_out);
    let quote_resp = get_oneclick_quote(
        oneclick_jwt.as_deref(),
        token_in,
        token_out,
        amount_in,
        swap_contract_id,
        slippage_tolerance,
    )?;

    let deposit_address = &quote_resp.quote.deposit_address;
    let amount_out = &quote_resp.quote.amount_out;
    let quote_min_amount_out = &quote_resp.quote.min_amount_out;

    eprintln!("1Click quote: deposit_address={}, amount_out={}, min_amount_out={}",
        deposit_address, amount_out, quote_min_amount_out);

    // Step 2: Validate min_amount_out against the quote's worst-case delivery
    // (quote.min_amount_out = amount_out minus 1Click's slippage tolerance),
    // so the delivered amount can never end up below the user's minimum.
    let quote_min_amount_out_num: u128 = quote_min_amount_out.parse()
        .map_err(|_| "Failed to parse quote min_amount_out")?;
    let min_amount_out_num: u128 = min_amount_out.parse()
        .map_err(|_| "Failed to parse min_amount_out")?;

    if quote_min_amount_out_num < min_amount_out_num {
        return Ok(Output {
            success: false,
            amount_out: None,
            error_message: Some(format!(
                "Quote worst-case output ({}) is less than min_amount_out ({}); expected output was {}",
                quote_min_amount_out, min_amount_out, amount_out
            )),
            intent_hash: None,
            funds_deposited: false,
        });
    }

    // Step 2.5: Pre-flight check - verify sender has storage deposit for output token
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
                eprintln!("Pre-flight check failed: sender {} has no storage deposit for {}",
                    sender_id, token_out_contract);
                return Ok(Output {
                    success: false,
                    amount_out: None,
                    error_message: Some(format!(
                        "User {} has no storage deposit for output token {}. Please call storage_deposit first.",
                        sender_id, token_out_contract
                    )),
                    intent_hash: None,
                    funds_deposited: false,
                });
            }
            eprintln!("Storage deposit verified for {}", sender_id);
        }
        Err(e) => {
            eprintln!("Warning: Could not verify storage deposit ({}). Proceeding anyway...", e);
        }
    }

    // Step 3: Deposit tokens to intents.near via ft_transfer_call
    let token_in_contract = token_in.strip_prefix("nep141:")
        .ok_or("Invalid token_in format, expected nep141:address")?;

    eprintln!("Step 3: Depositing {} {} into intents.near", amount_in, token_in_contract);

    let deposit_tx = near_tx::ft_transfer_call(
        &rpc_url,
        swap_contract_id,
        swap_contract_private_key,
        token_in_contract,
        INTENTS_CONTRACT,
        amount_in,
        "",
    )?;

    eprintln!("Deposit tx: {}", deposit_tx);

    // From this point on, token_in has LEFT the swap contract's NEP-141 balance and lives in
    // the contract's intents.near balance. Every failure path below must therefore return
    // Ok(Output{funds_deposited:true}) rather than Err — the contract keys its refund decision
    // off this flag and must NOT pay a pool-funded NEP-141 refund for these funds.
    let funds_deposited = true;

    // Step 4: mt_transfer on intents.near — move tokens to 1Click deposit address
    eprintln!("Step 4: mt_transfer {} {} to deposit address {}", amount_in, token_in, deposit_address);

    let mt_args = serde_json::json!({
        "receiver_id": deposit_address,
        "token_id": token_in,
        "amount": amount_in,
    });

    let mt_tx = match near_tx::call(
        &rpc_url,
        swap_contract_id,
        swap_contract_private_key,
        INTENTS_CONTRACT,
        "mt_transfer",
        &mt_args.to_string(),
        100_000_000_000_000, // 100 TGas
        1,                   // 1 yoctoNEAR
    ) {
        Ok(tx) => tx,
        Err(e) => {
            // Deposit succeeded but the move to the 1Click deposit address failed: token_in is
            // stranded in the contract's intents balance. Signal funds_deposited so the contract
            // defers to operator recovery instead of refunding from the pool.
            eprintln!("mt_transfer failed after deposit: {}", e);
            return Ok(Output {
                success: false,
                amount_out: None,
                error_message: Some(format!(
                    "Deposit succeeded but mt_transfer to 1Click failed: {}. Funds are in the contract's intents balance and require operator recovery.",
                    e
                )),
                intent_hash: None,
                funds_deposited,
            });
        }
    };

    eprintln!("mt_transfer tx: {}", mt_tx);

    // Step 5: Notify 1Click about the deposit (best-effort, non-fatal)
    eprintln!("Step 5: Submitting deposit notification to 1Click");
    if let Err(e) = submit_oneclick_deposit(
        oneclick_jwt.as_deref(),
        &mt_tx,
        deposit_address,
        Some(swap_contract_id),
    ) {
        eprintln!("Warning: Failed to submit deposit to 1Click (non-fatal): {}", e);
    }

    // Step 6: Poll 1Click status until terminal state
    eprintln!("Step 6: Polling 1Click status for deposit_address={}", deposit_address);
    let status_resp = match poll_oneclick_status(oneclick_jwt.as_deref(), deposit_address) {
        Ok(resp) => resp,
        Err(e) => {
            // Post-deposit: don't let a polling error become an Err (which the contract would
            // treat as a safe-to-refund pre-deposit failure). Funds are already in flight.
            eprintln!("Status polling failed after deposit: {}", e);
            return Ok(Output {
                success: false,
                amount_out: None,
                error_message: Some(format!(
                    "Deposit succeeded but status polling failed: {}. Funds may be in flight or in the contract's intents balance; operator recovery required.",
                    e
                )),
                intent_hash: Some(deposit_address.clone()),
                funds_deposited,
            });
        }
    };

    match status_resp.status.as_str() {
        "SUCCESS" => {
            // The delivered amount must come from 1Click's swap_details. Falling back to the
            // quoted amount_out would hand the contract a number nobody confirmed, and the
            // contract pays that out of its own NEP-141 balance — a pool shortfall whenever the
            // real delivery was smaller. If 1Click reports SUCCESS without an amount, the
            // delivery is simply unknown, so report it as unresolved rather than inventing one.
            let actual_amount_out = match status_resp
                .swap_details
                .as_ref()
                .and_then(|d| d.amount_out.clone())
            {
                Some(delivered) => delivered,
                None => {
                    eprintln!("1Click reported SUCCESS without swap_details.amount_out");
                    return Ok(Output {
                        success: false,
                        amount_out: None,
                        error_message: Some(
                            "1Click reported SUCCESS but did not report a delivered amount; \
                             the delivery is unconfirmed and requires operator verification."
                                .to_string(),
                        ),
                        intent_hash: Some(deposit_address.clone()),
                        funds_deposited,
                    });
                }
            };

            let intent_hash = status_resp.swap_details
                .as_ref()
                .and_then(|d| d.intent_hashes.first().cloned())
                .unwrap_or_else(|| deposit_address.clone());

            eprintln!("Swap completed: {} {} -> {} {}", amount_in, token_in, actual_amount_out, token_out);

            Ok(Output {
                success: true,
                amount_out: Some(actual_amount_out),
                error_message: None,
                intent_hash: Some(intent_hash),
                funds_deposited,
            })
        }
        "FAILED" => Ok(Output {
            success: false,
            amount_out: None,
            error_message: Some("1Click swap failed".to_string()),
            intent_hash: Some(deposit_address.clone()),
            funds_deposited,
        }),
        "REFUNDED" => Ok(Output {
            success: false,
            amount_out: None,
            error_message: Some("1Click swap was refunded — tokens returned to wallet".to_string()),
            intent_hash: Some(deposit_address.clone()),
            funds_deposited,
        }),
        other => Ok(Output {
            success: false,
            amount_out: None,
            error_message: Some(format!("1Click swap still processing after timeout (status: {})", other)),
            intent_hash: Some(deposit_address.clone()),
            funds_deposited,
        }),
    }
}

// ============================================================================
// 1Click API Functions
// ============================================================================

fn get_oneclick_quote(
    jwt: Option<&str>,
    token_in: &str,
    token_out: &str,
    amount_in: &str,
    swap_contract_id: &str,
    slippage_tolerance: u32,
) -> Result<OneClickQuoteResponse, Box<dyn std::error::Error>> {
    let deadline = get_deadline_iso8601(300);

    let request = OneClickQuoteRequest {
        dry: false,
        swap_type: "EXACT_INPUT".to_string(),
        slippage_tolerance, // basis points, 100 = 1%; caller-supplied
        origin_asset: token_in.to_string(),
        deposit_type: "INTENTS".to_string(),
        destination_asset: token_out.to_string(),
        amount: amount_in.to_string(),
        refund_to: swap_contract_id.to_string(),
        refund_type: "INTENTS".to_string(),
        recipient: swap_contract_id.to_string(),
        recipient_type: "DESTINATION_CHAIN".to_string(),
        deadline,
    };

    let url = format!("{}/v0/quote", ONECLICK_BASE_URL);
    let body = serde_json::to_string(&request)?;

    const MAX_RETRIES: u32 = 3;
    let mut last_error = String::from("no attempts made");

    for attempt in 1..=MAX_RETRIES {
        eprintln!("1Click quote attempt {}/{}", attempt, MAX_RETRIES);

        let mut req = Client::new()
            .post(&url)
            .header("Content-Type", "application/json")
            .connect_timeout(Duration::from_secs(15));
        if let Some(token) = jwt {
            req = req.header("Authorization", format!("Bearer {}", token).as_str());
        }
        match req.body(body.as_bytes()).send() {
            Ok(response) => {
                let status = response.status();
                match response.body() {
                    Ok(resp_body) => {
                        let resp_str = String::from_utf8_lossy(&resp_body);
                        if status / 100 != 2 {
                            last_error = format!("HTTP {}: {}", status, &resp_str[..resp_str.len().min(500)]);
                            eprintln!("Attempt {}: {}", attempt, last_error);
                        } else {
                            match serde_json::from_slice::<OneClickQuoteResponse>(&resp_body) {
                                Ok(quote_resp) => return Ok(quote_resp),
                                Err(e) => {
                                    last_error = format!("JSON parse error: {} body={}", e, &resp_str[..resp_str.len().min(500)]);
                                    eprintln!("Attempt {}: {}", attempt, last_error);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        last_error = format!("Failed to read response body: {}", e);
                        eprintln!("Attempt {}: {}", attempt, last_error);
                    }
                }
            }
            Err(e) => {
                last_error = format!("HTTP request failed: {}", e);
                eprintln!("Attempt {}: {}", attempt, last_error);
            }
        }

        if attempt < MAX_RETRIES {
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    Err(format!("1Click quote failed after {} retries. Last error: {}", MAX_RETRIES, last_error).into())
}

fn submit_oneclick_deposit(
    jwt: Option<&str>,
    tx_hash: &str,
    deposit_address: &str,
    near_sender_account: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let request = OneClickSubmitDeposit {
        tx_hash: tx_hash.to_string(),
        deposit_address: deposit_address.to_string(),
        near_sender_account: near_sender_account.map(|s| s.to_string()),
    };

    let url = format!("{}/v0/deposit/submit", ONECLICK_BASE_URL);

    let mut req = Client::new()
        .post(&url)
        .header("Content-Type", "application/json")
        .connect_timeout(Duration::from_secs(10));
    if let Some(token) = jwt {
        req = req.header("Authorization", format!("Bearer {}", token).as_str());
    }
    let response = req.body(serde_json::to_string(&request)?.as_bytes()).send()?;

    let status = response.status();
    if status / 100 != 2 {
        let body = response.body().unwrap_or_default();
        let body_str = String::from_utf8_lossy(&body);
        return Err(format!("1Click deposit/submit returned HTTP {}: {}", status, body_str).into());
    }

    eprintln!("1Click deposit submitted: tx={}, deposit_addr={}", tx_hash, deposit_address);
    Ok(())
}

fn poll_oneclick_status(
    jwt: Option<&str>,
    deposit_address: &str,
) -> Result<OneClickStatusResponse, Box<dyn std::error::Error>> {
    const POLL_INTERVAL_MS: u64 = 2_000;
    const POLL_TIMEOUT_MS: u64 = 120_000;
    let max_attempts = POLL_TIMEOUT_MS / POLL_INTERVAL_MS;

    let url = format!("{}/v0/status?depositAddress={}", ONECLICK_BASE_URL, deposit_address);

    for attempt in 0..max_attempts {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
        }

        let mut req = Client::new()
            .get(&url)
            .connect_timeout(Duration::from_secs(10));
        if let Some(token) = jwt {
            req = req.header("Authorization", format!("Bearer {}", token).as_str());
        }
        match req.send() {
            Ok(response) => {
                if response.status() / 100 != 2 {
                    eprintln!("1Click status poll attempt {}: HTTP {}", attempt + 1, response.status());
                    continue;
                }

                match response.body() {
                    Ok(body) => {
                        match serde_json::from_slice::<OneClickStatusResponse>(&body) {
                            Ok(status_resp) => {
                                eprintln!("1Click status (attempt {}): {}", attempt + 1, status_resp.status);

                                match status_resp.status.as_str() {
                                    "SUCCESS" | "FAILED" | "REFUNDED" => return Ok(status_resp),
                                    _ => {} // Continue polling
                                }
                            }
                            Err(e) => {
                                eprintln!("1Click status parse error (attempt {}): {}", attempt + 1, e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("1Click status body read error (attempt {}): {}", attempt + 1, e);
                    }
                }
            }
            Err(e) => {
                eprintln!("1Click status request error (attempt {}): {}", attempt + 1, e);
            }
        }
    }

    // Timeout — return processing status
    Ok(OneClickStatusResponse {
        status: "PROCESSING".to_string(),
        swap_details: None,
    })
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Format a deadline as ISO 8601 UTC string, `seconds_from_now` seconds in the future.
/// Uses Howard Hinnant's civil_from_days algorithm for correct leap year handling.
fn get_deadline_iso8601(seconds_from_now: u64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let total_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + seconds_from_now;

    let days = (total_secs / 86400) as i64;
    let time_of_day = total_secs % 86400;

    // Howard Hinnant's algorithm (basis of C++20 chrono / Rust chrono)
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.000Z",
        y, m, d, hours, minutes, seconds
    )
}
