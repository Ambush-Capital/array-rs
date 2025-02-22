use super::config::PoolLiquidityConfig;
use crate::{
    kamino::utils::fraction::Fraction,
    save::{error::LendingError, math::Decimal},
};
use fixed::types::I80F48;

/// Normalizes pool liquidity values from different protocols into a standard format
#[derive(Debug, Clone, Copy)]
pub struct PoolLiquidityNormalizer {
    config: PoolLiquidityConfig,
}

impl PoolLiquidityNormalizer {
    /// Create a normalizer for Save protocol
    pub const fn save() -> Self {
        Self { config: PoolLiquidityConfig::SAVE }
    }

    /// Create a normalizer for MarginFi protocol
    pub const fn marginfi() -> Self {
        Self { config: PoolLiquidityConfig::MARGINFI }
    }

    /// Create a normalizer for Kamino protocol
    pub const fn kamino() -> Self {
        Self { config: PoolLiquidityConfig::KAMINO }
    }

    /// Create a normalizer for Drift protocol
    pub const fn drift() -> Self {
        Self { config: PoolLiquidityConfig::DRIFT }
    }

    /// Normalize a value using this normalizer's configuration
    pub fn normalize_amount<T: LiquidityValue>(&self, value: T) -> Result<u128, LendingError> {
        value.normalize(self)
    }
}

// Allow creating a normalizer directly from a config
impl From<PoolLiquidityConfig> for PoolLiquidityNormalizer {
    fn from(config: PoolLiquidityConfig) -> Self {
        Self { config }
    }
}

pub trait LiquidityValue {
    fn normalize(&self, normalizer: &PoolLiquidityNormalizer) -> Result<u128, LendingError>;
}

// Core type implementations
impl LiquidityValue for Decimal {
    fn normalize(&self, normalizer: &PoolLiquidityNormalizer) -> Result<u128, LendingError> {
        let scaled_val = self.to_scaled_val().map_err(|_| LendingError::MathOverflow)?;
        // Decimal is already WAD-scaled, so divide by scale factor
        if normalizer.config.scale_factor.as_u128() == 1 {
            Ok(scaled_val)
        } else {
            scaled_val
                .checked_div(normalizer.config.scale_factor.as_u128())
                .ok_or(LendingError::MathOverflow)
        }
    }
}

impl LiquidityValue for Fraction {
    fn normalize(&self, normalizer: &PoolLiquidityNormalizer) -> Result<u128, LendingError> {
        let bits = self.to_num::<u128>();
        if normalizer.config.scale_factor.as_u128() == 1 {
            Ok(bits)
        } else {
            bits.checked_mul(normalizer.config.scale_factor.as_u128())
                .ok_or(LendingError::MathOverflow)
        }
    }
}

impl LiquidityValue for I80F48 {
    fn normalize(&self, normalizer: &PoolLiquidityNormalizer) -> Result<u128, LendingError> {
        let val = self.to_num::<u128>();
        // I80F48 is already scaled, so divide by scale factor
        if normalizer.config.scale_factor.as_u128() == 1 {
            Ok(val)
        } else {
            val.checked_mul(normalizer.config.scale_factor.as_u128())
                .ok_or(LendingError::MathOverflow)
        }
    }
}

// Primitive type implementations
impl LiquidityValue for u64 {
    fn normalize(&self, normalizer: &PoolLiquidityNormalizer) -> Result<u128, LendingError> {
        let val = *self as u128;
        // Raw values need to be scaled up
        if normalizer.config.scale_factor.as_u128() == 1 {
            Ok(val)
        } else {
            normalizer.config.scale_factor.safe_mul(val).ok_or(LendingError::MathOverflow)
        }
    }
}

impl LiquidityValue for u128 {
    fn normalize(&self, normalizer: &PoolLiquidityNormalizer) -> Result<u128, LendingError> {
        // Raw values need to be scaled up
        if normalizer.config.scale_factor.as_u128() == 1 {
            Ok(*self)
        } else {
            normalizer.config.scale_factor.safe_mul(*self).ok_or(LendingError::MathOverflow)
        }
    }
}
