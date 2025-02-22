use super::{scale_factor::ScaleFactor, shift::Shift};

/// Configuration for rate normalization operations
#[derive(Debug, Clone, Copy)]
pub struct RateConfig {
    pub scale_factor: ScaleFactor,
    pub shift: Shift,
    pub multiplier: u128,
}

impl RateConfig {
    // Protocol-specific scale factors
    pub const SUPPLY_SCALE: ScaleFactor = ScaleFactor::new(1_000_000_000_000_000_000); // 1e18
    pub const MARGINFI_SCALE: ScaleFactor = ScaleFactor::new(1_000_000_000_000_000_000); // 1e18
    pub const DRIFT_SCALE: ScaleFactor = ScaleFactor::new(1_000_000_000_000_000); // 1e15
    pub const WAD: ScaleFactor = ScaleFactor::new(1_000_000_000_000_000_000); // 1e18

    // Protocol-specific shifts
    pub const SAVE_SHIFT: Shift = Shift::new(60);
    pub const MARGINFI_SHIFT: Shift = Shift::new(12);

    /// Configuration for Save protocol
    pub const SAVE: Self =
        Self { scale_factor: Self::WAD, shift: Self::SAVE_SHIFT, multiplier: 1000 };

    /// Configuration for MarginFi protocol
    pub const MARGINFI: Self =
        Self { scale_factor: ScaleFactor::new(1), shift: Self::MARGINFI_SHIFT, multiplier: 1000 };

    /// Configuration for Kamino protocol
    pub const KAMINO: Self =
        Self { scale_factor: ScaleFactor::new(1), shift: Shift::zero(), multiplier: 1000 };

    /// Configuration for Drift protocol
    pub const DRIFT: Self =
        Self { scale_factor: Self::DRIFT_SCALE, shift: Shift::zero(), multiplier: 1 };
}

/// Configuration for pool liquidity normalization operations
#[derive(Debug, Clone, Copy)]
pub struct PoolLiquidityConfig {
    pub scale_factor: ScaleFactor,
}

impl PoolLiquidityConfig {
    // Protocol-specific scale factors
    pub const SUPPLY_SCALE: ScaleFactor = ScaleFactor::new(1_000_000_000_000_000_000); // 1e18
    pub const KAMINO_SCALE: ScaleFactor = ScaleFactor::new(1_000_000_000_000_000_000); // 1e18
    pub const MARGINFI_SCALE: ScaleFactor = ScaleFactor::new(1_000_000_000_000_000_000); // 1e18
    pub const DRIFT_SCALE: ScaleFactor = ScaleFactor::new(1_000_000_000_000_000_000); // 1e15

    /// Configuration for Save protocol - no scaling needed
    pub const SAVE: Self = Self { scale_factor: ScaleFactor::new(1) };

    /// Configuration for MarginFi protocol
    pub const MARGINFI: Self = Self { scale_factor: Self::MARGINFI_SCALE };

    /// Configuration for Kamino protocol
    pub const KAMINO: Self = Self { scale_factor: Self::KAMINO_SCALE };

    /// Configuration for Drift protocol
    pub const DRIFT: Self = Self { scale_factor: Self::DRIFT_SCALE };
}
