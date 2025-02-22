/// A type-safe bit shift amount for scaling operations.
/// This helps prevent confusion between different shift amounts used in different contexts.
///
/// # Examples
/// ```
/// let save_shift = Shift::SAVE; // 60 bits
/// let marginfi_shift = Shift::MARGINFI; // 12 bits
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shift(pub u32);

impl Shift {
    /// Creates a new Shift with the given value
    ///
    /// # Arguments
    /// * `value` - The number of bits to shift
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the underlying u32 value
    pub const fn as_u32(&self) -> u32 {
        self.0
    }

    /// Returns a zero shift
    pub const fn zero() -> Self {
        Self(0)
    }
}
