use crate::save::models::Reserve;
use anchor_client::{Client, Program};
use prettytable::{row, Table};
use solana_client::{
    rpc_config::RpcProgramAccountsConfig,
    rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType},
};
use solana_program::program_pack::Pack;
use solana_sdk::{pubkey::Pubkey, signature::Signer};
use std::collections::HashMap;
use std::ops::Deref;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SolendPool {
    pub name: String,
    pub pubkey: Pubkey,
    pub reserves: Vec<Reserve>,
}

pub struct SaveClient<C> {
    program: Program<C>,
    pub pools: Vec<SolendPool>,
}

impl<C: Clone + Deref<Target = impl Signer>> SaveClient<C> {
    pub fn new(client: &Client<C>) -> Self {
        let solend_program_id = "So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo"
            .parse()
            .expect("Failed to parse Solend program ID");

        let program = client.program(solend_program_id).expect("Failed to create program client");

        // Initialize with known Solend pools
        let pools = vec![
            SolendPool {
                name: "Main Pool".to_string(),
                pubkey: "4UpD2fh7xH3VP9QQaXtsS1YY3bxzWhtfpks7FatyKvdY".parse().unwrap(),
                reserves: Vec::new(),
            },
            SolendPool {
                name: "JLP Pool".to_string(),
                pubkey: "7XttJ7hp83u5euzT7ybC5zsjdgKA4WPbQHVS27CATAJH".parse().unwrap(),
                reserves: Vec::new(),
            },
            SolendPool {
                name: "JLP/SOL/USDC Pool".to_string(),
                pubkey: "ErM46rCeAtGtEKjvZ3tuGrzL6L5nVq6pFuXukocbKqGX".parse().unwrap(),
                reserves: Vec::new(),
            },
            SolendPool {
                name: "Turbo SOL Pool".to_string(),
                pubkey: "7RCz8wb6WXxUhAigok9ttgrVgDFFFbibcirECzWSBauM".parse().unwrap(),
                reserves: Vec::new(),
            },
        ];

        Self { program, pools }
    }

    pub fn load_reserves_for_pool(
        program: &Program<C>,
        pool: &SolendPool,
    ) -> Result<Vec<Reserve>, String> {
        let reserve_filters = vec![
            RpcFilterType::DataSize(Reserve::LEN as u64),
            RpcFilterType::Memcmp(Memcmp::new(
                10,
                MemcmpEncodedBytes::Base58(pool.pubkey.to_string()),
            )),
        ];

        let reserve_accounts = program
            .rpc()
            .get_program_accounts_with_config(
                &program.id(),
                RpcProgramAccountsConfig {
                    filters: Some(reserve_filters),
                    account_config: solana_client::rpc_config::RpcAccountInfoConfig {
                        encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .map_err(|e| format!("Failed to fetch reserve accounts: {}", e))?;

        let reserves: Vec<Reserve> = reserve_accounts
            .into_iter()
            .filter_map(|(_, account)| Reserve::unpack(&account.data).ok())
            .collect();

        Ok(reserves)
    }

    pub fn load_all_reserves(&mut self) -> Result<(), String> {
        for pool in &mut self.pools {
            let reserves = SaveClient::<C>::load_reserves_for_pool(&self.program, pool)?;
            pool.reserves = reserves;
        }
        Ok(())
    }

    pub fn print_pools(&self, reserves_by_pool: &HashMap<SolendPool, Vec<(Pubkey, Reserve)>>) {
        let mut table = Table::new();
        table.add_row(row![
            "Pool",
            "Token Mint Address",
            "Borrow Amount (m)",
            "Available Amount (m)",
            "Supply Amount (m)",
            "Market Price",
            "Util Rate",
            "Borrow APR",
            "Supply APR",
            "Pub Key"
        ]);

        for (pool, reserves) in reserves_by_pool {
            for (pubkey, reserve) in reserves {
                table.add_row(row![
                    pool.name,
                    reserve.liquidity.mint_pubkey.to_string(),
                    format!(
                        "{:.3}",
                        reserve.liquidity.borrowed_amount_wads.to_scaled_val().unwrap_or(0) as f64
                            / 1_000_000.0
                    ),
                    format!("{:.3}", reserve.liquidity.available_amount as f64 / 1_000_000.0),
                    format!("{:.3}", reserve.liquidity.total_supply().unwrap()),
                    format!("{:.3}", reserve.liquidity.market_price),
                    format!("{:.1}", reserve.liquidity.utilization_rate().unwrap()),
                    format!("{:.1}%", reserve.current_slot_adjusted_borrow_rate()),
                    format!("{:.1}%", reserve.current_supply_apr()),
                    pubkey.to_string()
                ]);
                // println!("{} {:#?}", pool.name,reserve.config);
            }
        }
        table.printstd();
    }
}
