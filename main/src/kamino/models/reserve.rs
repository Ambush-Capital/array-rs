use std::{
    cmp::{max, min},
    ops::{Add, Div, Mul},
};

use borsh::{BorshDeserialize, BorshSerialize};
use bytemuck::Zeroable;
use derivative::Derivative;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use solana_program::{clock::Slot, pubkey::Pubkey};

use crate::kamino::{
    models::{
        last_update::LastUpdate, lending_result::LendingResult, referral::ReferrerTokenState,
        token_info::TokenInfo, types::CalculateBorrowResult,
    },
    utils::{
        borrow_rate_curve::BorrowRateCurve,
        consts::{
            DEFAULT_SLOT_DURATION_MS, INITIAL_COLLATERAL_RATE, PROGRAM_VERSION,
            RESERVE_CONFIG_SIZE, SLOTS_PER_SECOND, SLOTS_PER_YEAR,
        },
        errors::LendingError,
        fraction::{pow_fraction, BigFraction, Fraction, FractionExtra},
    },
};

#[derive(
    Default, Debug, PartialEq, Eq, Zeroable, BorshSerialize, BorshDeserialize, Copy, Clone,
)]
#[repr(C)]
pub struct BigFractionBytes {
    pub value: [u64; 4],
    pub padding: [u64; 2],
}

impl From<BigFraction> for BigFractionBytes {
    fn from(value: BigFraction) -> BigFractionBytes {
        BigFractionBytes { value: value.to_bits(), padding: [0; 2] }
    }
}

impl From<BigFractionBytes> for BigFraction {
    fn from(value: BigFractionBytes) -> BigFraction {
        BigFraction::from_bits(value.value)
    }
}

// TODO: fix this
// static_assertions::const_assert_eq!(RESERVE_SIZE, std::mem::size_of::<Reserve>());
static_assertions::const_assert_eq!(0, std::mem::size_of::<Reserve>() % 8);
#[derive(PartialEq, Derivative, BorshSerialize, BorshDeserialize, Zeroable)]
#[derivative(Debug)]
#[repr(C)]
pub struct Reserve {
    pub version: u64,

    pub last_update: LastUpdate,

    pub lending_market: Pubkey,

    pub farm_collateral: Pubkey,
    pub farm_debt: Pubkey,

    pub liquidity: ReserveLiquidity,

    #[derivative(Debug = "ignore")]
    pub reserve_liquidity_padding: [u64; 150],

    pub collateral: ReserveCollateral,

    #[derivative(Debug = "ignore")]
    pub reserve_collateral_padding: [u64; 150],

    pub config: ReserveConfig,

    #[derivative(Debug = "ignore")]
    pub config_padding: [u64; 117],

    pub borrowed_amount_outside_elevation_group: u64,

    pub borrowed_amounts_against_this_reserve_in_elevation_groups: [u64; 32],

    #[derivative(Debug = "ignore")]
    pub padding: [u64; 207],
}

impl Default for Reserve {
    fn default() -> Self {
        Self {
            version: 0,
            last_update: LastUpdate::default(),
            lending_market: Pubkey::default(),
            liquidity: ReserveLiquidity::default(),
            collateral: ReserveCollateral::default(),
            config: ReserveConfig::default(),
            farm_collateral: Pubkey::default(),
            farm_debt: Pubkey::default(),
            reserve_liquidity_padding: [0; 150],
            reserve_collateral_padding: [0; 150],
            config_padding: [0; 117],
            borrowed_amount_outside_elevation_group: 0,
            borrowed_amounts_against_this_reserve_in_elevation_groups: [0; 32],
            padding: [0; 207],
        }
    }
}

#[derive(
    BorshDeserialize,
    BorshSerialize,
    Debug,
    PartialEq,
    Eq,
    Clone,
    Copy,
    TryFromPrimitive,
    IntoPrimitive,
)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum ReserveFarmKind {
    Collateral = 0,
    Debt = 1,
}

impl Reserve {
    pub fn init(&mut self, params: InitReserveParams) {
        *self = Self::default();
        self.version = PROGRAM_VERSION as u64;
        self.last_update = LastUpdate::new(params.current_slot);
        self.lending_market = params.lending_market;
        self.liquidity = *params.liquidity;
        self.collateral = *params.collateral;
        self.config = *params.config;
    }

