use crate::casting::Cast;
use crate::client::{calculate_utilization, get_token_amount};
use crate::error::DriftResult;
use crate::math::constants::PERCENTAGE_PRECISION;
use crate::math::safe_math::SafeMath;

use super::idl::accounts::SpotMarket;
use super::idl::types::{MarketStatus, SpotBalanceType};

impl SpotMarket {
    pub fn get_min_borrow_rate(self) -> DriftResult<u32> {
        self.min_borrow_rate.cast::<u32>()?.safe_mul((PERCENTAGE_PRECISION / 200).cast()?)
    }

    pub fn get_deposits(&self) -> DriftResult<u128> {
        get_token_amount(self.deposit_balance.as_u128(), self, &SpotBalanceType::Deposit)
    }

    pub fn get_borrows(&self) -> DriftResult<u128> {
        get_token_amount(self.borrow_balance.as_u128(), self, &SpotBalanceType::Borrow)
    }

    pub fn get_available_deposits(&self) -> DriftResult<u128> {
        let deposit_token_amount =
            get_token_amount(self.deposit_balance.as_u128(), self, &SpotBalanceType::Deposit)?;

        let borrow_token_amount =
            get_token_amount(self.borrow_balance.as_u128(), self, &SpotBalanceType::Borrow)?;

        deposit_token_amount.safe_sub(borrow_token_amount)
    }

    pub fn get_precision(self) -> u64 {
        10_u64.pow(self.decimals)
    }

    pub fn get_utilization(self) -> DriftResult<u128> {
        let deposit_token_amount =
            get_token_amount(self.deposit_balance.as_u128(), &self, &SpotBalanceType::Deposit)?;

        let borrow_token_amount =
            get_token_amount(self.borrow_balance.as_u128(), &self, &SpotBalanceType::Borrow)?;
        calculate_utilization(deposit_token_amount, borrow_token_amount)
    }

    pub fn is_in_settlement(&self, now: i64) -> bool {
        let in_settlement =
            matches!(self.status, MarketStatus::Settlement | MarketStatus::Delisted);
        let expired = self.expiry_ts != 0 && now >= self.expiry_ts;
        in_settlement || expired
    }

    pub fn is_reduce_only(&self) -> bool {
        self.status == MarketStatus::ReduceOnly
    }
}
