use crate::common::rpc_utils::{
    format_pubkey_for_error, with_pooled_client, LendingErrorConverter,
};
use borsh::BorshDeserialize;
use common::{
    lending::{LendingClient, LendingError},
    ObligationType, UserObligation,
};
use common_rpc::SolanaRpcBuilder;
use log::info;
use solana_sdk::{account::Account, pubkey::Pubkey};
use std::{collections::HashMap, str::FromStr};

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

// Define discriminators as constants
const KAMINO_RESERVE_DISCRIMINATOR: [u8; 8] = [43, 242, 204, 202, 26, 247, 59, 127];
const KAMINO_OBLIGATION_DISCRIMINATOR: [u8; 8] = [168, 206, 141, 106, 88, 76, 172, 167];

pub struct KaminoClient {
    program_id: Pubkey,
    rpc_url: String,
    market_pubkeys: Vec<String>,
    pub markets: Vec<KaminoMarkets>,
    pub market_names: MarketNameMap,
}

impl Clone for KaminoClient {
    fn clone(&self) -> Self {
        Self {
            program_id: self.program_id,
            rpc_url: self.rpc_url.clone(),
            market_pubkeys: self.market_pubkeys.clone(),
            markets: self.markets.clone(),
            market_names: self.market_names.clone(),
        }
    }
}

impl KaminoClient {
    pub fn new(rpc_url: &str) -> Self {
        let program_id = Pubkey::from_str("KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD")
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

        Self {
            program_id,
            rpc_url: rpc_url.to_string(),
            market_pubkeys,
            markets: Vec::new(),
            market_names,
        }
    }

    /// Updates the client's state with the fetched market data
    pub fn set_market_data(&mut self, markets: Vec<KaminoMarkets>) {
        self.markets = markets;
    }

