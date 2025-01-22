use anchor_lang::prelude::*;
use bytemuck::Zeroable;
use enum_dispatch::enum_dispatch;
use fixed::types::I80F48;

use crate::marginfi::utils::prelude::*;

use anchor_lang::prelude::borsh;

#[repr(u8)]
#[derive(Copy, Clone, Debug, AnchorSerialize, AnchorDeserialize, PartialEq, Eq, Zeroable)]
pub enum OracleSetup {
    None,
    PythLegacy,
    SwitchboardV2,
    PythPushOracle,
    SwitchboardPull,
    StakedWithPythPush,
}

#[derive(Copy, Clone, Debug)]
pub enum PriceBias {
    Low,
    High,
}

#[derive(Copy, Clone, Debug)]
pub enum OraclePriceType {
    /// Time weighted price
    /// EMA for PythEma
    TimeWeighted,
    /// Real time price
    RealTime,
}

#[enum_dispatch]
pub trait PriceAdapter {
    fn get_price_of_type(
        &self,
        oracle_price_type: OraclePriceType,
        bias: Option<PriceBias>,
    ) -> MarginfiResult<I80F48>;
}
pub struct Price {
    /// Price.
    // #[serde(with = "utils::as_string")] // To ensure accuracy on conversion to json.
    // #[schemars(with = "String")]
    pub price: i64,
    /// Confidence interval.
    // #[serde(with = "utils::as_string")]
    // #[schemars(with = "String")]
    pub conf: u64,
    /// Exponent.
    pub expo: i32,
    /// Publish time.
    pub publish_time: UnixTimestamp,
}

pub type UnixTimestamp = i64;
