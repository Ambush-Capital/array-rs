use std::ops::{Div, Mul};

/// A type-safe scale factor to prevent mixing different kinds of scaling operations.
/// These are typically used to normalize values between different protocols that may use
/// different decimal places or scaling factors.
///
/// # Examples
/// ```
/// let wad = ScaleFactor::WAD; // 1e18
/// let drift = ScaleFactor::DRIFT; // 1e15
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScaleFactor(pub u128);

impl ScaleFactor {
    /// Creates a new ScaleFactor with the given value
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    /// Returns the underlying u128 value
    pub const fn as_u128(&self) -> u128 {
        self.0
    }

    /// Safely multiply a value by this scale factor, handling potential overflow
    pub fn safe_mul(&self, value: u128) -> Option<u128> {
        // If either value is 0, return 0 to avoid unnecessary computation
        if self.0 == 0 || value == 0 {
            return Some(0);
        }

        // Check if the multiplication would overflow
        // First check if value > u128::MAX / scale_factor
        if value > u128::MAX / self.0 {
            None
        } else {
            Some(value * self.0)
        }
    }

    /// Safely divide a value by this scale factor, handling potential overflow and division by zero
    pub fn safe_div(&self, value: u128) -> Option<u128> {
        if self.0 == 0 {
            None
        } else {
            Some(value / self.0)
        }
    }
}

impl Mul<u128> for ScaleFactor {
    type Output = u128;

    fn mul(self, rhs: u128) -> u128 {
        self.safe_mul(rhs).expect("ScaleFactor multiplication overflow")
    }
}

impl Div<ScaleFactor> for u128 {
    type Output = u128;

    fn div(self, rhs: ScaleFactor) -> u128 {
        rhs.safe_div(self).expect("ScaleFactor division overflow or division by zero")
    }
}
