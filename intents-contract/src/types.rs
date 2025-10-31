use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::AccountId;
use schemars::JsonSchema;

pub type Balance = u128;

pub type TokenId = AccountId;

/// Swap response from WASI execution
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone)]
#[serde(crate = "near_sdk::serde")]
pub struct SwapResponse {
    pub success: bool,
    pub amount_out: Option<String>,
    pub error_message: Option<String>,
    pub intent_hash: Option<String>,
}

/// Swap request stored in contract
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone, Debug)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde")]
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
#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub enum TokenReceiverMessage {
    Swap {
        token_out: TokenId,
        #[serde(default)]
        min_amount_out: Option<String>,
    },
}

/// Token configuration with defuse asset ID
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone, Debug)]
#[borsh(crate = "near_sdk::borsh")]
#[serde(crate = "near_sdk::serde")]
pub struct TokenConfig {
    pub symbol: String,
    pub decimals: u8,
    pub defuse_asset_id: String,
}
