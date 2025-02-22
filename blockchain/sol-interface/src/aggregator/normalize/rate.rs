use super::config::RateConfig;
use crate::{
    kamino::utils::fraction::Fraction,
    save::{error::LendingError, math::Rate},
};
use fixed::types::I80F48;

/// Normalizes rate values from different protocols into a standard format
#[derive(Debug, Clone, Copy)]
pub struct RateNormalizer {
    config: RateConfig,
}

impl RateNormalizer {
    /// Create a normalizer for Save protocol
    pub const fn save() -> Self {
        Self { config: RateConfig::SAVE }
    }

    /// Create a normalizer for MarginFi protocol
    pub const fn marginfi() -> Self {
        Self { config: RateConfig::MARGINFI }
    }

    /// Create a normalizer for Kamino protocol
    pub const fn kamino() -> Self {
        Self { config: RateConfig::KAMINO }
    }

    /// Create a normalizer for Drift protocol
    pub const fn drift() -> Self {
        Self { config: RateConfig::DRIFT }
    }

    /// Normalize a value using this normalizer's configuration
    pub fn normalize_rate<T: RateValue>(&self, value: T) -> Result<u128, LendingError> {
        value.normalize(self)
    }
}

// Allow creating a normalizer directly from a config
impl From<RateConfig> for RateNormalizer {
    fn from(config: RateConfig) -> Self {
        Self { config }
    }
}

// Trait for values that can be normalized
pub trait RateValue {
    fn normalize(&self, normalizer: &RateNormalizer) -> Result<u128, LendingError>;
}

impl RateValue for Rate {
    fn normalize(&self, normalizer: &RateNormalizer) -> Result<u128, LendingError> {
        let scaled_val = self.to_scaled_val();
        let shifted = scaled_val
            .checked_shl(normalizer.config.shift.as_u32())
            .ok_or(LendingError::MathOverflow)?;
        // The input is already WAD-scaled, so divide by scale factor
        let scaled = shifted
            .checked_div(normalizer.config.scale_factor.as_u128())
            .ok_or(LendingError::MathOverflow)?;
        // Then apply shift and multiplier

        scaled.checked_mul(normalizer.config.multiplier).ok_or(LendingError::MathOverflow)
    }
}

impl RateValue for I80F48 {
    fn normalize(&self, normalizer: &RateNormalizer) -> Result<u128, LendingError> {
        let bits = self.to_bits() as u128;
        // Then apply shift and multiplier
        let shifted =
            bits.checked_shl(normalizer.config.shift.as_u32()).ok_or(LendingError::MathOverflow)?;
        shifted.checked_mul(normalizer.config.multiplier).ok_or(LendingError::MathOverflow)
    }
}

impl RateValue for Fraction {
    fn normalize(&self, normalizer: &RateNormalizer) -> Result<u128, LendingError> {
        // Fraction is already scaled, so divide by scale factor first
        let bits = self.to_bits();
        let scaled = bits
            .checked_div(normalizer.config.scale_factor.as_u128())
            .ok_or(LendingError::MathOverflow)?;
        scaled.checked_mul(normalizer.config.multiplier).ok_or(LendingError::MathOverflow)
    }
}

impl RateValue for u128 {
    fn normalize(&self, normalizer: &RateNormalizer) -> Result<u128, LendingError> {
        // Raw u128 values are not scaled, so we need to scale them
        if *self == 0 {
            return Ok(0);
        }
        // First multiply by scale factor
        normalizer.config.scale_factor.safe_mul(*self).ok_or(LendingError::MathOverflow)
    }
}
