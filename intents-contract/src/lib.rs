mod admin;
mod types;

use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::{
    env, ext_contract, log, near_bindgen, AccountId, BorshStorageKey, Gas,
    NearToken, PanicOnDefault, PromiseError,
};

use types::{SwapRequest, SwapResponse, TokenConfig, TokenId, TokenReceiverMessage};

pub type Balance = u128;

// ============================================================================
// Constants
// ============================================================================

const TGAS: u64 = 1_000_000_000_000;

/// Gas for FT transfer
pub const GAS_FOR_FT_TRANSFER: Gas = Gas::from_gas(10 * TGAS);

/// Gas for callback
pub const CALLBACK_GAS: Gas = Gas::from_gas(50 * TGAS);

/// Minimum deposit to cover OutLayer execution
const MIN_DEPOSIT: u128 = 50_000_000_000_000_000_000_000; // 0.05 NEAR

/// OutLayer contract ID
const OUTLAYER_CONTRACT_ID: &str = "outlayer.near";

/// GitHub repo for WASI binary
const WASI_REPO: &str = "https://github.com/zavodil/intents-ark";
const WASI_COMMIT: &str = "main";

// ============================================================================
// Storage Keys
// ============================================================================

#[derive(BorshSerialize, BorshStorageKey)]
#[borsh(crate = "near_sdk::borsh")]
enum StorageKey {
    Whitelist,
    PendingSwaps,
    CollectedFees,
}

// ============================================================================
// External Contracts
// ============================================================================

/// OutLayer contract interface
#[ext_contract(ext_outlayer)]
#[allow(dead_code)]
trait OutLayer {
    fn request_execution(
        &mut self,
        code_source: near_sdk::serde_json::Value,
        resource_limits: near_sdk::serde_json::Value,
        input_data: String,
        secrets_ref: Option<near_sdk::serde_json::Value>,
        response_format: String,
        payer_account_id: Option<AccountId>,
    );
}

/// FT contract interface
#[ext_contract(ext_ft)]
pub trait FungibleToken {
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
    fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    );
}

/// OutLayer execution response
#[derive(near_sdk::serde::Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct ExecutionResponse {
    pub success: bool,
    pub output: Option<ExecutionOutput>,
    pub error: Option<String>,
}

#[derive(near_sdk::serde::Deserialize, Debug)]
#[serde(crate = "near_sdk::serde")]
pub struct ExecutionOutput {
    pub data: String,
    pub format: String,
}

/// Self callback interface
#[ext_contract(ext_self)]
#[allow(dead_code)]
trait ExtSelf {
    fn on_execution_response(
        &mut self,
        request_id: u64,
        sender_id: AccountId,
        token_in: TokenId,
        token_out: TokenId,
        amount_in: U128,
        min_amount_out: U128,
        fee_amount: U128,
        #[callback_result] result: Result<Option<ExecutionResponse>, PromiseError>,
    ) -> Option<U128>;
}

// ============================================================================
// Contract State
// ============================================================================

#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
#[borsh(crate = "near_sdk::borsh")]
#[near_bindgen]
pub struct Contract {
    /// Contract owner
    pub(crate) owner_id: AccountId,

    /// Operator account ID (used in secrets_ref)
    pub(crate) operator_id: AccountId,

    /// Pause state (stops ALL operations)
    pub(crate) paused: bool,

    /// Swap pause state (stops NEW swaps only, callbacks still work)
    pub(crate) swap_paused: bool,

    /// Whitelist: token_id => TokenConfig
    pub(crate) whitelist: LookupMap<TokenId, TokenConfig>,

    /// Active swap requests
    pub(crate) pending_swaps: LookupMap<u64, SwapRequest>,

    /// Request counter
    pub(crate) next_request_id: u64,

    /// Secrets profile name (e.g., "production")
    pub(crate) secrets_profile: String,

    /// Fee percentage in basis points (e.g., 10 = 0.1%, 100 = 1%)
    pub(crate) fee_basis_points: u16,

    /// Collected fees per token: token_id => balance
    pub(crate) collected_fees: LookupMap<TokenId, Balance>,
}

