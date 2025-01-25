use crate::casting::Cast;
use crate::error::{DriftResult, ErrorCode};
use crate::math::constants::SPOT_UTILIZATION_PRECISION;
use crate::math::safe_math::SafeMath;
use crate::models::idl::types::SpotBalanceType;
use anchor_client::{Client, Program};
use anchor_lang::AccountDeserialize;
use prettytable::{row, Table};
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use std::ops::Mul;
use std::{ops::Deref, str::FromStr};

use crate::models::idl::accounts::SpotMarket;

pub struct DriftClient<C> {
    program: Program<C>,
    drift_program_id: Pubkey,
    pub spot_markets: Vec<(Pubkey, SpotMarket)>,
}

impl<C: Clone + Deref<Target = impl Signer>> DriftClient<C> {
    pub fn new(client: &Client<C>) -> Self {
        let drift_program_id = Pubkey::from_str("dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH")
            .expect("Invalid Drift Program ID");

        let program = client.program(drift_program_id).expect("Failed to load Drift program");

        Self { program, drift_program_id, spot_markets: Vec::new() }
    }

    pub fn load_spot_markets(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Filter for spot market accounts using the discriminator
        let filters = vec![RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            0,
            [100, 177, 8, 107, 168, 65, 65, 39].to_vec(),
        ))];

        let account_config = RpcAccountInfoConfig {
            encoding: Some(solana_account_decoder::UiAccountEncoding::Base64Zstd),
            ..RpcAccountInfoConfig::default()
        };

        let gpa_config = RpcProgramAccountsConfig {
            filters: Some(filters),
            account_config,
            with_context: Some(true),
        };

        let accounts = self
            .program
            .rpc()
            .get_program_accounts_with_config(&self.drift_program_id, gpa_config)?;

        self.spot_markets = accounts
            .into_iter()
            .filter_map(|(pubkey, account)| {
                SpotMarket::try_deserialize(&mut &account.data[..])
                    .map(|market| (pubkey, market))
                    .ok()
            })
            .collect();

        Ok(())
    }

    pub fn print_spot_markets(&self) {
        let mut table = Table::new();
        table.add_row(row![
            "Market Index",
            "Name",
            "Oracle",
            "Mint",
            "Status",
            "Asset Tier",
            "Total Deposits",
            "Total Borrows",
            "Market Address"
        ]);

        for (pubkey, market) in &self.spot_markets {
            let precision_decrease = 10_u128.pow(19_u32.checked_sub(market.decimals).unwrap_or(0));

            let utilization = calculate_spot_market_utilization(&market).unwrap_or(100);

            table.add_row(row![
                market.market_index,
                String::from_utf8_lossy(&market.name).trim_matches(char::from(0)),
                market.oracle,
                market.mint,
                format!("{:?}", market.status),
                format!("{:?}", market.asset_tier),
                market.deposit_balance.as_u128().mul(market.cumulative_deposit_interest.as_u128())
                    / precision_decrease,
                market.borrow_balance.as_u128().mul(market.cumulative_borrow_interest.as_u128())
                    / precision_decrease,
                calculate_borrow_rate(&market, utilization).unwrap_or(10),
                utilization,
                pubkey.to_string()
            ]);
        }

        table.printstd();
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
    let deposit_token_amount = get_token_amount(
        spot_market.deposit_balance.as_u128(),
        spot_market,
        &SpotBalanceType::Deposit,
    )?;
    let borrow_token_amount = get_token_amount(
        spot_market.borrow_balance.as_u128(),
        spot_market,
        &SpotBalanceType::Borrow,
    )?;
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
            balance.safe_mul(cumulative_interest.as_u128())?.safe_div(precision_decrease)?
        }
        SpotBalanceType::Borrow => {
            balance.safe_mul(cumulative_interest.as_u128())?.safe_div_ceil(precision_decrease)?
        }
    };

    Ok(token_amount)
}

impl From<Box<dyn std::error::Error>> for ErrorCode {
    fn from(_: Box<dyn std::error::Error>) -> Self {
        ErrorCode::CastingFailure
    }
}
