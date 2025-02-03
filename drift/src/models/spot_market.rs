use crate::casting::Cast;
use crate::error::DriftResult;
use crate::math::constants::{PERCENTAGE_PRECISION, SPOT_UTILIZATION_PRECISION};
use crate::math::safe_math::SafeMath;

use super::idl::types::{MarketStatus, SpotBalanceType};
use crate::models::idl::accounts::SpotMarket;

impl SpotMarket {
    pub fn get_min_borrow_rate(self) -> DriftResult<u32> {
        self.min_borrow_rate.cast::<u32>()?.safe_mul((PERCENTAGE_PRECISION / 200).cast()?)
    }

    pub fn get_borrow_rate(&self) -> DriftResult<u128> {
        let utilization = self.get_utilization()?;
        calculate_borrow_rate(self, utilization)
    }

    pub fn get_deposit_rate(&self) -> DriftResult<u128> {
        let utilization = self.get_utilization()?;
        let borrow_rate = self.get_borrow_rate()?;
        calculate_deposit_rate(self, utilization, borrow_rate)
    }

    pub fn get_deposits(&self) -> DriftResult<u128> {
        get_token_amount(self.deposit_balance, self, &SpotBalanceType::Deposit)
    }

    pub fn get_borrows(&self) -> DriftResult<u128> {
        get_token_amount(self.borrow_balance, self, &SpotBalanceType::Borrow)
    }

    pub fn get_available_deposits(&self) -> DriftResult<u128> {
        let deposit_token_amount =
            get_token_amount(self.deposit_balance, self, &SpotBalanceType::Deposit)?;

        let borrow_token_amount =
            get_token_amount(self.borrow_balance, self, &SpotBalanceType::Borrow)?;

        deposit_token_amount.safe_sub(borrow_token_amount)
    }

    pub fn get_precision(self) -> u64 {
        10_u64.pow(self.decimals)
    }

    pub fn get_utilization(self) -> DriftResult<u128> {
        let deposit_token_amount =
            get_token_amount(self.deposit_balance, &self, &SpotBalanceType::Deposit)?;

        let borrow_token_amount =
            get_token_amount(self.borrow_balance, &self, &SpotBalanceType::Borrow)?;
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

    pub fn is_active(&self) -> bool {
        self.status == MarketStatus::Active
    }
}

pub fn calculate_borrow_rate(spot_market: &SpotMarket, utilization: u128) -> DriftResult<u128> {
    let borrow_rate = if utilization > spot_market.optimal_utilization.cast::<u128>()? {
        let surplus_utilization = utilization.safe_sub(spot_market.optimal_utilization.cast()?)?;

        let borrow_rate_slope = spot_market
            .max_borrow_rate
            .cast::<u128>()?
            .safe_sub(spot_market.optimal_borrow_rate.cast()?)?
            .safe_mul(SPOT_UTILIZATION_PRECISION)?
            .safe_div(
                SPOT_UTILIZATION_PRECISION.safe_sub(spot_market.optimal_utilization.cast()?)?,
            )?;

        spot_market.optimal_borrow_rate.cast::<u128>()?.safe_add(
            surplus_utilization
                .safe_mul(borrow_rate_slope)?
                .safe_div(SPOT_UTILIZATION_PRECISION)?,
        )?
    } else {
        let borrow_rate_slope = spot_market
            .optimal_borrow_rate
            .cast::<u128>()?
            .safe_mul(SPOT_UTILIZATION_PRECISION)?
            .safe_div(spot_market.optimal_utilization.cast()?)?;

        utilization.safe_mul(borrow_rate_slope)?.safe_div(SPOT_UTILIZATION_PRECISION)?
    }
    .max(spot_market.get_min_borrow_rate()?.cast()?);

    Ok(borrow_rate)
}

pub fn calculate_spot_market_utilization(spot_market: &SpotMarket) -> DriftResult<u128> {
    let deposit_token_amount =
        get_token_amount(spot_market.deposit_balance, spot_market, &SpotBalanceType::Deposit)?;
    let borrow_token_amount =
        get_token_amount(spot_market.borrow_balance, spot_market, &SpotBalanceType::Borrow)?;
    let utilization = calculate_utilization(deposit_token_amount, borrow_token_amount)?;

    Ok(utilization)
}

pub fn calculate_utilization(
    deposit_token_amount: u128,
    borrow_token_amount: u128,
) -> DriftResult<u128> {
    let utilization = borrow_token_amount
        .safe_mul(SPOT_UTILIZATION_PRECISION)?
        .checked_div(deposit_token_amount)
        .unwrap_or({
            if deposit_token_amount == 0 && borrow_token_amount == 0 {
                0_u128
            } else {
                // if there are borrows without deposits, default to maximum utilization rate
                SPOT_UTILIZATION_PRECISION
            }
        });

    Ok(utilization)
}

pub fn get_token_amount(
    balance: u128,
    spot_market: &SpotMarket,
    balance_type: &SpotBalanceType,
) -> DriftResult<u128> {
    let precision_decrease = 10_u128.pow(19_u32.safe_sub(spot_market.decimals)?);

    let cumulative_interest = match balance_type {
        SpotBalanceType::Deposit => spot_market.cumulative_deposit_interest,
        SpotBalanceType::Borrow => spot_market.cumulative_borrow_interest,
    };

    let token_amount = match balance_type {
        SpotBalanceType::Deposit => {
            balance.safe_mul(cumulative_interest)?.safe_div(precision_decrease)?
        }
        SpotBalanceType::Borrow => {
            balance.safe_mul(cumulative_interest)?.safe_div_ceil(precision_decrease)?
        }
    };

    Ok(token_amount)
}

pub fn calculate_deposit_rate(
    spot_market: &SpotMarket,
    utilization: u128,
    borrow_rate: u128,
) -> DriftResult<u128> {
    borrow_rate
        .safe_mul(PERCENTAGE_PRECISION.safe_sub(spot_market.insurance_fund.total_factor.cast()?)?)?
        .safe_mul(utilization)?
        .safe_div(SPOT_UTILIZATION_PRECISION)?
        .safe_div(PERCENTAGE_PRECISION)
}
