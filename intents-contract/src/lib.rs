mod types;

use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, UnorderedSet};
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
    TokenConfigs,
    PendingSwaps,
}

// ============================================================================
// External Contracts
// ============================================================================

/// OutLayer contract interface
#[ext_contract(ext_outlayer)]
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

/// Self callback interface
#[ext_contract(ext_self)]
trait ExtSelf {
    fn on_swap_result(
        &mut self,
        request_id: u64,
        sender_id: AccountId,
        token_in: TokenId,
        token_out: TokenId,
        amount_in: U128,
        #[callback_result] result: Result<Option<SwapResponse>, PromiseError>,
    ) -> String;
}

// ============================================================================
// Contract State
// ============================================================================

#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
#[borsh(crate = "near_sdk::borsh")]
#[near_bindgen]
pub struct Contract {
    /// Contract owner
    owner_id: AccountId,

    /// Operator account ID (used in secrets_ref)
    operator_id: AccountId,

    /// Pause state
    paused: bool,

    /// Whitelist of supported tokens
    whitelist: UnorderedSet<TokenId>,

    /// Token configurations (symbol, decimals, defuse_asset_id)
    token_configs: LookupMap<TokenId, TokenConfig>,

    /// Active swap requests
    pending_swaps: LookupMap<u64, SwapRequest>,

    /// Request counter
    next_request_id: u64,

    /// Secrets profile name (e.g., "production")
    secrets_profile: String,
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
    ) -> Self {
        Self {
            owner_id: owner_id.clone(),
            operator_id: operator_id.unwrap_or(owner_id),
            paused: false,
            whitelist: UnorderedSet::new(StorageKey::Whitelist),
            token_configs: LookupMap::new(StorageKey::TokenConfigs),
            pending_swaps: LookupMap::new(StorageKey::PendingSwaps),
            next_request_id: 0,
            secrets_profile: secrets_profile.unwrap_or_else(|| "production".to_string()),
        }
    }

    /// Handle incoming token transfers and initiate swap
    pub fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> U128 {
        self.assert_not_paused();

        let token_in = env::predecessor_account_id();
        self.assert_token_whitelisted(&token_in);

        // Parse message
        let message: TokenReceiverMessage =
            serde_json::from_str(&msg).expect("Invalid token receiver message format");

        match message {
            TokenReceiverMessage::Swap {
                token_out,
                min_amount_out,
            } => {
                self.assert_token_whitelisted(&token_out);

                let min_amount_out_value = min_amount_out
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                // Initiate swap via OutLayer
                self.internal_initiate_swap(
                    sender_id,
                    token_in,
                    token_out,
                    amount.0,
                    min_amount_out_value,
                );

                // Return 0 to keep all tokens
                U128(0)
            }
        }
    }

    fn internal_initiate_swap(
        &mut self,
        sender_id: AccountId,
        token_in: TokenId,
        token_out: TokenId,
        amount_in: Balance,
        min_amount_out: Balance,
    ) {
        // Validate
        assert_ne!(token_in, token_out, "Cannot swap token to itself");
        assert!(amount_in > 0, "Amount in must be greater than 0");

        let request_id = self.next_request_id;
        self.next_request_id += 1;

        // Store swap request
        let swap_request = SwapRequest {
            request_id,
            sender_id: sender_id.clone(),
            token_in: token_in.clone(),
            token_out: token_out.clone(),
            amount_in,
            min_amount_out,
            timestamp: env::block_timestamp(),
        };

        self.pending_swaps.insert(&request_id, &swap_request);

        // Get token configs
        let token_in_config = self
            .token_configs
            .get(&token_in)
            .expect("Token in config not found");
        let token_out_config = self
            .token_configs
            .get(&token_out)
            .expect("Token out config not found");

        // Build input for WASI
        let input_data = near_sdk::serde_json::json!({
            "sender_id": sender_id.to_string(),
            "token_in": token_in_config.defuse_asset_id,
            "token_out": token_out_config.defuse_asset_id,
            "amount_in": amount_in.to_string(),
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
                    .on_swap_result(
                        request_id,
                        sender_id,
                        token_in,
                        token_out,
                        U128(amount_in),
                    ),
            );
    }

    #[private]
    pub fn on_swap_result(
        &mut self,
        request_id: u64,
        sender_id: AccountId,
        token_in: TokenId,
        token_out: TokenId,
        amount_in: U128,
        #[callback_result] result: Result<Option<SwapResponse>, PromiseError>,
    ) -> String {
        // Remove pending swap
        self.pending_swaps.remove(&request_id);

        match result {
            Ok(Some(swap_response)) => {
                log!(
                    "✅ Swap #{} result received: success={}, amount_out={:?}, intent_hash={:?}",
                    request_id,
                    swap_response.success,
                    swap_response.amount_out,
                    swap_response.intent_hash
                );

                if swap_response.success {
                    if let Some(amount_out_str) = swap_response.amount_out {
                        let amount_out: Balance = amount_out_str.parse().unwrap_or(0);

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
                            "🎉 Swap completed: {} {} -> {} {}",
                            amount_in.0,
                            token_in,
                            amount_out,
                            token_out
                        );

                        return format!(
                            "Swap successful: {} {} -> {} {}",
                            amount_in.0, token_in, amount_out, token_out
                        );
                    }
                }

                // Swap failed - refund input tokens
                log!(
                    "❌ Swap failed: {}. Refunding {} {} to {}",
                    swap_response.error_message.unwrap_or_default(),
                    amount_in.0,
                    token_in,
                    sender_id
                );

                ext_ft::ext(token_in.clone())
                    .with_static_gas(GAS_FOR_FT_TRANSFER)
                    .with_attached_deposit(NearToken::from_yoctonear(1))
                    .ft_transfer(
                        sender_id.clone(),
                        amount_in,
                        Some("Swap failed - refund".to_string()),
                    );

                format!("Swap failed: refunded {} {}", amount_in.0, token_in)
            }

            Ok(None) => {
                log!("❌ OutLayer execution failed - refunding");

                ext_ft::ext(token_in.clone())
                    .with_static_gas(GAS_FOR_FT_TRANSFER)
                    .with_attached_deposit(NearToken::from_yoctonear(1))
                    .ft_transfer(
                        sender_id.clone(),
                        amount_in,
                        Some("OutLayer execution failed - refund".to_string()),
                    );

                "OutLayer execution failed - refunded".to_string()
            }

            Err(promise_error) => {
                log!("❌ Promise error: {:?}", promise_error);

                ext_ft::ext(token_in.clone())
                    .with_static_gas(GAS_FOR_FT_TRANSFER)
                    .with_attached_deposit(NearToken::from_yoctonear(1))
                    .ft_transfer(
                        sender_id.clone(),
                        amount_in,
                        Some("Promise error - refund".to_string()),
                    );

                format!("Promise error: {:?} - refunded", promise_error)
            }
        }
    }
}

