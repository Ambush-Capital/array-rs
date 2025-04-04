//! State types

mod last_update;
mod lending_market;
mod lending_market_metadata;
mod obligation;
mod rate_limiter;
mod reserve;

pub use last_update::*;
pub use lending_market::*;
pub use lending_market_metadata::*;
pub use obligation::*;
pub use rate_limiter::*;
pub use reserve::*;

use crate::save::math::{Decimal, WAD};
use solana_program::{msg, program_error::ProgramError};

/// Collateral tokens are initially valued at a ratio of 5:1 (collateral:liquidity)
// @FIXME: restore to 5
pub const INITIAL_COLLATERAL_RATIO: u64 = 1;
const INITIAL_COLLATERAL_RATE: u64 = INITIAL_COLLATERAL_RATIO * WAD;

/// Current version of the program and all new accounts created
pub const PROGRAM_VERSION: u8 = 1;

/// Accounts are created with data zeroed out, so uninitialized state instances
/// will have the version set to 0.
pub const UNINITIALIZED_VERSION: u8 = 0;

/// Number of slots per year
// 2 (slots per second) * 60 * 60 * 24 * 365 = 63072000
pub const SLOTS_PER_YEAR: u64 = 63072000;

// Helpers
fn pack_decimal(decimal: Decimal, dst: &mut [u8; 16]) {
    *dst = decimal
        .to_scaled_val()
        .expect("Decimal cannot be packed")
        .to_le_bytes();
}

fn unpack_decimal(src: &[u8; 16]) -> Decimal {
    Decimal::from_scaled_val(u128::from_le_bytes(*src))
}

fn pack_bool(boolean: bool, dst: &mut [u8; 1]) {
    *dst = (boolean as u8).to_le_bytes()
}

fn unpack_bool(src: &[u8; 1]) -> Result<bool, ProgramError> {
    match u8::from_le_bytes(*src) {
        0 => Ok(false),
        1 => Ok(true),
        _ => {
            msg!("Boolean cannot be unpacked");
            Err(ProgramError::InvalidAccountData)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn initial_collateral_rate_sanity() {
        assert_eq!(
            INITIAL_COLLATERAL_RATIO.checked_mul(WAD).unwrap(),
            INITIAL_COLLATERAL_RATE
        );
    }
}