    // defaulting to 450ms slot duration, matches broadcasted values on UI from what i can tell
    pub fn slot_adjustment_factor(&self) -> f64 {
        1_000.0 / SLOTS_PER_SECOND as f64 / DEFAULT_SLOT_DURATION_MS as f64
    }

    pub fn current_borrow_rate(&self) -> Result<Fraction, LendingError> {
        let utilization_rate = self.liquidity.utilization_rate()?;

        self.config.borrow_rate_curve.get_borrow_rate(utilization_rate)
    }

    pub fn slot_adjusted_borrow_rate(&self) -> Fraction {
        let utilization_rate = self.liquidity.utilization_rate().unwrap();

        self.config.borrow_rate_curve.get_borrow_rate(utilization_rate).unwrap()
            * Fraction::from_num(self.slot_adjustment_factor())
    }

    pub fn current_borrow_apy(&self) -> Fraction {
        let fixed_rate = self.get_fixed_interest_rate();
        let apr = self.slot_adjusted_borrow_rate() + fixed_rate;

        // Change to 365 compounding periods to match TypeScript
        let compounds_per_year = 365;
        let rate_per_period = apr.to_num::<f64>() / compounds_per_year as f64;

        let base = Fraction::ONE + Fraction::from_num(rate_per_period);
        let compounded = pow_fraction(base, compounds_per_year as u32).unwrap();

        compounded - Fraction::ONE
    }

    pub fn current_supply_apr(&self) -> Fraction {
        let protocol_take_rate =
            Fraction::from_num(1.0 - self.get_protocol_take_rate().to_num::<f64>());
        let slot_adjusted_borrow_rate = self.slot_adjusted_borrow_rate();
        let current_utilization_rate = self.liquidity.utilization_rate().unwrap();

        protocol_take_rate * slot_adjusted_borrow_rate * current_utilization_rate
    }

    pub fn current_supply_apy(&self) -> Fraction {
        let apr = self.current_supply_apr();

        // Change to 365 compounding periods to match TypeScript
        let compounds_per_year = 365;
        let rate_per_period = apr.to_num::<f64>() / compounds_per_year as f64;

        let base = Fraction::ONE + Fraction::from_num(rate_per_period);
        let compounded = pow_fraction(base, compounds_per_year as u32).unwrap();

        compounded - Fraction::ONE
    }

    pub fn get_fixed_interest_rate(&self) -> Fraction {
        Fraction::from_bps(self.config.host_fixed_interest_rate_bps)
    }

    pub fn get_protocol_take_rate(&self) -> Fraction {
        Fraction::from_percent(self.config.protocol_take_rate_pct)
    }

    pub fn borrow_factor_f(&self, is_in_elevation_group: bool) -> Fraction {
        if is_in_elevation_group {
            Fraction::ONE
        } else {
            self.config.get_borrow_factor()
        }
    }

    pub fn get_farm(&self, mode: ReserveFarmKind) -> Pubkey {
        match mode {
            ReserveFarmKind::Collateral => self.farm_collateral,
            ReserveFarmKind::Debt => self.farm_debt,
        }
    }

    pub fn token_symbol(&self) -> &str {
        self.config.token_info.symbol()
    }

    pub fn collateral_exchange_rate(&self) -> LendingResult<CollateralExchangeRate> {
        let total_liquidity = self.liquidity.total_supply()?;
        self.collateral.exchange_rate(total_liquidity)
    }

    pub fn collateral_exchange_rate_ceil(&self) -> LendingResult<CollateralExchangeRate> {
        let total_liquidity = self.liquidity.total_supply()?;
        self.collateral.exchange_rate(total_liquidity)
    }

