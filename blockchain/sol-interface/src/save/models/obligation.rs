use crate::save::error::LendingError;
use crate::save::math::{Decimal, Rate, TryDiv, TryMul, TrySub};
use arrayref::{array_mut_ref, array_ref, array_refs, mut_array_refs};
use solana_program::{
    clock::Slot,
    msg,
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack, Sealed},
    pubkey::{Pubkey, PUBKEY_BYTES},
};
use std::{cmp::min, convert::TryFrom};

use super::{
    pack_bool, pack_decimal, unpack_bool, unpack_decimal, LastUpdate, Reserve,
    LIQUIDATION_CLOSE_FACTOR, MAX_LIQUIDATABLE_VALUE_AT_ONCE, PROGRAM_VERSION,
    UNINITIALIZED_VERSION,
};

/// Max number of collateral and liquidity reserve accounts combined for an obligation
pub const MAX_OBLIGATION_RESERVES: usize = 10;

/// Lending market obligation state
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Obligation {
    /// Version of the struct
    pub version: u8,
    /// Last update to collateral, liquidity, or their market values
    pub last_update: LastUpdate,
    /// Lending market address
    pub lending_market: Pubkey,
    /// Owner authority which can borrow liquidity
    pub owner: Pubkey,
    /// Deposited collateral for the obligation, unique by deposit reserve address
    pub deposits: Vec<ObligationCollateral>,
    /// Borrowed liquidity for the obligation, unique by borrow reserve address
    pub borrows: Vec<ObligationLiquidity>,
    /// Market value of deposits
    pub deposited_value: Decimal,
    /// Risk-adjusted market value of borrows.
    /// ie sum(b.borrowed_amount * b.current_spot_price * b.borrow_weight for b in borrows)
    pub borrowed_value: Decimal,
    /// True borrow value. Ie, not risk adjusted like "borrowed_value"
    pub unweighted_borrowed_value: Decimal,
    /// Risk-adjusted upper bound market value of borrows.
    /// ie sum(b.borrowed_amount * max(b.current_spot_price, b.smoothed_price) * b.borrow_weight for b in borrows)
    pub borrowed_value_upper_bound: Decimal,
    /// The maximum open borrow value.
    /// ie sum(d.deposited_amount * d.ltv * min(d.current_spot_price, d.smoothed_price) for d in deposits)
    /// if borrowed_value_upper_bound >= allowed_borrow_value, then the obligation is unhealthy and
    /// borrows and withdraws are disabled.
    pub allowed_borrow_value: Decimal,
    /// The dangerous borrow value at the weighted average liquidation threshold.
    /// ie sum(d.deposited_amount * d.liquidation_threshold * d.current_spot_price for d in deposits)
    /// if borrowed_value >= unhealthy_borrow_value, the obligation can be liquidated
    pub unhealthy_borrow_value: Decimal,
    /// ie sum(d.deposited_amount * d.max_liquidation_threshold * d.current_spot_price for d in
    /// deposits). This field is used to calculate the liquidator bonus.
    /// An obligation with a borrowed value >= super_unhealthy_borrow_value is eligible for the max
    /// bonus
    pub super_unhealthy_borrow_value: Decimal,
    /// True if the obligation is currently borrowing an isolated tier asset
    pub borrowing_isolated_asset: bool,
    /// Obligation can be marked as closeable
    pub closeable: bool,
}

impl Obligation {
    /// Create a new obligation
    pub fn new(params: InitObligationParams) -> Self {
        let mut obligation = Self::default();
        Self::init(&mut obligation, params);
        obligation
    }

    /// Initialize an obligation
    pub fn init(&mut self, params: InitObligationParams) {
        self.version = PROGRAM_VERSION;
        self.last_update = LastUpdate::new(params.current_slot);
        self.lending_market = params.lending_market;
        self.owner = params.owner;
        self.deposits = params.deposits;
        self.borrows = params.borrows;
    }

    /// Calculate the current ratio of borrowed value to deposited value
    pub fn loan_to_value(&self) -> Result<Decimal, ProgramError> {
        self.borrowed_value.try_div(self.deposited_value)
    }

