use anchor_client::{Client, Program};
use borsh::BorshDeserialize;
use common::{ObligationType, UserObligation};
use prettytable::{row, Table};
use solana_client::{
    rpc_config::RpcProgramAccountsConfig,
    rpc_filter::{self, Memcmp, MemcmpEncodedBytes},
};
use solana_sdk::{account::Account, pubkey::Pubkey, signer::Signer};
use std::{collections::HashMap, ops::Deref, str::FromStr};

use crate::kamino::{
    models::{lending_market::LendingMarket, reserve::Reserve},
    utils::consts::OBLIGATION_SIZE,
    utils::fraction::Fraction,
};
use crate::{
    debug,
    kamino::{models::obligation::Obligation, utils::consts::RESERVE_SIZE},
};

type KaminoMarkets = (Pubkey, LendingMarket, Vec<(Pubkey, Reserve)>);
type MarketNameMap = HashMap<String, &'static str>;

pub struct KaminoClient<C> {
    program: Program<C>,
    kamino_program_id: Pubkey,
    market_pubkeys: Vec<String>,
    pub markets: Vec<KaminoMarkets>,
    pub market_names: MarketNameMap,
}

impl<C: Clone + Deref<Target = impl Signer>> KaminoClient<C> {
    pub fn new(client: &Client<C>) -> Self {
        let kamino_program_id = Pubkey::from_str("KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD")
            .expect("Invalid Kamino Lending Program ID");

        let market_names: MarketNameMap = [
            ("7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF".to_string(), "Main"),
            ("H6rHXmXoCQvq8Ue81MqNh7ow5ysPa1dSozwW3PU1dDH6".to_string(), "JITO"),
            ("DxXdAyU3kCjnyggvHmY5nAwg5cRbbmdyX3npfDMjjMek".to_string(), "JLP"),
            ("ByYiZxp8QrdN9qbdtaAiePN8AAr3qvTPppNJDpf5DVJ5".to_string(), "Altcoin"),
            ("BJnbcRHqvppTyGesLzWASGKnmnF1wq9jZu6ExrjT7wvF".to_string(), "Ethena"),
        ]
        .into_iter()
        .collect();

        let market_pubkeys: Vec<String> = market_names.keys().cloned().collect();

        let program = client.program(kamino_program_id).expect("Failed to load Kamino program");

        Self { program, kamino_program_id, market_pubkeys, markets: Vec::new(), market_names }
    }

    pub fn load_markets(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.markets = self
            .market_pubkeys
            .iter()
            .filter_map(|pubkey_str| {
                let pubkey = Pubkey::from_str(pubkey_str).ok()?;

                let account_data = self.program.rpc().get_account(&pubkey).ok()?;
                let lending_market = LendingMarket::try_from_slice(&account_data.data[8..]).ok()?;

                let reserves = self.get_reserves(&pubkey).ok()?;
                let parsed_reserves = reserves
                    .into_iter()
                    .filter_map(|(pubkey, account)| {
                        Reserve::try_from_slice(&account.data[8..])
                            .map(|reserve| (pubkey, reserve))
                            .ok()
                    })
                    .collect();

                Some((pubkey, lending_market, parsed_reserves))
            })
            .collect();

        Ok(())
    }

    fn get_reserve_by_pubkey(
        &self,
        pubkey: &Pubkey,
    ) -> Result<Option<&Reserve>, Box<dyn std::error::Error>> {
        for (_, _, reserves) in &self.markets {
            if let Some((_, reserve)) =
                reserves.iter().find(|(reserve_pubkey, _)| reserve_pubkey == pubkey)
            {
                return Ok(Some(reserve));
            }
        }
        Ok(None)
    }

    pub fn print_markets(&self) {
        let mut table = Table::new();
        table.add_row(row![
            "Market",
            "Token Symbol",
            "Borrow Amount (m)",
            "Supply Amount (m)",
            "Available Amount (m)",
            "Market Price",
            "Util Rate",
            "Supply APY",
            "Borrow APY",
            "Token Mint",
            "Reserve"
        ]);

        for (market_pubkey, _, reserves) in &self.markets {
            let market_name =
                self.market_names.get(&market_pubkey.to_string()).unwrap_or(&"Unknown");
            for (pubkey, reserve) in reserves {
                table.add_row(row![
                    market_name,
                    reserve.token_symbol(),
                    format!(
                        "{:.3}",
                        reserve.liquidity.total_borrow().to_num::<f64>() / 1_000_000_000_000.0
                    ),
                    format!(
                        "{:.3}",
                        reserve.liquidity.total_supply().unwrap().to_num::<f64>()
                            / 1_000_000_000_000.0
                    ),
                    format!(
                        "{:.3}",
                        (reserve.liquidity.total_supply().unwrap()
                            - reserve.liquidity.total_borrow())
                        .to_num::<f64>()
                            / 1_000_000_000_000.0
                    ),
                    format!("{:.3}", reserve.liquidity.get_market_price_f()),
                    format!(
                        "{:.1}%",
                        reserve.liquidity.utilization_rate().unwrap().to_num::<f64>() * 100.0
                    ),
                    format!(
                        "{:.1}%",
                        reserve.current_supply_apy_unadjusted().unwrap().to_num::<f64>() * 100.0
                    ),
                    format!(
                        "{:.1}%",
                        reserve.current_borrow_apy_unadjusted().unwrap().to_num::<f64>() * 100.0
                    ),
                    reserve.liquidity.mint_pubkey.to_string(),
                    pubkey.to_string()
                ]);
            }
        }
        table.printstd();
    }