    pub fn calculate_borrow(
        &self,
        amount_to_borrow: u64,
        max_borrow_factor_adjusted_debt_value: Fraction,
        remaining_reserve_borrow: Fraction,
        referral_fee_bps: u16,
        is_in_elevation_group: bool,
        has_referrer: bool,
    ) -> Result<CalculateBorrowResult, LendingError> {
        let decimals = 10u64
            .checked_pow(self.liquidity.mint_decimals as u32)
            .ok_or(LendingError::MathOverflow)?;
        let market_price_f = self.liquidity.get_market_price_f();

        if amount_to_borrow == u64::MAX {
            let borrow_amount_f = (max_borrow_factor_adjusted_debt_value * u128::from(decimals)
                / market_price_f
                / self.borrow_factor_f(is_in_elevation_group))
            .min(remaining_reserve_borrow)
            .min(self.liquidity.available_amount.into());
            let (borrow_fee, referrer_fee) = self.config.fees.calculate_borrow_fees(
                borrow_amount_f,
                FeeCalculation::Inclusive,
                referral_fee_bps,
                has_referrer,
            )?;
            let borrow_amount: u64 = borrow_amount_f.to_floor();
            let receive_amount = borrow_amount - borrow_fee - referrer_fee;

            Ok(CalculateBorrowResult { borrow_amount_f, receive_amount, borrow_fee, referrer_fee })
        } else {
            let receive_amount = amount_to_borrow;
            let mut borrow_amount_f = Fraction::from(receive_amount);
            let (borrow_fee, referrer_fee) = self.config.fees.calculate_borrow_fees(
                borrow_amount_f,
                FeeCalculation::Exclusive,
                referral_fee_bps,
                has_referrer,
            )?;

            borrow_amount_f += Fraction::from_num(borrow_fee + referrer_fee);
            let borrow_factor_adjusted_debt_value = borrow_amount_f
                .mul(market_price_f)
                .div(u128::from(decimals))
                .mul(self.borrow_factor_f(is_in_elevation_group));
            if borrow_factor_adjusted_debt_value > max_borrow_factor_adjusted_debt_value {
                return Err(LendingError::BorrowTooLarge);
            }

            Ok(CalculateBorrowResult { borrow_amount_f, receive_amount, borrow_fee, referrer_fee })
        }
    }

    pub fn calculate_redeem_fees(&self) -> Result<u64, LendingError> {
        Ok(min(
            self.liquidity.available_amount,
            Fraction::from_bits(self.liquidity.accumulated_protocol_fees_sf).to_floor(),
        ))
    }

    pub fn deposit_limit_crossed(&self) -> Result<bool, LendingError> {
        let crossed = self.liquidity.total_supply()? > Fraction::from(self.config.deposit_limit);
        Ok(crossed)
    }

    pub fn borrow_limit_crossed(&self) -> Result<bool, LendingError> {
        let crossed = self.liquidity.total_borrow() > Fraction::from(self.config.borrow_limit);
        Ok(crossed)
    }

    pub fn get_withdraw_referrer_fees(
        &self,
        referrer_token_state: &ReferrerTokenState,
    ) -> Result<u64, LendingError> {
        let available_unclaimed_sf = min(
            referrer_token_state.amount_unclaimed_sf,
            self.liquidity.accumulated_referrer_fees_sf,
        );
        let available_unclaimed: u64 = Fraction::from_bits(available_unclaimed_sf).to_floor();
        Ok(min(available_unclaimed, self.liquidity.available_amount))
    }
}

pub struct InitReserveParams {
    pub current_slot: Slot,
    pub lending_market: Pubkey,
    pub liquidity: Box<ReserveLiquidity>,
    pub collateral: Box<ReserveCollateral>,
    pub config: Box<ReserveConfig>,
}

#[derive(Debug, PartialEq, Eq, Zeroable, BorshSerialize, BorshDeserialize)]
#[repr(C)]
pub struct ReserveLiquidity {
    pub mint_pubkey: Pubkey,
    pub supply_vault: Pubkey,
    pub fee_vault: Pubkey,
    pub available_amount: u64,
    pub borrowed_amount_sf: u128,
    pub market_price_sf: u128,
    pub market_price_last_updated_ts: u64,
    pub mint_decimals: u64,

    pub deposit_limit_crossed_slot: u64,
    pub borrow_limit_crossed_slot: u64,

    pub cumulative_borrow_rate_bsf: BigFractionBytes,
    pub accumulated_protocol_fees_sf: u128,
    pub accumulated_referrer_fees_sf: u128,
    pub pending_referrer_fees_sf: u128,
    pub absolute_referral_rate_sf: u128,
    pub token_program: Pubkey,

    pub padding2: [u64; 51],
    pub padding3: [u128; 32],
}

