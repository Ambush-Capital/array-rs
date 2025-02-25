use crate::common::rpc_utils::{
    format_pubkey_for_error, with_pooled_client, LendingErrorConverter,
};
use anchor_client::{Client, Program};
use borsh::BorshDeserialize;
use common::{
    lending::{LendingClient, LendingError},
    ObligationType, UserObligation,
};
use common_rpc::SolanaRpcBuilder;
use log::info;
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

// Define discriminators as constants
const KAMINO_RESERVE_DISCRIMINATOR: [u8; 8] = [43, 242, 204, 202, 26, 247, 59, 127];
const KAMINO_OBLIGATION_DISCRIMINATOR: [u8; 8] = [168, 206, 141, 106, 88, 76, 172, 167];

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

    /// Get the RPC URL from the program
    fn get_rpc_url(&self) -> String {
        self.program.rpc().url().to_string()
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

                // Get the RPC URL
                let rpc_url = self.get_rpc_url();

                // Get the lending market account
                let account_data = match with_pooled_client(&rpc_url, |client| {
                    SolanaRpcBuilder::new(client, self.kamino_program_id)
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
        // Get the RPC URL
        let rpc_url = self.get_rpc_url();

        // Use the RPC builder with optimized filters
        with_pooled_client(&rpc_url, |client| {
            SolanaRpcBuilder::new(client, self.kamino_program_id)
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
                let deposit_reserve_pubkey = Pubkey::from(deposit.deposit_reserve.to_bytes());
                if let Some(reserve) = self.get_reserve_by_pubkey(&deposit_reserve_pubkey)? {
                    user_obligations.push(UserObligation {
                        symbol: reserve.token_symbol().to_string(),
                        mint: reserve.liquidity.mint_pubkey.to_string(),
                        mint_decimals: reserve.liquidity.mint_decimals as u32,
                        amount: deposit.deposited_amount,
                        protocol_name: self.protocol_name().to_string(),
                        market_name: market_name.clone(),
                        obligation_type: ObligationType::Asset,
                    });
                } else {
                    debug!(
                        "Reserve not found for deposit: {}",
                        format_pubkey_for_error(&deposit_reserve_pubkey)
                    );
                }
            }

            // Process borrows
            for borrow in obligation.borrows {
                let borrow_reserve_pubkey = Pubkey::from(borrow.borrow_reserve.to_bytes());
                if let Some(reserve) = self.get_reserve_by_pubkey(&borrow_reserve_pubkey)? {
                    user_obligations.push(UserObligation {
                        symbol: reserve.token_symbol().to_string(),
                        mint: reserve.liquidity.mint_pubkey.to_string(),
                        mint_decimals: reserve.liquidity.mint_decimals as u32,
                        amount: borrow.borrowed_amount_sf as u64,
                        protocol_name: self.protocol_name().to_string(),
                        market_name: market_name.clone(),
                        obligation_type: ObligationType::Liability,
                    });
                } else {
                    debug!(
                        "Reserve not found for borrow: {}",
                        format_pubkey_for_error(&borrow_reserve_pubkey)
                    );
                }
            }
        }

        Ok(user_obligations)
    }

    fn get_obligations(
        &self,
        owner_pubkey: &str,
    ) -> Result<Vec<(Pubkey, Obligation)>, LendingError> {
        let owner = Pubkey::from_str(owner_pubkey).map_err(|e| {
            LendingError::InvalidAddress(format!("Invalid owner pubkey {}: {}", owner_pubkey, e))
        })?;

        // Get the RPC URL
        let rpc_url = self.get_rpc_url();

        // Use the RPC builder with optimized filters
        let accounts = with_pooled_client(&rpc_url, |client| {
            SolanaRpcBuilder::new(client, self.kamino_program_id)
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
