use solana_program::program_pack::IsInitialized;
use solana_program::{program_error::ProgramError, slot_history::Slot};

use crate::save::math::Decimal;
use arrayref::{array_mut_ref, array_ref, array_refs, mut_array_refs};
use solana_program::program_pack::{Pack, Sealed};

use super::{pack_decimal, unpack_decimal};

/// Sliding Window Rate limiter
/// guarantee: at any point, the outflow between [cur_slot - slot.window_duration, cur_slot]
/// is less than 2x max_outflow.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RateLimiter {
    /// configuration parameters
    pub config: RateLimiterConfig,

    // state
    /// prev qty is the sum of all outflows from [window_start - config.window_duration, window_start)
    prev_qty: Decimal,
    /// window_start is the start of the current window
    window_start: Slot,
    /// cur qty is the sum of all outflows from [window_start, window_start + config.window_duration)
    cur_qty: Decimal,
}

/// Lending market configuration parameters
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RateLimiterConfig {
    /// Rate limiter window size in slots
    pub window_duration: u64,
    /// Rate limiter param. Max outflow of tokens in a window
    pub max_outflow: u64,
}

impl RateLimiter {
    /// initialize rate limiter
    pub fn new(config: RateLimiterConfig, cur_slot: u64) -> Self {
        let slot_start = if config.window_duration != 0 {
            cur_slot / config.window_duration * config.window_duration
        } else {
            cur_slot
        };

        Self {
            config,
            prev_qty: Decimal::zero(),
            window_start: slot_start,
            cur_qty: Decimal::zero(),
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(RateLimiterConfig { window_duration: 1, max_outflow: u64::MAX }, 1)
    }
}

impl Sealed for RateLimiter {}

impl IsInitialized for RateLimiter {
    fn is_initialized(&self) -> bool {
        true
    }
}

/// Size of RateLimiter when packed into account
pub const RATE_LIMITER_LEN: usize = 56;
impl Pack for RateLimiter {
    const LEN: usize = RATE_LIMITER_LEN;

    fn pack_into_slice(&self, dst: &mut [u8]) {
        let dst = array_mut_ref![dst, 0, RATE_LIMITER_LEN];
        let (
            config_max_outflow_dst,
            config_window_duration_dst,
            prev_qty_dst,
            window_start_dst,
            cur_qty_dst,
        ) = mut_array_refs![dst, 8, 8, 16, 8, 16];
        *config_max_outflow_dst = self.config.max_outflow.to_le_bytes();
        *config_window_duration_dst = self.config.window_duration.to_le_bytes();
        pack_decimal(self.prev_qty, prev_qty_dst);
        *window_start_dst = self.window_start.to_le_bytes();
        pack_decimal(self.cur_qty, cur_qty_dst);
    }

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let src = array_ref![src, 0, RATE_LIMITER_LEN];
        let (
            config_max_outflow_src,
            config_window_duration_src,
            prev_qty_src,
            window_start_src,
            cur_qty_src,
        ) = array_refs![src, 8, 8, 16, 8, 16];

        Ok(Self {
            config: RateLimiterConfig {
                max_outflow: u64::from_le_bytes(*config_max_outflow_src),
                window_duration: u64::from_le_bytes(*config_window_duration_src),
            },
            prev_qty: unpack_decimal(prev_qty_src),
            window_start: u64::from_le_bytes(*window_start_src),
            cur_qty: unpack_decimal(cur_qty_src),
        })
    }
}

#[cfg(test)]
pub fn rand_rate_limiter() -> RateLimiter {
    use rand::Rng;
    let mut rng = rand::thread_rng();

    fn rand_decimal() -> Decimal {
        Decimal::from_scaled_val(rand::thread_rng().gen())
    }

    RateLimiter {
        config: RateLimiterConfig { window_duration: rng.gen(), max_outflow: rng.gen() },
        prev_qty: rand_decimal(),
        window_start: rng.gen(),
        cur_qty: rand_decimal(),
    }
}