impl Default for ReserveLiquidity {
    fn default() -> Self {
        Self {
            mint_pubkey: Pubkey::default(),
            supply_vault: Pubkey::default(),
            fee_vault: Pubkey::default(),
            available_amount: 0,
            borrowed_amount_sf: 0,
            cumulative_borrow_rate_bsf: BigFractionBytes::from(BigFraction::from(Fraction::ONE)),
            accumulated_protocol_fees_sf: 0,
            market_price_sf: 0,
            mint_decimals: 0,
            deposit_limit_crossed_slot: 0,
            borrow_limit_crossed_slot: 0,
            accumulated_referrer_fees_sf: 0,
            pending_referrer_fees_sf: 0,
            absolute_referral_rate_sf: 0,
            market_price_last_updated_ts: 0,
            token_program: Pubkey::default(),
            padding2: [0; 51],
            padding3: [0; 32],
        }
    }
}

impl ReserveLiquidity {
    pub fn new(params: NewReserveLiquidityParams) -> Self {
        Self {
            mint_pubkey: params.mint_pubkey,
            mint_decimals: params.mint_decimals as u64,
            supply_vault: params.supply_vault,
            fee_vault: params.fee_vault,
            available_amount: 0,
            borrowed_amount_sf: 0,
            cumulative_borrow_rate_bsf: BigFractionBytes::from(BigFraction::from(Fraction::ONE)),
            accumulated_protocol_fees_sf: 0,
            market_price_sf: params.market_price_sf,
            deposit_limit_crossed_slot: 0,
            borrow_limit_crossed_slot: 0,
            accumulated_referrer_fees_sf: 0,
            pending_referrer_fees_sf: 0,
            absolute_referral_rate_sf: 0,
            market_price_last_updated_ts: 0,
            token_program: params.mint_token_program,
            padding2: [0; 51],
            padding3: [0; 32],
        }
    }

    pub fn total_supply(&self) -> LendingResult<Fraction> {
        Ok(Fraction::from(self.available_amount) + Fraction::from_bits(self.borrowed_amount_sf)
            - Fraction::from_bits(self.accumulated_protocol_fees_sf)
            - Fraction::from_bits(self.accumulated_referrer_fees_sf)
            - Fraction::from_bits(self.pending_referrer_fees_sf))
    }

    pub fn total_borrow(&self) -> Fraction {
        Fraction::from_bits(self.borrowed_amount_sf)
    }

    pub fn utilization_rate(&self) -> LendingResult<Fraction> {
        let total_supply = self.total_supply()?;
        if total_supply == Fraction::ZERO {
            return Ok(Fraction::ZERO);
        }
        Ok(Fraction::from_bits(self.borrowed_amount_sf) / total_supply)
    }

    pub fn get_market_price_f(&self) -> Fraction {
        Fraction::from_bits(self.market_price_sf)
    }
}

pub struct NewReserveLiquidityParams {
    pub mint_pubkey: Pubkey,
    pub mint_decimals: u8,
    pub mint_token_program: Pubkey,
    pub supply_vault: Pubkey,
    pub fee_vault: Pubkey,
    pub market_price_sf: u128,
}

#[derive(Debug, Default, PartialEq, Eq, Zeroable, BorshSerialize, BorshDeserialize)]
#[repr(C)]
pub struct ReserveCollateral {
    pub mint_pubkey: Pubkey,
    pub mint_total_supply: u64,
    pub supply_vault: Pubkey,
    pub padding1: [u128; 32],
    pub padding2: [u128; 32],
}

impl ReserveCollateral {
    pub fn new(params: NewReserveCollateralParams) -> Self {
        Self {
            mint_pubkey: params.mint_pubkey,
            mint_total_supply: 0,
            supply_vault: params.supply_vault,
            padding1: [0; 32],
            padding2: [0; 32],
        }
    }

