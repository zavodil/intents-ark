use near_sdk::{env, log, AccountId, NearToken};
use near_sdk::json_types::U128;

use crate::{ext_ft, types::{TokenConfig, TokenId}, Balance, Contract, GAS_FOR_FT_TRANSFER};

// ============================================================================
// Internal Helper Functions
// ============================================================================

impl Contract {
    pub(crate) fn assert_owner(&self) {
        assert_eq!(
            env::predecessor_account_id(),
            self.owner_id,
            "Only owner can call this method"
        );
    }

    pub(crate) fn assert_not_paused(&self) {
        assert!(!self.paused, "Contract is paused");
    }

    pub(crate) fn assert_swaps_not_paused(&self) {
        assert!(!self.swap_paused, "Swaps are paused");
    }
}

// ============================================================================
// Admin Functions
// ============================================================================

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

    pub fn set_swap_paused(&mut self, swap_paused: bool) {
        self.assert_owner();
        self.swap_paused = swap_paused;
        log!("Swaps {}", if swap_paused { "paused (new swaps disabled, callbacks still work)" } else { "unpaused" });
    }

    pub fn set_secrets_profile(&mut self, profile: String) {
        self.assert_owner();
        self.secrets_profile = profile.clone();
        log!("Secrets profile set to {}", profile);
    }

    pub fn whitelist_token(
        &mut self,
        token_id: TokenId,
        defuse_asset_id: Option<String>,
        min_swap_amount: U128,
    ) {
        self.assert_owner();

        // Generate defuse_asset_id if not provided: "nep141:{token_id}"
        let asset_id = defuse_asset_id.unwrap_or_else(|| format!("nep141:{}", token_id));

        let config = TokenConfig {
            defuse_asset_id: asset_id.clone(),
            min_swap_amount: min_swap_amount.0,
        };

        self.whitelist.insert(&token_id, &config);

        log!(
            "Token {} whitelisted with defuse_asset_id: {}, min_swap_amount: {}",
            token_id,
            asset_id,
            min_swap_amount.0
        );
    }

    pub fn update_token_config(
        &mut self,
        token_id: TokenId,
        defuse_asset_id: Option<String>,
        min_swap_amount: Option<U128>,
    ) {
        self.assert_owner();

        let mut config = self.whitelist
            .get(&token_id)
            .expect("Token not in whitelist");

        if let Some(asset_id) = defuse_asset_id {
            config.defuse_asset_id = asset_id;
        }

        if let Some(min_amount) = min_swap_amount {
            config.min_swap_amount = min_amount.0;
        }

        self.whitelist.insert(&token_id, &config);

        log!(
            "Token {} config updated: defuse_asset_id={}, min_swap_amount={}",
            token_id,
            config.defuse_asset_id,
            config.min_swap_amount
        );
    }

    pub fn set_fee_percentage(&mut self, fee_basis_points: u16) {
        self.assert_owner();
        assert!(fee_basis_points <= 1000, "Fee cannot exceed 10%");

        self.fee_basis_points = fee_basis_points;
        log!("Fee set to {} basis points ({}%)", fee_basis_points, fee_basis_points as f64 / 100.0);
    }

    pub fn withdraw_fees(&mut self, token_id: TokenId, amount: Option<Balance>) {
        self.assert_owner();

        let available_fees = self.collected_fees.get(&token_id).unwrap_or(0);
        assert!(available_fees > 0, "No fees collected for this token");

        let withdraw_amount = amount.unwrap_or(available_fees);
        assert!(
            withdraw_amount <= available_fees,
            "Cannot withdraw {} - only {} available",
            withdraw_amount,
            available_fees
        );

        // Update collected fees
        let remaining_fees = available_fees.saturating_sub(withdraw_amount);
        if remaining_fees > 0 {
            self.collected_fees.insert(&token_id, &remaining_fees);
        } else {
            self.collected_fees.remove(&token_id);
        }

        // Transfer tokens to owner
        ext_ft::ext(token_id.clone())
            .with_static_gas(GAS_FOR_FT_TRANSFER)
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .ft_transfer(
                self.owner_id.clone(),
                near_sdk::json_types::U128(withdraw_amount),
                Some(format!("Fee withdrawal")),
            );

        log!(
            "Withdrew {} {} in fees to {} (remaining: {})",
            withdraw_amount,
            token_id,
            self.owner_id,
            remaining_fees
        );
    }

    pub fn remove_token_from_whitelist(&mut self, token_id: TokenId) {
        self.assert_owner();
        self.whitelist.remove(&token_id);
        log!("Token {} removed from whitelist", token_id);
    }

    pub fn get_config(&self) -> near_sdk::serde_json::Value {
        near_sdk::serde_json::json!({
            "owner_id": self.owner_id,
            "operator_id": self.operator_id,
            "paused": self.paused,
            "swap_paused": self.swap_paused,
            "secrets_profile": self.secrets_profile,
            "next_request_id": self.next_request_id,
            "fee_basis_points": self.fee_basis_points,
            "fee_percentage": format!("{}%", self.fee_basis_points as f64 / 100.0),
        })
    }

    pub fn get_token_config(&self, token_id: TokenId) -> Option<TokenConfig> {
        self.whitelist.get(&token_id)
    }

    pub fn get_collected_fees(&self, token_id: TokenId) -> Balance {
        self.collected_fees.get(&token_id).unwrap_or(0)
    }

    pub fn is_swap_paused(&self) -> bool {
        self.swap_paused
    }

    pub fn is_token_whitelisted(&self, token_id: TokenId) -> bool {
        self.whitelist.get(&token_id).is_some()
    }

    pub fn get_pending_swap(&self, request_id: u64) -> Option<crate::types::SwapRequest> {
        self.pending_swaps.get(&request_id)
    }
}