    /// calculate the maximum amount of collateral that can be borrowed
    pub fn max_withdraw_amount(
        &self,
        collateral: &ObligationCollateral,
        withdraw_reserve: &Reserve,
    ) -> Result<u64, ProgramError> {
        if self.borrows.is_empty() {
            return Ok(collateral.deposited_amount);
        }

        if self.allowed_borrow_value <= self.borrowed_value_upper_bound {
            return Ok(0);
        }

        let loan_to_value_ratio = withdraw_reserve.loan_to_value_ratio();
        if loan_to_value_ratio == Rate::zero() {
            return Ok(collateral.deposited_amount);
        }

        // max usd value that can be withdrawn
        let max_withdraw_value = self
            .allowed_borrow_value
            .try_sub(self.borrowed_value_upper_bound)?
            .try_div(loan_to_value_ratio)?;

        // convert max_withdraw_value to max withdraw liquidity amount

        // why is lower bound used and not upper bound? seems scary
        //
        // the tldr is that allowed borrow value is calculated with the minimum
        // of the spot price and the smoothed price, so we have to use the min here to be
        // consistent.
        //
        // note that safety-wise, it doesn't actually matter. if we used the max (which appears safer),
        // the initial max withdraw would be lower, but the user can immediately make another max withdraw call
        // because allowed_borrow_value is still greater than borrowed_value_upper_bound
        // after a large amount of consecutive max withdraw calls, the end state of using max would be the same
        // as using min.
        //
        // therefore, we use min for the better UX.
        let price = withdraw_reserve.price_lower_bound();

        let decimals = 10u64
            .checked_pow(withdraw_reserve.liquidity.mint_decimals as u32)
            .ok_or(LendingError::MathOverflow)?;

        let max_withdraw_liquidity_amount = max_withdraw_value.try_mul(decimals)?.try_div(price)?;

        // convert max withdraw liquidity amount to max withdraw collateral amount
        Ok(min(
            withdraw_reserve
                .collateral_exchange_rate()?
                .decimal_liquidity_to_collateral(max_withdraw_liquidity_amount)?
                .try_floor_u64()?,
            collateral.deposited_amount,
        ))
    }

    /// Calculate the maximum liquidity value that can be borrowed
    pub fn remaining_borrow_value(&self) -> Result<Decimal, ProgramError> {
        self.allowed_borrow_value.try_sub(self.borrowed_value_upper_bound)
    }

    /// Calculate the maximum liquidation amount for a given liquidity
    pub fn max_liquidation_amount(
        &self,
        liquidity: &ObligationLiquidity,
    ) -> Result<Decimal, ProgramError> {
        let max_liquidation_value = self
            .borrowed_value
            .try_mul(Rate::from_percent(LIQUIDATION_CLOSE_FACTOR))?
            .min(liquidity.market_value)
            .min(Decimal::from(MAX_LIQUIDATABLE_VALUE_AT_ONCE));

        let max_liquidation_pct = max_liquidation_value.try_div(liquidity.market_value)?;
        liquidity.borrowed_amount_wads.try_mul(max_liquidation_pct)
    }

    /// Find collateral by deposit reserve
    pub fn find_collateral_in_deposits(
        &self,
        deposit_reserve: Pubkey,
    ) -> Result<(&ObligationCollateral, usize), ProgramError> {
        if self.deposits.is_empty() {
            msg!("Obligation has no deposits");
            return Err(LendingError::ObligationDepositsEmpty.into());
        }
        let collateral_index = self
            ._find_collateral_index_in_deposits(deposit_reserve)
            .ok_or(LendingError::InvalidObligationCollateral)?;
        Ok((&self.deposits[collateral_index], collateral_index))
    }

    fn _find_collateral_index_in_deposits(&self, deposit_reserve: Pubkey) -> Option<usize> {
        self.deposits.iter().position(|collateral| collateral.deposit_reserve == deposit_reserve)
    }

    /// Find liquidity by borrow reserve
    pub fn find_liquidity_in_borrows(
        &self,
        borrow_reserve: Pubkey,
    ) -> Result<(&ObligationLiquidity, usize), ProgramError> {
        if self.borrows.is_empty() {
            msg!("Obligation has no borrows");
            return Err(LendingError::ObligationBorrowsEmpty.into());
        }
        let liquidity_index = self
            ._find_liquidity_index_in_borrows(borrow_reserve)
            .ok_or(LendingError::InvalidObligationLiquidity)?;
        Ok((&self.borrows[liquidity_index], liquidity_index))
    }