// ============================================================================
// Admin Functions
// ============================================================================

impl Contract {
    fn assert_owner(&self) {
        assert_eq!(
            env::predecessor_account_id(),
            self.owner_id,
            "Only owner can call this method"
        );
    }

    fn assert_not_paused(&self) {
        assert!(!self.paused, "Contract is paused");
    }

    fn assert_token_whitelisted(&self, token_id: &TokenId) {
        assert!(
            self.whitelist.contains(token_id),
            "Token is not whitelisted"
        );
    }
}

#[near_bindgen]
impl Contract {
    pub fn set_owner(&mut self, new_owner_id: AccountId) {
        self.assert_owner();
        self.owner_id = new_owner_id;
        log!("Owner changed to {}", self.owner_id);
    }

    pub fn set_operator(&mut self, new_operator_id: AccountId) {
        self.assert_owner();
        self.operator_id = new_operator_id;
        log!("Operator changed to {}", self.operator_id);
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.assert_owner();
        self.paused = paused;
        log!("Contract {}", if paused { "paused" } else { "unpaused" });
    }

    pub fn set_secrets_profile(&mut self, profile: String) {
        self.assert_owner();
        self.secrets_profile = profile.clone();
        log!("Secrets profile set to {}", profile);
    }

    pub fn whitelist_token(
        &mut self,
        token_id: TokenId,
        symbol: String,
        decimals: u8,
        defuse_asset_id: String,
    ) {
        self.assert_owner();

        self.whitelist.insert(&token_id);
        self.token_configs.insert(
            &token_id,
            &TokenConfig {
                symbol: symbol.clone(),
                decimals,
                defuse_asset_id: defuse_asset_id.clone(),
            },
        );

        log!(
            "Token {} whitelisted: {} ({})",
            token_id,
            symbol,
            defuse_asset_id
        );
    }

    pub fn remove_token_from_whitelist(&mut self, token_id: TokenId) {
        self.assert_owner();

        self.whitelist.remove(&token_id);
        self.token_configs.remove(&token_id);

        log!("Token {} removed from whitelist", token_id);
    }

    pub fn get_config(&self) -> near_sdk::serde_json::Value {
        near_sdk::serde_json::json!({
            "owner_id": self.owner_id,
            "operator_id": self.operator_id,
            "paused": self.paused,
            "secrets_profile": self.secrets_profile,
            "next_request_id": self.next_request_id,
            "whitelisted_tokens_count": self.whitelist.len()
        })
    }

    pub fn get_token_config(&self, token_id: TokenId) -> Option<TokenConfig> {
        self.token_configs.get(&token_id)
    }

    pub fn is_token_whitelisted(&self, token_id: TokenId) -> bool {
        self.whitelist.contains(&token_id)
    }

    pub fn get_pending_swap(&self, request_id: u64) -> Option<SwapRequest> {
        self.pending_swaps.get(&request_id)
    }
}