    fn exchange_rate(&self, total_liquidity: Fraction) -> LendingResult<CollateralExchangeRate> {
        let rate = if self.mint_total_supply == 0 || total_liquidity == Fraction::ZERO {
            INITIAL_COLLATERAL_RATE
        } else {
            Fraction::from(self.mint_total_supply) / total_liquidity
        };

        Ok(CollateralExchangeRate(rate))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollateralExchangeRate(Fraction);

impl CollateralExchangeRate {
    pub fn collateral_to_liquidity(&self, collateral_amount: u64) -> u64 {
        self.fraction_collateral_to_liquidity(collateral_amount.into()).to_floor()
    }

    pub fn fraction_collateral_to_liquidity(&self, collateral_amount: Fraction) -> Fraction {
        collateral_amount / self.0
    }

    pub fn fraction_liquidity_to_collateral(&self, liquidity_amount: Fraction) -> Fraction {
        self.0 * liquidity_amount
    }

    pub fn liquidity_to_collateral_fraction(&self, liquidity_amount: u64) -> Fraction {
        self.0 * u128::from(liquidity_amount)
    }

    pub fn liquidity_to_collateral(&self, liquidity_amount: u64) -> u64 {
        self.liquidity_to_collateral_fraction(liquidity_amount).to_floor()
    }

    pub fn liquidity_to_collateral_ceil(&self, liquidity_amount: u64) -> u64 {
        self.liquidity_to_collateral_fraction(liquidity_amount).to_ceil()
    }
}

impl From<CollateralExchangeRate> for Fraction {
    fn from(exchange_rate: CollateralExchangeRate) -> Self {
        exchange_rate.0
    }
}

impl From<Fraction> for CollateralExchangeRate {
    fn from(fraction: Fraction) -> Self {
        Self(fraction)
    }
}

pub struct NewReserveCollateralParams {
    pub mint_pubkey: Pubkey,
    pub supply_vault: Pubkey,
}

static_assertions::const_assert_eq!(RESERVE_CONFIG_SIZE, std::mem::size_of::<ReserveConfig>());
static_assertions::const_assert_eq!(0, std::mem::size_of::<ReserveConfig>() % 8);
#[derive(
    BorshDeserialize,
    BorshSerialize,
    PartialEq,
    Eq,
    Derivative,
    Default,
    Zeroable,
    Serialize,
    Deserialize,
)]
#[derivative(Debug)]
#[serde(deny_unknown_fields)]
#[repr(C)]
pub struct ReserveConfig {
    pub status: u8,
    pub asset_tier: u8,
    pub host_fixed_interest_rate_bps: u16,
    #[serde(skip_serializing, default)]
    #[derivative(Debug = "ignore")]
    pub reserved_2: [u8; 2],
    #[serde(skip_serializing, default)]
    #[derivative(Debug = "ignore")]
    pub reserved_3: [u8; 8],
    pub protocol_take_rate_pct: u8,
    pub protocol_liquidation_fee_pct: u8,
    pub loan_to_value_pct: u8,
    pub liquidation_threshold_pct: u8,
    pub min_liquidation_bonus_bps: u16,
    pub max_liquidation_bonus_bps: u16,
    pub bad_debt_liquidation_bonus_bps: u16,
    pub deleveraging_margin_call_period_secs: u64,
    pub deleveraging_threshold_slots_per_bps: u64,
    pub fees: ReserveFees,
    pub borrow_rate_curve: BorrowRateCurve,
    pub borrow_factor_pct: u64,

    pub deposit_limit: u64,
    pub borrow_limit: u64,
    pub token_info: TokenInfo,

    pub deposit_withdrawal_cap: WithdrawalCaps,
    pub debt_withdrawal_cap: WithdrawalCaps,

    pub elevation_groups: [u8; 20],
    pub disable_usage_as_coll_outside_emode: u8,

    pub utilization_limit_block_borrowing_above: u8,

    #[serde(skip_serializing, default)]
    #[derivative(Debug = "ignore")]
    pub reserved_1: [u8; 2],

    pub borrow_limit_outside_elevation_group: u64,

    pub borrow_limit_against_this_collateral_in_elevation_group: [u64; 32],
}

impl ReserveConfig {
    pub fn get_asset_tier(&self) -> AssetTier {
        AssetTier::try_from(self.asset_tier).unwrap()
    }

    pub fn get_borrow_factor(&self) -> Fraction {
        max(Fraction::ONE, Fraction::from_percent(self.borrow_factor_pct))
    }

    pub fn status(&self) -> ReserveStatus {
        ReserveStatus::try_from(self.status).unwrap()
    }
}

#[derive(
    BorshDeserialize,
    BorshSerialize,
    TryFromPrimitive,
    IntoPrimitive,
    PartialEq,
    Eq,
    Debug,
    Clone,
    Copy,
)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum ReserveStatus {
    Active = 0,
    Obsolete = 1,
    Hidden = 2,
}

#[derive(
    BorshDeserialize,
    BorshSerialize,
    PartialEq,
    Eq,
    Default,
    Debug,
    Zeroable,
    Serialize,
    Deserialize,
)]
#[repr(C)]
pub struct WithdrawalCaps {
    pub config_capacity: i64,
    pub current_total: i64,
    pub last_interval_start_timestamp: u64,
    pub config_interval_length_seconds: u64,
}