    /// Find liquidity by borrow reserve mut
    pub fn find_liquidity_in_borrows_mut(
        &mut self,
        borrow_reserve: Pubkey,
    ) -> Result<(&mut ObligationLiquidity, usize), ProgramError> {
        if self.borrows.is_empty() {
            msg!("Obligation has no borrows");
            return Err(LendingError::ObligationBorrowsEmpty.into());
        }
        let liquidity_index = self
            ._find_liquidity_index_in_borrows(borrow_reserve)
            .ok_or(LendingError::InvalidObligationLiquidity)?;
        Ok((&mut self.borrows[liquidity_index], liquidity_index))
    }

    fn _find_liquidity_index_in_borrows(&self, borrow_reserve: Pubkey) -> Option<usize> {
        self.borrows.iter().position(|liquidity| liquidity.borrow_reserve == borrow_reserve)
    }
}

/// Initialize an obligation
pub struct InitObligationParams {
    /// Last update to collateral, liquidity, or their market values
    pub current_slot: Slot,
    /// Lending market address
    pub lending_market: Pubkey,
    /// Owner authority which can borrow liquidity
    pub owner: Pubkey,
    /// Deposited collateral for the obligation, unique by deposit reserve address
    pub deposits: Vec<ObligationCollateral>,
    /// Borrowed liquidity for the obligation, unique by borrow reserve address
    pub borrows: Vec<ObligationLiquidity>,
}

impl Sealed for Obligation {}
impl IsInitialized for Obligation {
    fn is_initialized(&self) -> bool {
        self.version != UNINITIALIZED_VERSION
    }
}

/// Obligation collateral state
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObligationCollateral {
    /// Reserve collateral is deposited to
    pub deposit_reserve: Pubkey,
    /// Amount of collateral deposited
    pub deposited_amount: u64,
    /// Collateral market value in quote currency
    pub market_value: Decimal,
    /// How much borrow is attributed to this collateral (USD)
    pub attributed_borrow_value: Decimal,
}

impl ObligationCollateral {
    /// Create new obligation collateral
    pub fn new(deposit_reserve: Pubkey) -> Self {
        Self {
            deposit_reserve,
            deposited_amount: 0,
            market_value: Decimal::zero(),
            attributed_borrow_value: Decimal::zero(),
        }
    }
}

/// Obligation liquidity state
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObligationLiquidity {
    /// Reserve liquidity is borrowed from
    pub borrow_reserve: Pubkey,
    /// Borrow rate used for calculating interest
    pub cumulative_borrow_rate_wads: Decimal,
    /// Amount of liquidity borrowed plus interest
    pub borrowed_amount_wads: Decimal,
    /// Liquidity market value in quote currency
    pub market_value: Decimal,
}

impl ObligationLiquidity {
    /// Create new obligation liquidity
    pub fn new(borrow_reserve: Pubkey, cumulative_borrow_rate_wads: Decimal) -> Self {
        Self {
            borrow_reserve,
            cumulative_borrow_rate_wads,
            borrowed_amount_wads: Decimal::zero(),
            market_value: Decimal::zero(),
        }
    }
}

const OBLIGATION_COLLATERAL_LEN: usize = 88; // 32 + 8 + 16 + 32
const OBLIGATION_LIQUIDITY_LEN: usize = 112; // 32 + 16 + 16 + 16 + 32
const OBLIGATION_LEN: usize = 1300; // 1 + 8 + 1 + 32 + 32 + 16 + 16 + 16 + 16 + 64 + 1 + 1 + (88 * 1) + (112 * 9)
                                    // @TODO: break this up by obligation / collateral / liquidity https://git.io/JOCca
impl Pack for Obligation {
    const LEN: usize = OBLIGATION_LEN;

