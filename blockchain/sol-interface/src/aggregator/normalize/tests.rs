#[cfg(test)]
use crate::{
    aggregator::normalize::{
        config::RateConfig, pool_liquidity::PoolLiquidityNormalizer, rate::RateNormalizer,
        scale_factor::ScaleFactor, shift::Shift,
    },
    kamino::utils::fraction::Fraction,
    save::math::{Decimal, Rate},
};
use fixed::types::I80F48;

// Helper function to create a WAD-scaled value
fn wad(value: u128) -> u128 {
    value * 1_000_000_000_000_000_000 // 1e18
}

#[test]
fn test_rate_normalizer_save_protocol() {
    let normalizer = RateNormalizer::save();

    // Test with Rate type (typical APR value like 5%)
    let rate = Rate::from_percent(5);
    let normalized = normalizer.normalize_rate(rate).unwrap();
    assert_eq!(normalized, 5000); // 5% * 1000 (multiplier) = 5000

    // Test with I80F48 type
    let i80f48_val = I80F48::from_num(0.05); // 5%
    let normalized = normalizer.normalize_rate(i80f48_val).unwrap();
    assert_eq!(normalized, 5000);

    // Test with Fraction
    let fraction = Fraction::from_num(0.05);
    let normalized = normalizer.normalize_rate(fraction).unwrap();
    assert_eq!(normalized, 5000);
}

#[test]
fn test_rate_normalizer_marginfi_protocol() {
    let normalizer = RateNormalizer::marginfi();

    // Test with different rate values
    let rate = Rate::from_percent(10); // 10%
    let normalized = normalizer.normalize_rate(rate).unwrap();
    assert_eq!(normalized, 10000); // 10% * 1000 = 10000

    // Test edge case - very small rate
    let rate = Rate::from_percent(1); // 1%
    let normalized = normalizer.normalize_rate(rate).unwrap();
    assert_eq!(normalized, 1000);
}

#[test]
fn test_pool_liquidity_normalizer_save() {
    let normalizer = PoolLiquidityNormalizer::save();

    // Test with Decimal
    let decimal = Decimal::from(1_000_000u64); // 1M tokens
    let normalized = normalizer.normalize_amount(decimal).unwrap();
    assert_eq!(normalized, 1_000_000);

    // Test with u64
    let amount = 1_000_000u64;
    let normalized = normalizer.normalize_amount(amount).unwrap();
    assert_eq!(normalized, 1_000_000);
}

#[test]
fn test_pool_liquidity_normalizer_drift() {
    let normalizer = PoolLiquidityNormalizer::drift();

    // Test with regular values
    let amount = 1_000_000u64;
    let normalized = normalizer.normalize_amount(amount).unwrap();
    assert_eq!(normalized, amount as u128 * 1_000_000_000_000_000); // 1e15 scaling

    // Test with zero
    let amount = 0u64;
    let normalized = normalizer.normalize_amount(amount).unwrap();
    assert_eq!(normalized, 0);
}

#[test]
fn test_custom_config() {
    // Test creating normalizer with custom config
    let custom_config = RateConfig {
        scale_factor: ScaleFactor::new(1_000_000), // 1e6
        shift: Shift::new(6),
        multiplier: 100,
    };
    let normalizer = RateNormalizer::from(custom_config);

    // Test with a known value
    let rate = Rate::from_percent(5);
    let normalized = normalizer.normalize_rate(rate).unwrap();
    assert_eq!(normalized, 500); // 5% * 100 = 500
}

#[test]
fn test_overflow_conditions() {
    let normalizer = RateNormalizer::save();

    // Test with maximum u128 value
    let max_u128 = u128::MAX;
    let result = normalizer.normalize_rate(max_u128);
    assert!(result.is_err());

    // Test with very large rate
    let large_rate = Rate::from_percent(255); // Maximum u8 value
    let result = normalizer.normalize_rate(large_rate);
    assert!(result.is_ok());
}

#[test]
fn test_protocol_consistency() {
    // Test that the same value normalized through different protocols gives expected results
    let value = 1_000_000u128; // 1M tokens

    let save_norm = PoolLiquidityNormalizer::save().normalize_amount(value).unwrap();
    let marginfi_norm = PoolLiquidityNormalizer::marginfi().normalize_amount(value).unwrap();
    let kamino_norm = PoolLiquidityNormalizer::kamino().normalize_amount(value).unwrap();
    let drift_norm = PoolLiquidityNormalizer::drift().normalize_amount(value).unwrap();

    // Verify relative scaling between protocols is correct
    assert_eq!(save_norm, value); // Save doesn't scale
    assert_eq!(marginfi_norm, wad(value)); // 1e18 scaling
    assert_eq!(kamino_norm, wad(value)); // 1e18 scaling
    assert_eq!(drift_norm, value * 1_000_000_000_000_000); // 1e15 scaling
}
