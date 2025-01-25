use anchor_client::{Client, Program};
use borsh::BorshDeserialize;
use prettytable::{row, Table};
use solana_client::{
    rpc_config::RpcProgramAccountsConfig,
    rpc_filter::{self, Memcmp, MemcmpEncodedBytes},
};
use solana_sdk::{account::Account, pubkey::Pubkey, signer::Signer};
use std::{ops::Deref, str::FromStr};

use crate::kamino::{models::obligation::Obligation, utils::consts::RESERVE_SIZE};
use crate::kamino::{
    models::{lending_market::LendingMarket, reserve::Reserve},
    utils::consts::OBLIGATION_SIZE,
    utils::fraction::Fraction,
};

type KaminoMarkets = (Pubkey, LendingMarket, Vec<(Pubkey, Reserve)>);

pub struct KaminoClient<C> {
    program: Program<C>,
    kamino_program_id: Pubkey,
    market_pubkeys: Vec<String>,
    pub markets: Vec<KaminoMarkets>,
}

impl<C: Clone + Deref<Target = impl Signer>> KaminoClient<C> {
    pub fn new(client: &Client<C>) -> Self {
        let kamino_program_id = Pubkey::from_str("KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD")
            .expect("Invalid Kamino Lending Program ID");

        let market_pubkeys = vec![
            "7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF".to_string(), //main market
            "H6rHXmXoCQvq8Ue81MqNh7ow5ysPa1dSozwW3PU1dDH6".to_string(), //JITO Market
            "DxXdAyU3kCjnyggvHmY5nAwg5cRbbmdyX3npfDMjjMek".to_string(), //JLP Market
            "ByYiZxp8QrdN9qbdtaAiePN8AAr3qvTPppNJDpf5DVJ5".to_string(), //Altcoin market
            "BJnbcRHqvppTyGesLzWASGKnmnF1wq9jZu6ExrjT7wvF".to_string(), //Ethena market
        ];

        let program = client.program(kamino_program_id).expect("Failed to load Kamino program");

        Self { program, kamino_program_id, market_pubkeys, markets: Vec::new() }
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

        for (_, lending_market, reserves) in &self.markets {
            for (pubkey, reserve) in reserves {
                table.add_row(row![
                    String::from_utf8_lossy(&lending_market.name).trim_matches('\0'),
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
                    format!("{:.1}%", reserve.current_supply_apy().to_num::<f64>() * 100.0),
                    format!("{:.1}%", reserve.current_borrow_apy().to_num::<f64>() * 100.0),
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

    pub fn get_obligations(&self, owner_pubkey: &str) -> Result<(), Box<dyn std::error::Error>> {
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
            println!("No current obligations found for {}", owner_pubkey);
            return Ok(());
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
        }

        Ok(())
    }
}
