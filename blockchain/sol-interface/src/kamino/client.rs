use anchor_client::{Client, Program};
use borsh::BorshDeserialize;
use common::{
    lending::{LendingClient, LendingError},
    ObligationType, UserObligation,
};
use log::info;
use solana_client::{
    rpc_config::RpcProgramAccountsConfig,
    rpc_filter::{self, Memcmp, MemcmpEncodedBytes},
};
use solana_sdk::{account::Account, pubkey::Pubkey, signer::Signer};
use std::{collections::HashMap, ops::Deref, str::FromStr};

use crate::kamino::{
    models::{lending_market::LendingMarket, reserve::Reserve},
    utils::consts::OBLIGATION_SIZE,
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

    pub fn load_markets(&mut self) -> Result<(), LendingError> {
        self.markets = self
            .market_pubkeys
            .iter()
            .filter_map(|pubkey_str| {
                let pubkey = match Pubkey::from_str(pubkey_str) {
                    Ok(pubkey) => pubkey,
                    Err(_) => return None,
                };

                let account_data = match self.program.rpc().get_account(&pubkey) {
                    Ok(data) => data,
                    Err(_) => return None,
                };

                let lending_market = match LendingMarket::try_from_slice(&account_data.data[8..]) {
                    Ok(market) => market,
                    Err(_) => return None,
                };

                let reserves = match self.get_reserves(&pubkey) {
                    Ok(reserves) => reserves,
                    Err(_) => return None,
                };

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

    fn get_reserve_by_pubkey(&self, pubkey: &Pubkey) -> Result<Option<&Reserve>, LendingError> {
        for (_, _, reserves) in &self.markets {
            if let Some((_, reserve)) =
                reserves.iter().find(|(reserve_pubkey, _)| reserve_pubkey == pubkey)
            {
                return Ok(Some(reserve));
            }
        }
        Ok(None)
    }

    fn get_reserves(
        &self,
        market_address: &Pubkey,
    ) -> Result<Vec<(Pubkey, Account)>, LendingError> {
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
            .map_err(|e| LendingError::RpcError(Box::new(e)))
    }

    pub fn get_user_obligations(
        &self,
        owner_pubkey: &str,
    ) -> Result<Vec<UserObligation>, LendingError> {
        let obligations = self.get_obligations(owner_pubkey)?;
        info!("Found {} Kamino obligations", obligations.len());
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
                        protocol_name: self.protocol_name().to_string(),
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
                        protocol_name: self.protocol_name().to_string(),
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
    ) -> Result<Vec<(Pubkey, Obligation)>, LendingError> {
        let owner = Pubkey::from_str(owner_pubkey)
            .map_err(|e| LendingError::InvalidAddress(e.to_string()))?;

        let filters = vec![
            rpc_filter::RpcFilterType::DataSize(OBLIGATION_SIZE as u64 + 8),
            rpc_filter::RpcFilterType::Memcmp(Memcmp::new(
                8 + 8 + 16 + 32,
                MemcmpEncodedBytes::Base58(owner.to_string()),
            )),
        ];

        let obligations = self
            .program
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
            .map_err(|e| LendingError::RpcError(Box::new(e)))?;

        if obligations.is_empty() {
            debug!("No current obligations found for {}", owner_pubkey);
            return Ok(Vec::new());
        }

        let mut ret = Vec::new();
        for (pubkey, account) in obligations {
            let obligation = Obligation::try_from_slice(&account.data[8..])
                .map_err(|e| LendingError::DeserializationError(e.to_string()))?;
            ret.push((pubkey, obligation));
        }

        Ok(ret)
    }
}

impl<C: Clone + Deref<Target = impl Signer>> LendingClient<Pubkey> for KaminoClient<C> {
    fn load_markets(&mut self) -> Result<(), LendingError> {
        self.load_markets()
    }

    fn get_user_obligations(
        &self,
        wallet_address: &str,
    ) -> Result<Vec<UserObligation>, LendingError> {
        self.get_user_obligations(wallet_address)
    }

    fn program_id(&self) -> Pubkey {
        self.kamino_program_id
    }

    fn protocol_name(&self) -> &'static str {
        "Kamino"
    }
}