#[derive(BorshDeserialize, BorshSerialize, Default, PartialEq, Eq, Derivative, Zeroable)]
#[derivative(Debug)]
#[repr(C)]
pub struct ReserveFees {
    pub borrow_fee_sf: u64,
    pub flash_loan_fee_sf: u64,
    #[derivative(Debug = "ignore")]
    pub padding: [u8; 8],
}

mod serde_reserve_fees {
    use std::{fmt, result::Result};

    use serde::{
        de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor},
        ser::Serialize,
    };

    use super::*;

    impl<'de> Deserialize<'de> for ReserveFees {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            #[derive(serde::Deserialize)]
            #[serde(field_identifier, rename_all = "snake_case")]
            enum Field {
                BorrowFee,
                FlashLoanFee,
            }

            struct ReserveFeesVisitor;
            impl<'de> Visitor<'de> for ReserveFeesVisitor {
                type Value = ReserveFees;

                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                    formatter.write_str("struct ReserveFees")
                }

                fn visit_seq<V>(self, mut seq: V) -> Result<ReserveFees, V::Error>
                where
                    V: SeqAccess<'de>,
                {
                    let borrow_fee_sf = seq
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                    let flash_loan_fee_sf = seq
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
                    Ok(ReserveFees { borrow_fee_sf, flash_loan_fee_sf, padding: [0; 8] })
                }

                fn visit_map<V>(self, mut map: V) -> Result<ReserveFees, V::Error>
                where
                    V: MapAccess<'de>,
                {
                    let mut borrow_fee_f: Option<Fraction> = None;
                    let mut flash_loan_fee_f: Option<Fraction> = None;
                    while let Some(key) = map.next_key()? {
                        match key {
                            Field::BorrowFee => {
                                if borrow_fee_f.is_some() {
                                    return Err(de::Error::duplicate_field("borrow_fee"));
                                }
                                borrow_fee_f = Some(map.next_value()?);
                            }
                            Field::FlashLoanFee => {
                                if flash_loan_fee_f.is_some() {
                                    return Err(de::Error::duplicate_field("flash_loan_fee"));
                                }

                                let flash_loan_fee_str: Option<String> = map.next_value()?;
                                match flash_loan_fee_str.as_deref() {
                                    Some("disabled") => {
                                        flash_loan_fee_f = None;
                                    }
                                    Some(x) => {
                                        flash_loan_fee_f =
                                            Some(Fraction::from_str(x).map_err(|_| {
                                                de::Error::custom(
                                                    "flash_loan_fee must be a fraction",
                                                )
                                            })?);
                                    }
                                    None => {
                                        return Err(de::Error::custom(
                                            "flash_loan_fee must be a fraction or 'disabled'",
                                        ));
                                    }
                                }
                            }
                        }
                    }

                    let borrow_fee_f =
                        borrow_fee_f.ok_or_else(|| de::Error::missing_field("borrow_fee"))?;
                    let flash_loan_fee_f =
                        flash_loan_fee_f.unwrap_or(Fraction::from_bits(u64::MAX.into()));
                    Ok(ReserveFees {
                        borrow_fee_sf: u64::try_from(borrow_fee_f.to_bits())
                            .map_err(|_| de::Error::custom("borrow_fee does not fit in u64"))?,
                        flash_loan_fee_sf: u64::try_from(flash_loan_fee_f.to_bits())
                            .map_err(|_| de::Error::custom("flash_loan_fee does not fit in u64"))?,
                        padding: [0; 8],
                    })
                }
            }

            const FIELDS: &[&str] = &["borrow_fee", "flash_loan_fee"];
            deserializer.deserialize_struct("ReserveFees", FIELDS, ReserveFeesVisitor)
        }
    }

    impl Serialize for ReserveFees {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            #[derive(serde::Serialize)]
            struct ReserveFeesSerde {
                borrow_fee: Fraction,
                flash_loan_fee: String,
            }

            let borrow_fee_f = Fraction::from_bits(self.borrow_fee_sf.into());

            let flash_loan_fee = if self.flash_loan_fee_sf == u64::MAX {
                "disabled".to_string()
            } else {
                Fraction::from_bits(self.flash_loan_fee_sf.into()).to_string()
            };

            let fees = ReserveFeesSerde { borrow_fee: borrow_fee_f, flash_loan_fee };
            fees.serialize(serializer)
        }
    }
}

