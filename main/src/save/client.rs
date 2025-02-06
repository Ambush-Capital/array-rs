use crate::save::models::{Obligation, Reserve};
use aggregation::{ObligationType, UserObligation};
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

    pub fn get_user_obligations(&self, owner_pubkey: &str) -> Result<Vec<UserObligation>, String> {
        let obligations = self.get_obligations(owner_pubkey)?;
        let mut user_obligations = Vec::new();

        for obligation in obligations {
            // Process deposits
            for deposit in obligation.deposits {
                let reserve_account = self
                    .program
                    .rpc()
                    .get_account(&Pubkey::new_from_array(deposit.deposit_reserve.to_bytes()))
                    .map_err(|e| format!("Failed to fetch reserve: {}", e))?;

                let reserve = Reserve::unpack(&reserve_account.data)
                    .map_err(|e| format!("Failed to unpack reserve: {}", e))?;

                let exchange_rate = reserve
                    .collateral_exchange_rate()
                    .map_err(|e| format!("Failed to get collateral exchange rate: {}", e))?;

                let amount =
                    exchange_rate.collateral_to_liquidity(deposit.deposited_amount).unwrap_or(0);

                user_obligations.push(UserObligation {
                    symbol: reserve.liquidity.mint_pubkey.to_string(),
                    market_price_sf: deposit.market_value.try_round_u64().unwrap_or(0),
                    mint: Pubkey::new_from_array(reserve.liquidity.mint_pubkey.to_bytes()),
                    mint_decimals: reserve.liquidity.mint_decimals as u32,
                    amount,
                    protocol_name: "Solend".to_string(),
                    market_name: "Main Pool".to_string(), // Could be improved by looking up actual pool name
                    obligation_type: ObligationType::Asset,
                });
            }

            // Process borrows
            for borrow in obligation.borrows {
                let reserve_account = self
                    .program
                    .rpc()
                    .get_account(&Pubkey::new_from_array(borrow.borrow_reserve.to_bytes()))
                    .map_err(|e| format!("Failed to fetch reserve: {}", e))?;

                let reserve = Reserve::unpack(&reserve_account.data)
                    .map_err(|e| format!("Failed to unpack reserve: {}", e))?;

                user_obligations.push(UserObligation {
                    symbol: reserve.liquidity.mint_pubkey.to_string(),
                    market_price_sf: borrow.market_value.try_round_u64().unwrap_or(0),
                    mint: Pubkey::new_from_array(reserve.liquidity.mint_pubkey.to_bytes()),
                    mint_decimals: reserve.liquidity.mint_decimals as u32,
                    amount: borrow.borrowed_amount_wads.try_round_u64().unwrap_or(0),
                    protocol_name: "Solend".to_string(),
                    market_name: "Main Pool".to_string(), // Could be improved by looking up actual pool name
                    obligation_type: ObligationType::Liability,
                });
            }
        }

        Ok(user_obligations)
    }

    fn get_obligations(&self, owner_pubkey: &str) -> Result<Vec<Obligation>, String> {
        let mut ret = Vec::new();
        let owner = owner_pubkey.parse::<Pubkey>().map_err(|e| format!("Invalid pubkey: {}", e))?;

        let filters = vec![
            RpcFilterType::DataSize(Obligation::LEN as u64),
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                1 + 8 + 1 + 32, // Skip version(1) + last_update(8+1) + lending_market(32) to get to owner
                owner.to_bytes().to_vec(),
            )),
        ];

        let obligations = self
            .program
            .rpc()
            .get_program_accounts_with_config(
                &self.program.id(),
                RpcProgramAccountsConfig {
                    filters: Some(filters),
                    account_config: solana_client::rpc_config::RpcAccountInfoConfig {
                        encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .map_err(|e| format!("Failed to fetch obligations: {}", e))?;

        if obligations.is_empty() {
            println!("No Solend obligations found for {}", owner_pubkey);
            return Ok(ret);
        }

        println!("\nSolend Obligations for {}:", owner_pubkey);
        for (pubkey, account) in obligations {
            let obligation = match Obligation::unpack(&account.data) {
                Ok(obligation) => obligation,
                Err(_) => {
                    continue;
                }
            };
            if obligation.owner.to_string() != owner.to_string() {
                continue;
            }

            if obligation.deposits.is_empty() && obligation.borrows.is_empty() {
                continue;
            }

            println!("\nObligation address: {}", pubkey);
            println!("Owner: {}", obligation.owner);
            println!("Lending Market: {}", obligation.lending_market);
            println!("Deposited value: {:.3}", obligation.deposited_value);
            println!("Borrowed value: {:.3}", obligation.borrowed_value);
            println!("Allowed borrow value: {:.3}", obligation.allowed_borrow_value);
            println!("Unhealthy borrow value: {:.3}", obligation.unhealthy_borrow_value);
            println!(
                "Super unhealthy borrow value: {:.3}",
                obligation.super_unhealthy_borrow_value
            );
            println!("Borrowing isolated asset: {}", obligation.borrowing_isolated_asset);

            // Print deposits
            if !obligation.deposits.is_empty() {
                println!("\nDeposits:");
                let decimals = 6; // or reserve.liquidity.mint_decimals
                                  // Convert the raw deposited_amount to a UI amount:
                for deposit in &obligation.deposits {
                    let reserve_account = self
                        .program
                        .rpc()
                        .get_account(&Pubkey::new_from_array(deposit.deposit_reserve.to_bytes()))
                        .map_err(|e| format!("Failed to fetch reserve: {}", e))?;

                    let reserve = Reserve::unpack(&reserve_account.data)
                        .map_err(|e| format!("Failed to unpack reserve: {}", e))?;
                    let exchange_rate = reserve
                        .collateral_exchange_rate()
                        .map_err(|e| format!("Failed to get collateral exchange rate: {}", e))?;

                    let ui_amount = exchange_rate
                        .collateral_to_liquidity(deposit.deposited_amount)
                        .unwrap() as f64
                        / 10f64.powi(decimals);
                    println!("  Reserve: {}", deposit.deposit_reserve);
                    println!("  Amount: {:.3}", ui_amount);
                    println!("  Market Value: {:.3}", deposit.market_value);
                    println!("  Attributed Borrow Value: {:.3}", deposit.attributed_borrow_value);
                }
            }

            // Print borrows
            if !obligation.borrows.is_empty() {
                println!("\nBorrows:");
                for borrow in &obligation.borrows {
                    println!("  Reserve: {}", borrow.borrow_reserve);
                    println!("  Amount: {:.3}", borrow.borrowed_amount_wads);
                    println!("  Market Value: {:.3}", borrow.market_value);
                }
            }
            ret.push(obligation);
        }

        Ok(ret)
    }
}
