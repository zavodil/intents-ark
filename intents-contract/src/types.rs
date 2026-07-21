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
    /// True once token_in has left the contract's NEP-141 balance into intents.near.
    /// On a failed swap this means the funds are NOT recoverable via the standard NEP-141
    /// refund (a panic would pay that refund out of the shared pool while the user's own
    /// tokens sit in the contract's intents balance); the incident is logged for manual
    /// operator recovery instead. Defaults to false for backward compatibility, so an older
    /// WASI that does not emit this field keeps the previous refund-on-panic behaviour.
    #[serde(default)]
    pub funds_deposited: bool,
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
        /// Slippage tolerance in basis points (100 = 1%) forwarded to the 1Click quote.
        /// Optional: when absent, `DEFAULT_SLIPPAGE_TOLERANCE_BPS` is used, so callers that
        /// predate this field keep the previous behaviour unchanged. A wider tolerance cannot
        /// be used to bypass `min_amount_out`: it lowers the quote's worst-case output, which
        /// makes the WASI's pre-deposit check reject sooner. Only a sanity bound (< 100%) is
        /// enforced, in `ft_on_transfer`.
        #[serde(default)]
        slippage_tolerance: Option<u32>,
    },
}