    fn pack_into_slice(&self, dst: &mut [u8]) {
        let output = array_mut_ref![dst, 0, OBLIGATION_LEN];
        #[allow(clippy::ptr_offset_with_cast)]
        let (
            version,
            last_update_slot,
            last_update_stale,
            lending_market,
            owner,
            deposited_value,
            borrowed_value,
            allowed_borrow_value,
            unhealthy_borrow_value,
            borrowed_value_upper_bound,
            borrowing_isolated_asset,
            super_unhealthy_borrow_value,
            unweighted_borrowed_value,
            closeable,
            _padding,
            deposits_len,
            borrows_len,
            data_flat,
        ) = mut_array_refs![
            output,
            1,
            8,
            1,
            PUBKEY_BYTES,
            PUBKEY_BYTES,
            16,
            16,
            16,
            16,
            16,
            1,
            16,
            16,
            1,
            14,
            1,
            1,
            OBLIGATION_COLLATERAL_LEN + (OBLIGATION_LIQUIDITY_LEN * (MAX_OBLIGATION_RESERVES - 1))
        ];

        // obligation
        *version = self.version.to_le_bytes();
        *last_update_slot = self.last_update.slot.to_le_bytes();
        pack_bool(self.last_update.stale, last_update_stale);
        lending_market.copy_from_slice(self.lending_market.as_ref());
        owner.copy_from_slice(self.owner.as_ref());
        pack_decimal(self.deposited_value, deposited_value);
        pack_decimal(self.borrowed_value, borrowed_value);
        pack_decimal(self.borrowed_value_upper_bound, borrowed_value_upper_bound);
        pack_decimal(self.allowed_borrow_value, allowed_borrow_value);
        pack_decimal(self.unhealthy_borrow_value, unhealthy_borrow_value);
        pack_bool(self.borrowing_isolated_asset, borrowing_isolated_asset);
        pack_decimal(self.super_unhealthy_borrow_value, super_unhealthy_borrow_value);
        pack_decimal(self.unweighted_borrowed_value, unweighted_borrowed_value);
        pack_bool(self.closeable, closeable);

        *deposits_len = u8::try_from(self.deposits.len()).unwrap().to_le_bytes();
        *borrows_len = u8::try_from(self.borrows.len()).unwrap().to_le_bytes();

        let mut offset = 0;

        // deposits
        for collateral in &self.deposits {
            let deposits_flat = array_mut_ref![data_flat, offset, OBLIGATION_COLLATERAL_LEN];
            #[allow(clippy::ptr_offset_with_cast)]
            let (
                deposit_reserve,
                deposited_amount,
                market_value,
                attributed_borrow_value,
                _padding_deposit,
            ) = mut_array_refs![deposits_flat, PUBKEY_BYTES, 8, 16, 16, 16];
            deposit_reserve.copy_from_slice(collateral.deposit_reserve.as_ref());
            *deposited_amount = collateral.deposited_amount.to_le_bytes();
            pack_decimal(collateral.market_value, market_value);
            pack_decimal(collateral.attributed_borrow_value, attributed_borrow_value);
            offset += OBLIGATION_COLLATERAL_LEN;
        }

        // borrows
        for liquidity in &self.borrows {
            let borrows_flat = array_mut_ref![data_flat, offset, OBLIGATION_LIQUIDITY_LEN];
            #[allow(clippy::ptr_offset_with_cast)]
            let (
                borrow_reserve,
                cumulative_borrow_rate_wads,
                borrowed_amount_wads,
                market_value,
                _padding_borrow,
            ) = mut_array_refs![borrows_flat, PUBKEY_BYTES, 16, 16, 16, 32];
            borrow_reserve.copy_from_slice(liquidity.borrow_reserve.as_ref());
            pack_decimal(liquidity.cumulative_borrow_rate_wads, cumulative_borrow_rate_wads);
            pack_decimal(liquidity.borrowed_amount_wads, borrowed_amount_wads);
            pack_decimal(liquidity.market_value, market_value);
            offset += OBLIGATION_LIQUIDITY_LEN;
        }
    }

    /// Unpacks a byte buffer into an [ObligationInfo](struct.ObligationInfo.html).
    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let input = array_ref![src, 0, OBLIGATION_LEN];
        #[allow(clippy::ptr_offset_with_cast)]
        let (
            version,
            last_update_slot,
            last_update_stale,
            lending_market,
            owner,
            deposited_value,
            borrowed_value,
            allowed_borrow_value,
            unhealthy_borrow_value,
            borrowed_value_upper_bound,
            borrowing_isolated_asset,
            super_unhealthy_borrow_value,
            unweighted_borrowed_value,
            closeable,
            _padding,
            deposits_len,
            borrows_len,
            data_flat,
        ) = array_refs![
            input,
            1,
            8,
            1,
            PUBKEY_BYTES,
            PUBKEY_BYTES,
            16,
            16,
            16,
            16,
            16,
            1,
            16,
            16,
            1,
            14,
            1,
            1,
            OBLIGATION_COLLATERAL_LEN + (OBLIGATION_LIQUIDITY_LEN * (MAX_OBLIGATION_RESERVES - 1))
        ];

