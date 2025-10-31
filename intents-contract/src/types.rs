use near_sdk::{near, AccountId};

pub type Balance = u128;

pub type TokenId = AccountId;

/// Token configuration for whitelist
#[derive(Clone, Debug)]
#[near(serializers=[borsh, json])]
pub struct TokenConfig {
    /// Defuse asset identifier (e.g., "nep141:wrap.near")
    pub defuse_asset_id: String,
    /// Minimum swap amount (in token's smallest unit)
    pub min_swap_amount: Balance,
}

/// Swap response from WASI execution
#[derive(Clone, Debug)]
#[near(serializers=[borsh, json])]
pub struct SwapResponse {
    pub success: bool,
    pub amount_out: Option<String>,
    pub error_message: Option<String>,
    pub intent_hash: Option<String>,
}

/// Swap request stored in contract
#[derive(Clone, Debug)]
#[near(serializers=[borsh, json])]
pub struct SwapRequest {
    pub request_id: u64,
    pub sender_id: AccountId,
    pub token_in: TokenId,
    pub token_out: TokenId,
    pub amount_in: Balance,
    pub min_amount_out: Balance,
    pub timestamp: u64,
}

/// Message format for ft_transfer_call
#[near(serializers=[borsh, json])]
pub enum TokenReceiverMessage {
    Swap {
        token_out: TokenId,
        #[serde(default)]
        min_amount_out: Option<String>,
    },
}

