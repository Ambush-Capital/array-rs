use super::{normalize::RateNormalizer, PoolLiquidityNormalizer};
use crate::{
    kamino::models::reserve::Reserve as KaminoReserve,
    marginfi::models::group::{Bank, MarginfiGroup},
    save::models::Reserve,
};
use common::LendingReserve;
use drift::models::idl::accounts::SpotMarket;

// Wrapper types for protocol reserves
pub struct SaveReserveWrapper<'a> {
    pub reserve: &'a Reserve,
    pub market_name: &'a str,
    pub slot: u64,
}

impl<'a> From<SaveReserveWrapper<'a>> for LendingReserve {
    fn from(wrapper: SaveReserveWrapper<'a>) -> Self {
        let supply_rate = wrapper.reserve.current_supply_apr_unadjusted().unwrap();
        let borrow_rate = wrapper.reserve.current_borrow_rate().unwrap();
        let borrow_apy = wrapper.reserve.current_borrow_apy_slot_unadjusted().unwrap();
        let supply_apy = wrapper.reserve.current_supply_apr_slot_adjusted().unwrap();

        let rate_normalizer = RateNormalizer::save();
        let liquidity_normalizer = PoolLiquidityNormalizer::save();

        LendingReserve {
            protocol_name: "Save".to_string(),
            market_name: wrapper.market_name.to_string(),
            total_supply: liquidity_normalizer
                .normalize_amount(wrapper.reserve.liquidity.total_supply().unwrap())
                .unwrap(),
            total_borrows: liquidity_normalizer
                .normalize_amount(wrapper.reserve.liquidity.borrowed_amount_wads)
                .unwrap(),
            supply_rate: rate_normalizer.normalize_rate(supply_rate).unwrap(),
            borrow_rate: rate_normalizer.normalize_rate(borrow_rate).unwrap(),
            borrow_apy: rate_normalizer.normalize_rate(borrow_apy).unwrap(),
            supply_apy: rate_normalizer.normalize_rate(supply_apy).unwrap(),
            collateral_assets: vec![],
            slot: wrapper.slot,
        }
    }
}

pub struct MarginfiReserveWrapper<'a> {
    pub bank: &'a Bank,
    pub group: &'a MarginfiGroup,
    pub market_name: &'a str,
    pub slot: u64,
}

impl<'a> From<MarginfiReserveWrapper<'a>> for LendingReserve {
    fn from(wrapper: MarginfiReserveWrapper<'a>) -> Self {
        let interest_rates = wrapper.bank.get_interest_rate(wrapper.group).unwrap();
        let rate_normalizer = RateNormalizer::marginfi();
        let liquidity_normalizer = PoolLiquidityNormalizer::marginfi();

        LendingReserve {
            protocol_name: "Marginfi".to_string(),
            market_name: wrapper.market_name.to_string(),
            total_supply: liquidity_normalizer
                .normalize_amount(wrapper.bank.get_total_supply().unwrap())
                .unwrap(),
            total_borrows: liquidity_normalizer
                .normalize_amount(wrapper.bank.get_total_borrowed().unwrap())
                .unwrap(),
            supply_rate: rate_normalizer.normalize_rate(interest_rates.lending_rate_apr).unwrap(),
            borrow_rate: rate_normalizer.normalize_rate(interest_rates.borrowing_rate_apr).unwrap(),
            borrow_apy: rate_normalizer
                .normalize_rate(interest_rates.borrowing_rate_apy())
                .unwrap(),
            supply_apy: rate_normalizer.normalize_rate(interest_rates.lending_rate_apy()).unwrap(),
            collateral_assets: vec![],
            slot: wrapper.slot,
        }
    }
}

pub struct KaminoReserveWrapper<'a> {
    pub reserve: &'a KaminoReserve,
    pub market_name: &'a str,
    pub slot: u64,
}

impl<'a> From<KaminoReserveWrapper<'a>> for LendingReserve {
    fn from(wrapper: KaminoReserveWrapper<'a>) -> Self {
        let supply_rate = wrapper.reserve.current_supply_apr_unadjusted().unwrap();
        let borrow_rate = wrapper.reserve.current_borrow_rate_unadjusted().unwrap();
        let borrow_apy = wrapper.reserve.current_borrow_apy_unadjusted().unwrap();
        let supply_apy = wrapper.reserve.current_supply_apy_unadjusted().unwrap();
        let rate_normalizer = RateNormalizer::kamino();
        let liquidity_normalizer = PoolLiquidityNormalizer::kamino();

        LendingReserve {
            protocol_name: "Kamino".to_string(),
            market_name: wrapper.market_name.to_string(),
            total_supply: liquidity_normalizer
                .normalize_amount(wrapper.reserve.liquidity.total_supply().unwrap())
                .unwrap(),
            total_borrows: liquidity_normalizer
                .normalize_amount(wrapper.reserve.liquidity.total_borrow())
                .unwrap(),
            supply_rate: rate_normalizer.normalize_rate(supply_rate).unwrap(),
            borrow_rate: rate_normalizer.normalize_rate(borrow_rate).unwrap(),
            borrow_apy: rate_normalizer.normalize_rate(borrow_apy).unwrap(),
            supply_apy: rate_normalizer.normalize_rate(supply_apy).unwrap(),
            collateral_assets: vec![],
            slot: wrapper.slot,
        }
    }
}

pub struct DriftReserveWrapper<'a> {
    pub market: &'a SpotMarket,
    pub market_name: &'a str,
    pub slot: u64,
}

impl<'a> From<DriftReserveWrapper<'a>> for LendingReserve {
    fn from(wrapper: DriftReserveWrapper<'a>) -> Self {
        let supply_rate = wrapper.market.get_borrow_rate().unwrap();
        let borrow_rate = wrapper.market.get_deposit_rate().unwrap();
        let borrow_apy = wrapper.market.current_borrow_apy_unadjusted().unwrap();
        let supply_apy = wrapper.market.current_supply_apy_unadjusted().unwrap();
        let rate_normalizer = RateNormalizer::drift();
        let liquidity_normalizer = PoolLiquidityNormalizer::drift();

        LendingReserve {
            protocol_name: "Drift".to_string(),
            market_name: wrapper.market_name.trim().replace('\0', "").to_string(),
            total_supply: liquidity_normalizer
                .normalize_amount(wrapper.market.get_deposits().unwrap_or(0))
                .unwrap(),
            total_borrows: liquidity_normalizer
                .normalize_amount(wrapper.market.get_borrows().unwrap_or(0))
                .unwrap(),
            supply_rate: rate_normalizer.normalize_rate(supply_rate).unwrap(),
            borrow_rate: rate_normalizer.normalize_rate(borrow_rate).unwrap(),
            borrow_apy: rate_normalizer.normalize_rate(borrow_apy).unwrap(),
            supply_apy: rate_normalizer.normalize_rate(supply_apy).unwrap(),
            collateral_assets: vec![],
            slot: wrapper.slot,
        }
    }
}