        let version = u8::from_le_bytes(*version);
        if version > PROGRAM_VERSION {
            msg!("Obligation version does not match lending program version");
            return Err(ProgramError::InvalidAccountData);
        }

        let deposits_len = u8::from_le_bytes(*deposits_len);
        let borrows_len = u8::from_le_bytes(*borrows_len);
        let mut deposits = Vec::with_capacity(deposits_len as usize + 1);
        let mut borrows = Vec::with_capacity(borrows_len as usize + 1);

        let mut offset = 0;
        for _ in 0..deposits_len {
            let deposits_flat = array_ref![data_flat, offset, OBLIGATION_COLLATERAL_LEN];
            #[allow(clippy::ptr_offset_with_cast)]
            let (
                deposit_reserve,
                deposited_amount,
                market_value,
                attributed_borrow_value,
                _padding_deposit,
            ) = array_refs![deposits_flat, PUBKEY_BYTES, 8, 16, 16, 16];
            deposits.push(ObligationCollateral {
                deposit_reserve: Pubkey::from(*deposit_reserve),
                deposited_amount: u64::from_le_bytes(*deposited_amount),
                market_value: unpack_decimal(market_value),
                attributed_borrow_value: unpack_decimal(attributed_borrow_value),
            });
            offset += OBLIGATION_COLLATERAL_LEN;
        }
        for _ in 0..borrows_len {
            let borrows_flat = array_ref![data_flat, offset, OBLIGATION_LIQUIDITY_LEN];
            #[allow(clippy::ptr_offset_with_cast)]
            let (
                borrow_reserve,
                cumulative_borrow_rate_wads,
                borrowed_amount_wads,
                market_value,
                _padding_borrow,
            ) = array_refs![borrows_flat, PUBKEY_BYTES, 16, 16, 16, 32];
            borrows.push(ObligationLiquidity {
                borrow_reserve: Pubkey::from(*borrow_reserve),
                cumulative_borrow_rate_wads: unpack_decimal(cumulative_borrow_rate_wads),
                borrowed_amount_wads: unpack_decimal(borrowed_amount_wads),
                market_value: unpack_decimal(market_value),
            });
            offset += OBLIGATION_LIQUIDITY_LEN;
        }