    fn get_reserves(
        &self,
        market_address: &Pubkey,
    ) -> Result<Vec<(Pubkey, Account)>, Box<solana_client::client_error::ClientError>> {
        let filters = vec![
            rpc_filter::RpcFilterType::DataSize((RESERVE_SIZE + 8) as u64),
            rpc_filter::RpcFilterType::Memcmp(Memcmp::new(
                32,
                MemcmpEncodedBytes::Base58(market_address.to_string()),
            )),
        ];

        self.program
            .rpc()
            .get_program_accounts_with_config(
                &self.kamino_program_id,
                RpcProgramAccountsConfig {
                    filters: Some(filters),
                    account_config: solana_client::rpc_config::RpcAccountInfoConfig {
                        encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .map_err(Box::new)
    }

    pub fn get_user_obligations(
        &self,
        owner_pubkey: &str,
    ) -> Result<Vec<UserObligation>, Box<dyn std::error::Error>> {
        let obligations = self.get_obligations(owner_pubkey)?;

        let mut user_obligations = Vec::new();

        for (_, obligation) in obligations {
            let market_name = self
                .market_names
                .get(&obligation.lending_market.to_string())
                .unwrap_or(&"Unknown")
                .to_string();

            // Process deposits
            for deposit in obligation.deposits {
                if let Some(reserve) =
                    self.get_reserve_by_pubkey(&Pubkey::from(deposit.deposit_reserve.to_bytes()))?
                {
                    user_obligations.push(UserObligation {
                        symbol: reserve.token_symbol().to_string(),
                        mint: reserve.liquidity.mint_pubkey.to_string(),
                        mint_decimals: reserve.liquidity.mint_decimals as u32,
                        amount: deposit.deposited_amount,
                        protocol_name: "Kamino".to_string(),
                        market_name: market_name.clone(),
                        obligation_type: ObligationType::Asset,
                    });
                }
            }

            // Process borrows
            for borrow in obligation.borrows {
                if let Some(reserve) =
                    self.get_reserve_by_pubkey(&Pubkey::from(borrow.borrow_reserve.to_bytes()))?
                {
                    user_obligations.push(UserObligation {
                        symbol: reserve.token_symbol().to_string(),
                        mint: reserve.liquidity.mint_pubkey.to_string(),
                        mint_decimals: reserve.liquidity.mint_decimals as u32,
                        amount: borrow.borrowed_amount_sf as u64,
                        protocol_name: "Kamino".to_string(),
                        market_name: market_name.clone(),
                        obligation_type: ObligationType::Liability,
                    });
                }
            }
        }

        Ok(user_obligations)
    }

    fn get_obligations(
        &self,
        owner_pubkey: &str,
    ) -> Result<Vec<(Pubkey, Obligation)>, Box<dyn std::error::Error>> {
        let mut ret = Vec::new();
        let owner = Pubkey::from_str(owner_pubkey)?;

        let filters = vec![
            rpc_filter::RpcFilterType::DataSize(OBLIGATION_SIZE as u64 + 8),
            rpc_filter::RpcFilterType::Memcmp(Memcmp::new(
                8 + 8 + 16 + 32, // Offset for owner field in Obligation struct
                MemcmpEncodedBytes::Base58(owner.to_string()),
            )),
        ];

        let obligations = self.program.rpc().get_program_accounts_with_config(
            &self.kamino_program_id,
            RpcProgramAccountsConfig {
                filters: Some(filters),
                account_config: solana_client::rpc_config::RpcAccountInfoConfig {
                    encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
                    ..Default::default()
                },
                ..Default::default()
            },
        )?;

        if obligations.is_empty() {
            debug!("No current obligations found for {}", owner_pubkey);
            return Ok(ret);
        }

        println!("\nObligations for {}:", owner_pubkey);
        for (pubkey, account) in obligations {
            let obligation: Obligation = Obligation::try_from_slice(&account.data[8..])?;

            println!("\nObligation address: {}", pubkey);
            println!(
                "Deposited value: {:.3}",
                Fraction::from_bits(obligation.deposited_value_sf).to_num::<f64>()
            );
            println!(
                "Borrowed value: {:.3}",
                Fraction::from_bits(obligation.borrowed_assets_market_value_sf).to_num::<f64>()
            );
            println!(
                "Allowed borrow value: {:.3}",
                Fraction::from_bits(obligation.allowed_borrow_value_sf).to_num::<f64>()
            );
            println!(
                "Unhealthy borrow value: {:.3}",
                Fraction::from_bits(obligation.unhealthy_borrow_value_sf).to_num::<f64>()
            );
            println!("Has debt: {}", obligation.has_debt != 0);
            println!("Borrowing disabled: {}", obligation.borrowing_disabled != 0);
            ret.push((pubkey, obligation));
        }

        Ok(ret)
    }
}