    pub fn fetch_kamino_markets_impl(&self) -> Result<Vec<KaminoMarkets>, LendingError> {
        let markets = self
            .market_pubkeys
            .iter()
            .filter_map(|pubkey_str| {
                let pubkey = match Pubkey::from_str(pubkey_str) {
                    Ok(pubkey) => pubkey,
                    Err(_) => return None,
                };

                // Get the lending market account
                let account_data = match with_pooled_client(&self.rpc_url, |client| {
                    SolanaRpcBuilder::new(client, self.program_id)
                        .get_account_with_conversion::<LendingError, LendingErrorConverter>(&pubkey)
                }) {
                    Ok(data) => data,
                    Err(_) => return None,
                };

                let lending_market = match LendingMarket::try_from_slice(&account_data.data[8..]) {
                    Ok(market) => market,
                    Err(_) => return None,
                };

                // Get the reserves for this market
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

        Ok(markets)
    }

    pub fn load_markets(&mut self) -> Result<(), LendingError> {
        self.markets = self.fetch_kamino_markets_impl()?;
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
        // Use the RPC builder with optimized filters
        with_pooled_client(&self.rpc_url, |client| {
            SolanaRpcBuilder::new(client, self.program_id)
                .with_data_size(RESERVE_SIZE as u64 + 8)
                .with_memcmp(0, KAMINO_RESERVE_DISCRIMINATOR.to_vec())
                .with_memcmp_base58(32, market_address.to_string())
                .optimize_filters() // Apply filter optimization
                .get_program_accounts_with_conversion::<LendingError, LendingErrorConverter>()
        })
    }

    pub fn get_user_obligations(
        &self,
        owner_pubkey: &str,
    ) -> Result<Vec<UserObligation>, LendingError> {
        let obligations = self.fetch_raw_obligations(owner_pubkey)?;
        info!("Found {} Kamino obligations", obligations.len());
        let mut user_obligations = Vec::new();

        // Cache protocol name to avoid repeated allocations
        let protocol_name = self.protocol_name().to_string();

        for (_, obligation) in obligations {
            // Get market name once per obligation
            let market_name = self
                .market_names
                .get(&obligation.lending_market.to_string())
                .unwrap_or(&"Unknown")
                .to_string();

            // Process deposits
            for deposit in obligation.deposits {
                let deposit_reserve_pubkey = Pubkey::from(deposit.deposit_reserve.to_bytes());
                if let Some(reserve) = self.get_reserve_by_pubkey(&deposit_reserve_pubkey)? {
                    // Get symbol and mint once
                    let symbol = reserve.token_symbol().to_string();
                    let mint = reserve.liquidity.mint_pubkey.to_string();

                    user_obligations.push(UserObligation {
                        symbol,
                        mint,
                        mint_decimals: reserve.liquidity.mint_decimals as u32,
                        amount: deposit.deposited_amount,
                        protocol_name: protocol_name.clone(),
                        market_name: market_name.clone(),
                        obligation_type: ObligationType::Asset,
                    });
                }
            }

            // Process borrows
            for borrow in obligation.borrows {
                let borrow_reserve_pubkey = Pubkey::from(borrow.borrow_reserve.to_bytes());
                if let Some(reserve) = self.get_reserve_by_pubkey(&borrow_reserve_pubkey)? {
                    // Get symbol and mint once
                    let symbol = reserve.token_symbol().to_string();
                    let mint = reserve.liquidity.mint_pubkey.to_string();

                    user_obligations.push(UserObligation {
                        symbol,
                        mint,
                        mint_decimals: reserve.liquidity.mint_decimals as u32,
                        amount: borrow.borrowed_amount_sf as u64,
                        protocol_name: protocol_name.clone(),
                        market_name: market_name.clone(),
                        obligation_type: ObligationType::Liability,
                    });
                }
            }
        }

        Ok(user_obligations)
    }

    fn fetch_raw_obligations(
        &self,
        owner_pubkey: &str,
    ) -> Result<Vec<(Pubkey, Obligation)>, LendingError> {
        let owner = Pubkey::from_str(owner_pubkey).map_err(|e| {
            LendingError::InvalidAddress(format!("Invalid owner pubkey {}: {}", owner_pubkey, e))
        })?;

        // Use the RPC builder with optimized filters
        let accounts = with_pooled_client(&self.rpc_url, |client| {
            SolanaRpcBuilder::new(client, self.program_id)
                .with_memcmp(0, KAMINO_OBLIGATION_DISCRIMINATOR.to_vec())
                .with_data_size(OBLIGATION_SIZE as u64 + 8)
                .with_memcmp_base58(8 + 8 + 16 + 32, owner.to_string())
                .optimize_filters() // Apply filter optimization
                .get_program_accounts_with_conversion::<LendingError, LendingErrorConverter>()
        })?;

        if accounts.is_empty() {
            debug!("No current obligations found for {}", owner_pubkey);
            return Ok(Vec::new());
        }

        // Pre-allocate with capacity
        let mut ret = Vec::with_capacity(accounts.len());

        for (pubkey, account) in accounts {
            match Obligation::try_from_slice(&account.data[8..]) {
                Ok(obligation) => ret.push((pubkey, obligation)),
                Err(e) => {
                    debug!(
                        "Failed to deserialize obligation {}: {}",
                        format_pubkey_for_error(&pubkey),
                        e
                    );
                    continue;
                }
            }
        }

        Ok(ret)
    }
}

impl LendingClient<Pubkey, Vec<KaminoMarkets>> for KaminoClient {
    fn load_markets(&mut self) -> Result<(), LendingError> {
        self.markets = self.fetch_kamino_markets_impl()?;
        Ok(())
    }

    fn fetch_markets(&self) -> Result<Vec<KaminoMarkets>, LendingError> {
        self.fetch_kamino_markets_impl()
    }

    fn set_market_data(&mut self, data: Vec<KaminoMarkets>) {
        self.markets = data;
    }

    fn get_user_obligations(
        &self,
        wallet_address: &str,
    ) -> Result<Vec<UserObligation>, LendingError> {
        self.get_user_obligations(wallet_address)
    }

    fn program_id(&self) -> Pubkey {
        self.program_id
    }

    fn protocol_name(&self) -> &'static str {
        "Kamino"
    }
}