        Ok(Self {
            version,
            last_update: LastUpdate {
                slot: u64::from_le_bytes(*last_update_slot),
                stale: unpack_bool(last_update_stale)?,
            },
            lending_market: Pubkey::new_from_array(*lending_market),
            owner: Pubkey::new_from_array(*owner),
            deposits,
            borrows,
            deposited_value: unpack_decimal(deposited_value),
            borrowed_value: unpack_decimal(borrowed_value),
            unweighted_borrowed_value: unpack_decimal(unweighted_borrowed_value),
            borrowed_value_upper_bound: unpack_decimal(borrowed_value_upper_bound),
            allowed_borrow_value: unpack_decimal(allowed_borrow_value),
            unhealthy_borrow_value: unpack_decimal(unhealthy_borrow_value),
            super_unhealthy_borrow_value: unpack_decimal(super_unhealthy_borrow_value),
            borrowing_isolated_asset: unpack_bool(borrowing_isolated_asset)?,
            closeable: unpack_bool(closeable)?,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::save::models::{ReserveCollateral, ReserveConfig, ReserveLiquidity};
    use proptest::prelude::*;
    use rand::Rng;
    use solana_program::native_token::LAMPORTS_PER_SOL;

    fn rand_decimal() -> Decimal {
        Decimal::from_scaled_val(rand::thread_rng().gen())
    }

    #[test]
    fn pack_and_unpack_obligation() {
        let mut rng = rand::thread_rng();
        for _ in 0..100 {
            let obligation = Obligation {
                version: PROGRAM_VERSION,
                last_update: LastUpdate { slot: rng.gen(), stale: rng.gen() },
                lending_market: Pubkey::new_unique(),
                owner: Pubkey::new_unique(),
                deposits: vec![ObligationCollateral {
                    deposit_reserve: Pubkey::new_unique(),
                    deposited_amount: rng.gen(),
                    market_value: rand_decimal(),
                    attributed_borrow_value: rand_decimal(),
                }],
                borrows: vec![ObligationLiquidity {
                    borrow_reserve: Pubkey::new_unique(),
                    cumulative_borrow_rate_wads: rand_decimal(),
                    borrowed_amount_wads: rand_decimal(),
                    market_value: rand_decimal(),
                }],
                deposited_value: rand_decimal(),
                borrowed_value: rand_decimal(),
                unweighted_borrowed_value: rand_decimal(),
                borrowed_value_upper_bound: rand_decimal(),
                allowed_borrow_value: rand_decimal(),
                unhealthy_borrow_value: rand_decimal(),
                super_unhealthy_borrow_value: rand_decimal(),
                borrowing_isolated_asset: rng.gen(),
                closeable: rng.gen(),
            };

            let mut packed = [0u8; OBLIGATION_LEN];
            Obligation::pack(obligation.clone(), &mut packed).unwrap();
            let unpacked = Obligation::unpack(&packed).unwrap();
            assert_eq!(obligation, unpacked);
        }
    }

    #[test]
    fn max_liquidation_amount_normal() {
        let obligation_liquidity = ObligationLiquidity {
            borrowed_amount_wads: Decimal::from(50u64),
            market_value: Decimal::from(100u64),
            ..ObligationLiquidity::default()
        };

        let obligation = Obligation {
            deposited_value: Decimal::from(100u64),
            borrowed_value: Decimal::from(100u64),
            borrows: vec![obligation_liquidity.clone()],
            ..Obligation::default()
        };

        let expected_collateral = Decimal::from(50u64)
            .try_mul(Decimal::from(LIQUIDATION_CLOSE_FACTOR as u64))
            .unwrap()
            .try_div(100)
            .unwrap();

        assert_eq!(
            obligation.max_liquidation_amount(&obligation_liquidity).unwrap(),
            expected_collateral
        );
    }

    #[test]
    fn max_liquidation_amount_low_liquidity() {
        let obligation_liquidity = ObligationLiquidity {
            borrowed_amount_wads: Decimal::from(100u64),
            market_value: Decimal::from(1u64),
            ..ObligationLiquidity::default()
        };

        let obligation = Obligation {
            deposited_value: Decimal::from(100u64),
            borrowed_value: Decimal::from(100u64),
            borrows: vec![obligation_liquidity.clone()],
            ..Obligation::default()
        };

        assert_eq!(
            obligation.max_liquidation_amount(&obligation_liquidity).unwrap(),
            Decimal::from(100u64)
        );
    }

    #[test]
    fn max_liquidation_amount_big_whale() {
        let obligation_liquidity = ObligationLiquidity {
            borrowed_amount_wads: Decimal::from(1_000_000_000u64),
            market_value: Decimal::from(1_000_000_000u64),
            ..ObligationLiquidity::default()
        };

        let obligation = Obligation {
            deposited_value: Decimal::from(1_000_000_000u64),
            borrowed_value: Decimal::from(1_000_000_000u64),
            borrows: vec![obligation_liquidity.clone()],
            ..Obligation::default()
        };

        assert_eq!(
            obligation.max_liquidation_amount(&obligation_liquidity).unwrap(),
            Decimal::from(MAX_LIQUIDATABLE_VALUE_AT_ONCE)
        );
    }

    #[derive(Debug, Clone)]
    struct MaxWithdrawAmountTestCase {
        obligation: Obligation,
        reserve: Reserve,

        expected_max_withdraw_amount: u64,
    }

    fn max_withdraw_amount_test_cases() -> impl Strategy<Value = MaxWithdrawAmountTestCase> {
        prop_oneof![
            // borrowed as much as we can already, so can't borrow anything more
            Just(MaxWithdrawAmountTestCase {
                obligation: Obligation {
                    deposits: vec![ObligationCollateral {
                        deposited_amount: 20 * LAMPORTS_PER_SOL,
                        ..ObligationCollateral::default()
                    }],
                    borrows: vec![ObligationLiquidity {
                        borrowed_amount_wads: Decimal::from(10u64),
                        ..ObligationLiquidity::default()
                    }],
                    deposited_value: Decimal::from(100u64),
                    borrowed_value_upper_bound: Decimal::from(50u64),
                    allowed_borrow_value: Decimal::from(50u64),
                    ..Obligation::default()
                },
                reserve: Reserve {
                    config: ReserveConfig { loan_to_value_ratio: 50, ..ReserveConfig::default() },
                    ..Reserve::default()
                },
                expected_max_withdraw_amount: 0,
            }),
            // regular case
            Just(MaxWithdrawAmountTestCase {
                obligation: Obligation {
                    deposits: vec![ObligationCollateral {
                        deposited_amount: 20 * LAMPORTS_PER_SOL,
                        ..ObligationCollateral::default()
                    }],
                    borrows: vec![ObligationLiquidity {
                        borrowed_amount_wads: Decimal::from(10u64),
                        ..ObligationLiquidity::default()
                    }],

                    allowed_borrow_value: Decimal::from(100u64),
                    borrowed_value_upper_bound: Decimal::from(50u64),
                    ..Obligation::default()
                },

                reserve: Reserve {
                    config: ReserveConfig { loan_to_value_ratio: 50, ..ReserveConfig::default() },
                    liquidity: ReserveLiquidity {
                        available_amount: 100 * LAMPORTS_PER_SOL,
                        borrowed_amount_wads: Decimal::zero(),
                        market_price: Decimal::from(10u64),
                        smoothed_market_price: Decimal::from(5u64),
                        mint_decimals: 9,
                        ..ReserveLiquidity::default()
                    },
                    collateral: ReserveCollateral {
                        mint_total_supply: 50 * LAMPORTS_PER_SOL,
                        ..ReserveCollateral::default()
                    },
                    ..Reserve::default()
                },

                // deposited 20 cSOL
                // => allowed borrow value: 20 cSOL * 2(SOL/cSOL) * 0.5(ltv) * $5 = $100
                // => borrowed value upper bound: $50
                // => max withdraw value: ($100 - $50) / 0.5 = $100
                // => max withdraw liquidity amount: $100 / $5 = 20 SOL
                // => max withdraw collateral amount: 20 SOL / 2(SOL/cSOL) = 10 cSOL
                // after withdrawing, the new allowed borrow value is:
                // 10 cSOL * 2(SOL/cSOL) * 0.5(ltv) * $5 = $50, which is exactly what we want.
                expected_max_withdraw_amount: 10 * LAMPORTS_PER_SOL, // 10 cSOL
            }),
            // regular case
            Just(MaxWithdrawAmountTestCase {
                obligation: Obligation {
                    deposits: vec![ObligationCollateral {
                        deposited_amount: 20 * LAMPORTS_PER_SOL,
                        ..ObligationCollateral::default()
                    }],
                    borrows: vec![ObligationLiquidity {
                        borrowed_amount_wads: Decimal::from(10u64),
                        ..ObligationLiquidity::default()
                    }],

                    allowed_borrow_value: Decimal::from(100u64),
                    borrowed_value_upper_bound: Decimal::from(50u64),
                    ..Obligation::default()
                },

                reserve: Reserve {
                    config: ReserveConfig { loan_to_value_ratio: 50, ..ReserveConfig::default() },
                    liquidity: ReserveLiquidity {
                        available_amount: 100 * LAMPORTS_PER_SOL,
                        borrowed_amount_wads: Decimal::zero(),
                        market_price: Decimal::from(10u64),
                        smoothed_market_price: Decimal::from(10u64),
                        extra_market_price: Some(Decimal::from(5u64)),
                        mint_decimals: 9,
                        ..ReserveLiquidity::default()
                    },
                    collateral: ReserveCollateral {
                        mint_total_supply: 50 * LAMPORTS_PER_SOL,
                        ..ReserveCollateral::default()
                    },
                    ..Reserve::default()
                },

                // deposited 20 cSOL
                // => allowed borrow value: 20 cSOL * 2(SOL/cSOL) * 0.5(ltv) * $5 = $100
                // => borrowed value upper bound: $50
                // => max withdraw value: ($100 - $50) / 0.5 = $100
                // => max withdraw liquidity amount: $100 / $5 = 20 SOL
                // => max withdraw collateral amount: 20 SOL / 2(SOL/cSOL) = 10 cSOL
                // after withdrawing, the new allowed borrow value is:
                // 10 cSOL * 2(SOL/cSOL) * 0.5(ltv) * $5 = $50, which is exactly what we want.
                expected_max_withdraw_amount: 10 * LAMPORTS_PER_SOL, // 10 cSOL
            }),
            // same case as above but this time we didn't deposit that much collateral
            Just(MaxWithdrawAmountTestCase {
                obligation: Obligation {
                    deposits: vec![ObligationCollateral {
                        deposited_amount: 2 * LAMPORTS_PER_SOL,
                        ..ObligationCollateral::default()
                    }],
                    borrows: vec![ObligationLiquidity {
                        borrowed_amount_wads: Decimal::from(10u64),
                        ..ObligationLiquidity::default()
                    }],

                    allowed_borrow_value: Decimal::from(100u64),
                    borrowed_value_upper_bound: Decimal::from(50u64),
                    ..Obligation::default()
                },

                reserve: Reserve {
                    config: ReserveConfig { loan_to_value_ratio: 50, ..ReserveConfig::default() },
                    liquidity: ReserveLiquidity {
                        available_amount: 100 * LAMPORTS_PER_SOL,
                        borrowed_amount_wads: Decimal::zero(),
                        market_price: Decimal::from(10u64),
                        smoothed_market_price: Decimal::from(5u64),
                        mint_decimals: 9,
                        ..ReserveLiquidity::default()
                    },
                    collateral: ReserveCollateral {
                        mint_total_supply: 50 * LAMPORTS_PER_SOL,
                        ..ReserveCollateral::default()
                    },
                    ..Reserve::default()
                },

                expected_max_withdraw_amount: 2 * LAMPORTS_PER_SOL,
            }),
            // no borrows so we can withdraw everything
            Just(MaxWithdrawAmountTestCase {
                obligation: Obligation {
                    deposits: vec![ObligationCollateral {
                        deposited_amount: 100 * LAMPORTS_PER_SOL,
                        ..ObligationCollateral::default()
                    }],

                    allowed_borrow_value: Decimal::from(100u64),
                    ..Obligation::default()
                },

                reserve: Reserve {
                    config: ReserveConfig { loan_to_value_ratio: 50, ..ReserveConfig::default() },
                    ..Reserve::default()
                },
                expected_max_withdraw_amount: 100 * LAMPORTS_PER_SOL,
            }),
            // ltv is 0 and the obligation is healthy so we can withdraw everything
            Just(MaxWithdrawAmountTestCase {
                obligation: Obligation {
                    deposits: vec![ObligationCollateral {
                        deposited_amount: 100 * LAMPORTS_PER_SOL,
                        ..ObligationCollateral::default()
                    }],
                    borrows: vec![ObligationLiquidity {
                        borrowed_amount_wads: Decimal::from(10u64),
                        ..ObligationLiquidity::default()
                    }],

                    allowed_borrow_value: Decimal::from(100u64),
                    borrowed_value_upper_bound: Decimal::from(50u64),
                    ..Obligation::default()
                },

                reserve: Reserve::default(),
                expected_max_withdraw_amount: 100 * LAMPORTS_PER_SOL,
            }),
            // ltv is 0 but the obligation is unhealthy so we can't withdraw anything
            Just(MaxWithdrawAmountTestCase {
                obligation: Obligation {
                    deposits: vec![ObligationCollateral {
                        deposited_amount: 100 * LAMPORTS_PER_SOL,
                        ..ObligationCollateral::default()
                    }],
                    borrows: vec![ObligationLiquidity {
                        borrowed_amount_wads: Decimal::from(10u64),
                        ..ObligationLiquidity::default()
                    }],

                    allowed_borrow_value: Decimal::from(100u64),
                    borrowed_value_upper_bound: Decimal::from(100u64),
                    ..Obligation::default()
                },

                reserve: Reserve::default(),
                expected_max_withdraw_amount: 0,
            }),
        ]
    }

    proptest! {
        #[test]
        fn max_withdraw_amount(test_case in max_withdraw_amount_test_cases()) {
            let max_withdraw_amount = test_case.obligation.max_withdraw_amount(
                &test_case.obligation.deposits[0],
                &test_case.reserve,
            ).unwrap();

            assert_eq!(max_withdraw_amount, test_case.expected_max_withdraw_amount);
        }
    }
}
