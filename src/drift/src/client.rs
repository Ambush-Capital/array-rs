use crate::error::ErrorCode;
use anchor_client::{Client, Program};
use anchor_lang::AccountDeserialize;
use prettytable::{row, Table};
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_sdk::{pubkey::Pubkey, signer::Signer};
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
                    .filter(|(_, market)| market.is_active())
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
            "Borrow Rate",
            "Utilization",
            "Market Address"
        ]);

        for (pubkey, market) in &self.spot_markets {
            table.add_row(row![
                market.market_index,
                String::from_utf8_lossy(&market.name).trim_matches(char::from(0)),
                market.oracle,
                market.mint,
                format!("{:?}", market.status),
                format!("{:?}", market.asset_tier),
                market.get_deposits().unwrap_or(0),
                market.get_borrows().unwrap_or(0),
                market.get_borrow_rate().unwrap_or(0),
                market.get_utilization().unwrap_or(0),
                pubkey.to_string()
            ]);
        }

        table.printstd();
    }
}

impl From<Box<dyn std::error::Error>> for ErrorCode {
    fn from(_: Box<dyn std::error::Error>) -> Self {
        ErrorCode::CastingFailure
    }
}
