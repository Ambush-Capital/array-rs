use borsh::{BorshDeserialize, BorshSerialize};
use bytemuck::Zeroable;
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use serde_values::*;
use solana_program::pubkey::Pubkey;

use crate::kamino::utils::consts::{
    CLOSE_TO_INSOLVENCY_RISKY_LTV, GLOBAL_ALLOWED_BORROW_VALUE,
    GLOBAL_UNHEALTHY_BORROW_VALUE, LENDING_MARKET_SIZE, LIQUIDATION_CLOSE_FACTOR,
    LIQUIDATION_CLOSE_VALUE, MAX_LIQUIDATABLE_VALUE_AT_ONCE, MIN_NET_VALUE_IN_OBLIGATION
};

use crate::kamino::utils::serde_helpers::{serde_string, serde_utf_string, serde_bool_u8};

static_assertions::const_assert_eq!(LENDING_MARKET_SIZE, std::mem::size_of::<LendingMarket>());
static_assertions::const_assert_eq!(0, std::mem::size_of::<LendingMarket>() % 8);
#[derive(BorshSerialize, BorshDeserialize, PartialEq, Eq, Derivative, Zeroable, Deserialize, Serialize)]
#[derivative(Debug)]
#[serde(deny_unknown_fields)]
#[repr(C)]
pub struct LendingMarket {
    pub version: u64,
    pub bump_seed: u64,
    #[serde(with = "serde_string", default)]
    pub lending_market_owner: Pubkey,
    #[serde(with = "serde_string", default)]
    pub lending_market_owner_cached: Pubkey,
    #[serde(with = "serde_utf_string", default)]
    pub quote_currency: [u8; 32],

    pub referral_fee_bps: u16,

    #[serde(with = "serde_bool_u8", default)]
    pub emergency_mode: u8,

    #[serde(with = "serde_bool_u8", default)]
    pub autodeleverage_enabled: u8,

    #[serde(with = "serde_bool_u8", default)]
    pub borrow_disabled: u8,

    pub price_refresh_trigger_to_max_age_pct: u8,

    pub liquidation_max_debt_close_factor_pct: u8,
    pub insolvency_risk_unhealthy_ltv_pct: u8,
    pub min_full_liquidation_value_threshold: u64,

    pub max_liquidatable_debt_market_value_at_once: u64,
    pub global_unhealthy_borrow_value: u64,
    pub global_allowed_borrow_value: u64,
    
    #[serde(with = "serde_string", default)]
    pub risk_council: Pubkey,

    #[serde(skip_deserializing, skip_serializing, default)]
    #[derivative(Debug = "ignore")]
    pub reserved1: [u8; 8],

    pub elevation_groups: [ElevationGroup; 32],
    #[serde(skip_deserializing, skip_serializing, default = "default_padding_90")]
    pub elevation_group_padding: [u64; 90],

    #[serde(serialize_with = "serialize_min_net_value", deserialize_with = "deserialize_min_net_value")]
    pub min_net_value_in_obligation_sf: u128,

    pub min_value_skip_liquidation_ltv_bf_checks: u64,

    #[serde(with = "serde_utf_string", default)]
    pub name: [u8; 32],

    #[serde(skip_deserializing, skip_serializing, default = "default_padding_173")]
    #[derivative(Debug = "ignore")]
    pub padding1: [u64; 173],
}


fn default_padding_173() -> [u64; 173] {
    [0; 173]
}


fn default_padding_90() -> [u64; 90] {
    [0; 90]
}

impl Default for LendingMarket {
    fn default() -> Self {
        Self {
            version: 0,
            bump_seed: 0,
            lending_market_owner: Pubkey::default(),
            risk_council: Pubkey::default(),
            quote_currency: [0; 32],
            lending_market_owner_cached: Pubkey::default(),
            emergency_mode: 0,
            borrow_disabled: 0,
            autodeleverage_enabled: 0,
            liquidation_max_debt_close_factor_pct: LIQUIDATION_CLOSE_FACTOR,
            insolvency_risk_unhealthy_ltv_pct: CLOSE_TO_INSOLVENCY_RISKY_LTV,
            max_liquidatable_debt_market_value_at_once: MAX_LIQUIDATABLE_VALUE_AT_ONCE,
            global_allowed_borrow_value: GLOBAL_ALLOWED_BORROW_VALUE,
            global_unhealthy_borrow_value: GLOBAL_UNHEALTHY_BORROW_VALUE,
            min_full_liquidation_value_threshold: LIQUIDATION_CLOSE_VALUE,
            reserved1: [0; 8],
            referral_fee_bps: 0,
            price_refresh_trigger_to_max_age_pct: 0,
            elevation_groups: [ElevationGroup::default(); 32],
            min_value_skip_liquidation_ltv_bf_checks: 0,
            elevation_group_padding: [0; 90],
            min_net_value_in_obligation_sf: MIN_NET_VALUE_IN_OBLIGATION.to_bits(),
            name: [0; 32],
            padding1: [0; 173],
        }
    }
}


#[derive(BorshSerialize, BorshDeserialize, Derivative, PartialEq, Eq, Zeroable, Copy, Clone)]
#[derivative(Debug)]
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
#[repr(C)]
pub struct ElevationGroup {
    pub max_liquidation_bonus_bps: u16,
    pub id: u8,
    pub ltv_pct: u8,
    pub liquidation_threshold_pct: u8,
    pub allow_new_loans: u8,
    pub max_reserves_as_collateral: u8,

    #[derivative(Debug = "ignore")]
    #[serde(skip_deserializing, skip_serializing, default)]
    pub padding_0: u8,

    #[serde(with = "serde_string", default)]
    pub debt_reserve: Pubkey,
    #[derivative(Debug = "ignore")]
    #[serde(skip_deserializing, skip_serializing, default)]
    pub padding_1: [u64; 4],
}

impl Default for ElevationGroup {
    fn default() -> Self {
        let mut default = Self::zeroed();
        default.max_reserves_as_collateral = u8::MAX;
        default
    }
}


mod serde_values {
    use std::result::Result;

    use serde::{
        de::{self, Deserialize, Deserializer},
        Serializer,
    };

    use crate::kamino::utils::fraction::Fraction;

    pub fn serialize_min_net_value<S>(value: &u128, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let min_net_action_value_f = Fraction::from_bits(*value);
        serializer.serialize_str(&min_net_action_value_f.to_string())
    }

    pub fn deserialize_min_net_value<'de, D>(deserializer: D) -> Result<u128, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let net_value_action_f = Fraction::from_str(&s)
            .map_err(|_| de::Error::custom("min_net_value must be a fraction"))?;

        Ok(net_value_action_f.to_bits())
    }
}