impl ReserveFees {
    pub fn calculate_borrow_fees(
        &self,
        borrow_amount: Fraction,
        fee_calculation: FeeCalculation,
        referral_fee_bps: u16,
        has_referrer: bool,
    ) -> Result<(u64, u64), LendingError> {
        self.calculate_fees(
            borrow_amount,
            self.borrow_fee_sf,
            fee_calculation,
            referral_fee_bps,
            has_referrer,
        )
    }

    pub fn calculate_flash_loan_fees(
        &self,
        flash_loan_amount_f: Fraction,
        referral_fee_bps: u16,
        has_referrer: bool,
    ) -> Result<(u64, u64), LendingError> {
        let (protocol_fee, referral_fee) = self.calculate_fees(
            flash_loan_amount_f,
            self.flash_loan_fee_sf,
            FeeCalculation::Exclusive,
            referral_fee_bps,
            has_referrer,
        )?;

        Ok((protocol_fee, referral_fee))
    }

    fn calculate_fees(
        &self,
        amount: Fraction,
        fee_sf: u64,
        fee_calculation: FeeCalculation,
        referral_fee_bps: u16,
        has_referrer: bool,
    ) -> Result<(u64, u64), LendingError> {
        let borrow_fee_rate = Fraction::from_bits(fee_sf.into());
        let referral_fee_rate = Fraction::from_bps(referral_fee_bps);
        if borrow_fee_rate > Fraction::ZERO && amount > Fraction::ZERO {
            let need_to_assess_referral_fee = referral_fee_rate > Fraction::ZERO && has_referrer;
            let minimum_fee = 1u64;
            let borrow_fee_amount = match fee_calculation {
                FeeCalculation::Exclusive => amount.mul(borrow_fee_rate),
                FeeCalculation::Inclusive => {
                    let borrow_fee_rate = borrow_fee_rate.div(borrow_fee_rate.add(Fraction::ONE));
                    amount.mul(borrow_fee_rate)
                }
            };

            let borrow_fee_f = borrow_fee_amount.max(minimum_fee.into());
            if borrow_fee_f >= amount {
                return Err(LendingError::BorrowTooSmall);
            }

            let borrow_fee: u64 = borrow_fee_f.to_round();
            let referral_fee = if need_to_assess_referral_fee {
                if referral_fee_bps == 10_000 {
                    borrow_fee
                } else {
                    let referral_fee_f = borrow_fee_f * referral_fee_rate;
                    referral_fee_f.to_floor::<u64>()
                }
            } else {
                0
            };

            let protocol_fee = borrow_fee - referral_fee;

            Ok((protocol_fee, referral_fee))
        } else {
            Ok((0, 0))
        }
    }
}

#[derive(BorshDeserialize, BorshSerialize, Debug, PartialEq, Eq)]
#[borsh(use_discriminant = true)]
pub enum FeeCalculation {
    Exclusive,
    Inclusive,
}

#[derive(
    BorshDeserialize, BorshSerialize, Debug, PartialEq, Eq, IntoPrimitive, TryFromPrimitive,
)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum AssetTier {
    Regular = 0,
    IsolatedCollateral = 1,
    IsolatedDebt = 2,
}

pub fn approximate_compounded_interest(rate: Fraction, elapsed_slots: u64) -> Fraction {
    let base = rate / u128::from(SLOTS_PER_YEAR);
    match elapsed_slots {
        0 => return Fraction::ONE,
        1 => return Fraction::ONE + base,
        2 => return (Fraction::ONE + base) * (Fraction::ONE + base),
        3 => return (Fraction::ONE + base) * (Fraction::ONE + base) * (Fraction::ONE + base),
        4 => {
            let pow_two = (Fraction::ONE + base) * (Fraction::ONE + base);
            return pow_two * pow_two;
        }
        _ => (),
    }

    let exp: u128 = elapsed_slots.into();
    let exp_minus_one = exp.wrapping_sub(1);
    let exp_minus_two = exp.wrapping_sub(2);

    let base_power_two = base * base;
    let base_power_three = base_power_two * base;

    let first_term = base * exp;

    let second_term = (base_power_two * exp * exp_minus_one) / 2;

    let third_term = (base_power_three * exp * exp_minus_one * exp_minus_two) / 6;

    Fraction::ONE + first_term + second_term + third_term
}