// ============================================================================
// Implementation
// ============================================================================

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(
        owner_id: AccountId,
        operator_id: Option<AccountId>,
        secrets_profile: Option<String>,
        fee_basis_points: Option<u16>,
    ) -> Self {
        Self {
            owner_id: owner_id.clone(),
            operator_id: operator_id.unwrap_or(owner_id),
            paused: false,
            swap_paused: false,
            whitelist: LookupMap::new(StorageKey::Whitelist),
            pending_swaps: LookupMap::new(StorageKey::PendingSwaps),
            next_request_id: 0,
            secrets_profile: secrets_profile.unwrap_or_else(|| "production".to_string()),
            fee_basis_points: fee_basis_points.unwrap_or(10), // Default: 0.1%
            collected_fees: LookupMap::new(StorageKey::CollectedFees),
        }
    }

    /// Handle incoming token transfers and initiate swap
    pub fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) {
        self.assert_not_paused();
        self.assert_swaps_not_paused();

        let token_in = env::predecessor_account_id();

        // Get token configs ONCE (gas optimization)
        let token_in_config = self
            .whitelist
            .get(&token_in)
            .expect("Token in not whitelisted");

        // Parse message
        let message: TokenReceiverMessage =
            serde_json::from_str(&msg).expect("Invalid token receiver message format");

        match message {
            TokenReceiverMessage::Swap {
                token_out,
                min_amount_out,
            } => {
                // Get token_out config ONCE (gas optimization)
                let token_out_config = self
                    .whitelist
                    .get(&token_out)
                    .expect("Token out not whitelisted");

                let min_amount_out_value = min_amount_out
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                // Initiate swap via OutLayer - returns promise
                self.internal_initiate_swap(
                    sender_id,
                    token_in,
                    token_out,
                    token_in_config,
                    token_out_config,
                    amount.0,
                    min_amount_out_value,
                )
            }
        }
    }

    fn internal_initiate_swap(
        &mut self,
        sender_id: AccountId,
        token_in: TokenId,
        token_out: TokenId,
        token_in_config: TokenConfig,
        token_out_config: TokenConfig,
        amount_in: Balance,
        min_amount_out: Balance,
    ) {
        // Validate
        assert_ne!(token_in, token_out, "Cannot swap token to itself");
        assert!(amount_in > 0, "Amount in must be greater than 0");

        // Validate minimum swap amount
        assert!(
            amount_in >= token_in_config.min_swap_amount,
            "Amount {} is below minimum swap amount {}",
            amount_in,
            token_in_config.min_swap_amount
        );

        // Calculate fee (in basis points: 10 = 0.1%, 100 = 1%)
        let fee_amount = (amount_in as u128)
            .saturating_mul(self.fee_basis_points as u128)
            / 10000;
        let amount_after_fee = amount_in.saturating_sub(fee_amount);

        log!(
            "💰 Fee calculation: amount={}, fee_bp={}, fee={}, after_fee={}",
            amount_in,
            self.fee_basis_points,
            fee_amount,
            amount_after_fee
        );

        let request_id = self.next_request_id;
        self.next_request_id += 1;

        // Store swap request with ORIGINAL amount (for refunds if failed)
        let swap_request = SwapRequest {
            request_id,
            sender_id: sender_id.clone(),
            token_in: token_in.clone(),
            token_out: token_out.clone(),
            amount_in, // Original amount (with fee)
            min_amount_out,
            timestamp: env::block_timestamp(),
        };

        self.pending_swaps.insert(&request_id, &swap_request);

        // Build input for WASI with REDUCED amount (after fee)
        let input_data = near_sdk::serde_json::json!({
            "sender_id": sender_id.to_string(),
            "token_in": token_in_config.defuse_asset_id,
            "token_out": token_out_config.defuse_asset_id,
            "amount_in": amount_after_fee.to_string(),  // Amount after fee
            "min_amount_out": min_amount_out.to_string(),
            "swap_contract_id": env::current_account_id().to_string(),
        })
        .to_string();

        log!(
            "🔄 Requesting swap #{} via OutLayer: {} {} → {} {} (min: {})",
            request_id,
            amount_in,
            token_in,
            min_amount_out,
            token_out,
            token_out
        );

        // Call OutLayer
        let code_source = near_sdk::serde_json::json!({
            "repo": WASI_REPO,
            "commit": WASI_COMMIT,
            "build_target": "wasm32-wasip2"
        });

        let resource_limits = near_sdk::serde_json::json!({
            "max_instructions": 100_000_000_000u64,
            "max_memory_mb": 256u32,
            "max_execution_seconds": 120u64
        });

        let secrets_ref = near_sdk::serde_json::json!({
            "profile": self.secrets_profile,
            "account_id": self.operator_id
        });

        // Create promise chain and return it to maintain execution unity
        ext_outlayer::ext(OUTLAYER_CONTRACT_ID.parse().unwrap())
            .with_attached_deposit(NearToken::from_yoctonear(MIN_DEPOSIT))
            .with_unused_gas_weight(1)
            .request_execution(
                code_source,
                resource_limits,
                input_data,
                Some(secrets_ref),
                "Json".to_string(),
                Some(sender_id.clone()),
            )
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(CALLBACK_GAS)
                    .on_execution_response(
                        request_id,
                        sender_id,
                        token_in,
                        token_out,
                        U128(amount_in),
                        U128(min_amount_out),
                        U128(fee_amount),
                    ),
            )
            .as_return(); // Return the promise to maintain execution unity
    }

    #[private]
    pub fn on_execution_response(
        &mut self,
        request_id: u64,
        sender_id: AccountId,
        token_in: TokenId,
        token_out: TokenId,
        amount_in: U128,
        min_amount_out: U128,
        fee_amount: U128,
        #[callback_result] result: Result<Option<ExecutionResponse>, PromiseError>,
    ) -> Option<U128> {
        // Remove pending swap
        self.pending_swaps.remove(&request_id);

        match result {
            Ok(Some(exec_response)) => {
                log!(
                    "✅ Execution #{} result received: success={}",
                    request_id,
                    exec_response.success
                );

                if exec_response.success {
                    // Parse SwapResponse from output JSON
                    if let Some(output) = exec_response.output {
                        match serde_json::from_str::<SwapResponse>(&output.data) {
                            Ok(swap_response) => {
                                log!(
                                    "📊 Swap data: amount_out={:?}, intent_hash={:?}",
                                    swap_response.amount_out,
                                    swap_response.intent_hash
                                );

                                if swap_response.success {
                                    if let Some(amount_out_str) = swap_response.amount_out {
                                        let amount_out: Balance = amount_out_str.parse().unwrap_or(0);

                                        // Validate minimum output amount
                                        assert!(
                                            amount_out >= min_amount_out.0,
                                            "Output amount {} is less than minimum {}",
                                            amount_out,
                                            min_amount_out.0
                                        );

                                        // Collect fee (already calculated in internal_initiate_swap)
                                        let current_fees = self.collected_fees.get(&token_in).unwrap_or(0);
                                        self.collected_fees.insert(&token_in, &(current_fees + fee_amount.0));

                                        log!(
                                            "💰 Fee collected: {} {} (total collected: {})",
                                            fee_amount.0,
                                            token_in,
                                            current_fees + fee_amount.0
                                        );

                                        // Transfer output tokens to user
                                        ext_ft::ext(token_out.clone())
                                            .with_static_gas(GAS_FOR_FT_TRANSFER)
                                            .with_attached_deposit(NearToken::from_yoctonear(1))
                                            .ft_transfer(
                                                sender_id.clone(),
                                                U128(amount_out),
                                                Some(format!(
                                                    "NEAR Intents swap completed. Intent: {}",
                                                    swap_response.intent_hash.unwrap_or_default()
                                                )),
                                            );

                                        log!(
                                            "🎉 Swap completed: {} {} -> {} {} (fee: {})",
                                            amount_in.0,
                                            token_in,
                                            amount_out,
                                            token_out,
                                            fee_amount.0
                                        );

                                        // Return Some(0) - all tokens used successfully
                                        return Some(U128(0));
                                    }
                                }

                                // Swap failed
                                env::panic_str(&format!(
                                    "Swap failed: {}",
                                    swap_response.error_message.unwrap_or_else(|| "Unknown error".to_string())
                                ));
                            }
                            Err(parse_err) => {
                                env::panic_str(&format!("Failed to parse swap response: {}", parse_err));
                            }
                        }
                    } else {
                        env::panic_str("No output data in successful execution");
                    }
                } else {
                    env::panic_str(&format!(
                        "Execution failed: {}",
                        exec_response.error.unwrap_or_else(|| "Unknown error".to_string())
                    ));
                }
            }

            Ok(None) => {
                env::panic_str("OutLayer returned no response");
            }

            Err(promise_error) => {
                env::panic_str(&format!("Promise error: {:?}", promise_error));
            }
        }
    }
    
}

