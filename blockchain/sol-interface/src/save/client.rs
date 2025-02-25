use crate::common::rpc_utils::{
    format_pubkey_for_error, with_pooled_client, LendingErrorConverter,
};
use crate::save::models::{Obligation, Reserve};
use common::{
    lending::{LendingClient, LendingError},
    ObligationType, UserObligation,
};
use common_rpc::SolanaRpcBuilder;
use log::debug;
use solana_program::program_pack::Pack;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SolendPool {
    pub name: String,
    pub pubkey: Pubkey,
    pub reserves: Vec<Reserve>,
}

pub struct SaveClient {
    pub program_id: Pubkey,
    pub rpc_url: String,
    pub pools: Vec<SolendPool>,
}

impl Clone for SaveClient {
    fn clone(&self) -> Self {
        Self {
            program_id: self.program_id,
            rpc_url: self.rpc_url.clone(),
            pools: self.pools.clone(),
        }
    }
}

impl SaveClient {
    pub fn new(rpc_url: &str) -> Self {
        let program_id = Pubkey::from_str("So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo")
            .expect("Invalid Solend Program ID");

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

        Self { program_id, rpc_url: rpc_url.to_string(), pools }
    }

    /// Updates the client's state with the fetched market data
    pub fn set_market_data(&mut self, reserves_data: Vec<(Pubkey, Vec<Reserve>)>) {
        for (pubkey, reserves) in reserves_data {
            if let Some(pool) = self.pools.iter_mut().find(|p| p.pubkey == pubkey) {
                pool.reserves = reserves;
            }
        }
    }

    pub fn load_reserves_for_pool(&self, pool: &SolendPool) -> Result<Vec<Reserve>, LendingError> {
        // Use the RPC builder with optimized filters
        let reserves = with_pooled_client(&self.rpc_url, |client| {
            SolanaRpcBuilder::new(client, self.program_id)
                .with_data_size(Reserve::LEN as u64)
                .with_memcmp_base58(10, pool.pubkey.to_string())
                .optimize_filters() // Apply filter optimization
                .get_program_accounts_with_conversion::<LendingError, LendingErrorConverter>()
        })?;

        let reserves = reserves
            .into_iter()
            .filter_map(|(pubkey, account)| match Reserve::unpack(&account.data) {
                Ok(reserve) => Some(reserve),
                Err(e) => {
                    debug!("Failed to unpack reserve {}: {}", format_pubkey_for_error(&pubkey), e);
                    None
                }
            })
            .collect();

        Ok(reserves)
    }

    pub fn fetch_reserves(&self) -> Result<Vec<(Pubkey, Vec<Reserve>)>, LendingError> {
        // Create a vector to hold all the reserves we'll load
        let mut all_reserves = Vec::with_capacity(self.pools.len());

        // Load all reserves without modifying self.pools
        for pool in &self.pools {
            let reserves = self.load_reserves_for_pool(pool)?;
            all_reserves.push((pool.pubkey, reserves));
        }

        Ok(all_reserves)
    }

    pub fn load_reserves(&mut self) -> Result<(), LendingError> {
        // Fetch reserves using the new method
        let all_reserves = self.fetch_reserves()?;

        // Update the pools with the loaded reserves
        for pool in &mut self.pools {
            if let Some((_, reserves)) =
                all_reserves.iter().find(|(pubkey, _)| *pubkey == pool.pubkey)
            {
                pool.reserves = reserves.clone();
            }
        }

        Ok(())
    }

    pub fn get_user_obligations(
        &self,
        owner_pubkey: &str,
    ) -> Result<Vec<UserObligation>, LendingError> {
        let obligations = self.fetch_raw_obligations(owner_pubkey)?;

        // Pre-allocate with estimated capacity (deposits + borrows per obligation)
        let estimated_capacity =
            obligations.iter().map(|obl| obl.deposits.len() + obl.borrows.len()).sum();
        let mut user_obligations = Vec::with_capacity(estimated_capacity);

        // Collect all reserve pubkeys first
        let mut reserve_pubkeys = Vec::with_capacity(estimated_capacity);

        for obligation in &obligations {
            // Add deposit reserves
            for deposit in &obligation.deposits {
                reserve_pubkeys.push(Pubkey::new_from_array(deposit.deposit_reserve.to_bytes()));
            }

            // Add borrow reserves
            for borrow in &obligation.borrows {
                reserve_pubkeys.push(Pubkey::new_from_array(borrow.borrow_reserve.to_bytes()));
            }
        }

        // Fetch all reserves in a single batch operation
        let reserves = with_pooled_client(&self.rpc_url, |client| {
            common_rpc::get_multiple_accounts_with_conversion::<LendingError, LendingErrorConverter>(
                client,
                &reserve_pubkeys,
            )
        })?;

        // Cache protocol name to avoid repeated allocations
        let protocol_name = self.protocol_name().to_string();

        // Default market name
        let default_market_name = "Main Pool".to_string();

        // Now process obligations with all reserve data available
        for obligation in obligations {
            // Process deposits
            for deposit in obligation.deposits {
                let deposit_reserve_pubkey =
                    Pubkey::new_from_array(deposit.deposit_reserve.to_bytes());

                if let Some(reserve_account) = reserves.get(&deposit_reserve_pubkey) {
                    let reserve = Reserve::unpack(&reserve_account.data).map_err(|e| {
                        LendingError::DeserializationError(format!(
                            "Failed to unpack reserve {}: {}",
                            format_pubkey_for_error(&deposit_reserve_pubkey),
                            e
                        ))
                    })?;

                    let exchange_rate = reserve.collateral_exchange_rate().map_err(|e| {
                        LendingError::ProtocolError(format!(
                            "Failed to get collateral exchange rate for reserve {}: {}",
                            format_pubkey_for_error(&deposit_reserve_pubkey),
                            e
                        ))
                    })?;

                    let amount = exchange_rate
                        .collateral_to_liquidity(deposit.deposited_amount)
                        .unwrap_or(0);

                    // Use the mint pubkey once and reuse it for both symbol and mint
                    let mint_str = reserve.liquidity.mint_pubkey.to_string();

                    user_obligations.push(UserObligation {
                        symbol: mint_str.clone(), // Clone only once
                        mint: mint_str,
                        mint_decimals: reserve.liquidity.mint_decimals as u32,
                        amount,
                        protocol_name: protocol_name.clone(), // Clone the cached value
                        market_name: default_market_name.clone(),
                        obligation_type: ObligationType::Asset,
                    });
                }
            }

            // Process borrows
            for borrow in obligation.borrows {
                let borrow_reserve_pubkey =
                    Pubkey::new_from_array(borrow.borrow_reserve.to_bytes());

                if let Some(reserve_account) = reserves.get(&borrow_reserve_pubkey) {
                    let reserve = Reserve::unpack(&reserve_account.data).map_err(|e| {
                        LendingError::DeserializationError(format!(
                            "Failed to unpack reserve {}: {}",
                            format_pubkey_for_error(&borrow_reserve_pubkey),
                            e
                        ))
                    })?;

                    // Use the mint pubkey once and reuse it for both symbol and mint
                    let mint_str = reserve.liquidity.mint_pubkey.to_string();

                    user_obligations.push(UserObligation {
                        symbol: mint_str.clone(), // Clone only once
                        mint: mint_str,
                        mint_decimals: reserve.liquidity.mint_decimals as u32,
                        amount: borrow.borrowed_amount_wads.try_round_u64().unwrap_or(0),
                        protocol_name: protocol_name.clone(), // Clone the cached value
                        market_name: default_market_name.clone(),
                        obligation_type: ObligationType::Liability,
                    });
                }
            }
        }

        Ok(user_obligations)
    }

    fn fetch_raw_obligations(&self, owner_pubkey: &str) -> Result<Vec<Obligation>, LendingError> {
        let mut ret = Vec::new();
        let owner = owner_pubkey.parse::<Pubkey>().map_err(|e| {
            LendingError::InvalidAddress(format!("Invalid owner pubkey {}: {}", owner_pubkey, e))
        })?;

        // Use the RPC builder with optimized filters
        let accounts = with_pooled_client(&self.rpc_url, |client| {
            SolanaRpcBuilder::new(client, self.program_id)
                .with_data_size(Obligation::LEN as u64)
                .with_memcmp(1 + 8 + 1 + 32, owner.to_bytes().to_vec()) // Skip version(1) + last_update(8+1) + lending_market(32) to get to owner
                .optimize_filters() // Apply filter optimization
                .get_program_accounts_with_conversion::<LendingError, LendingErrorConverter>()
        })?;

        if accounts.is_empty() {
            debug!("No current obligations found for {}", owner_pubkey);
            return Ok(ret);
        }

        for (pubkey, account) in accounts {
            match Obligation::unpack(&account.data) {
                Ok(obligation) => {
                    if obligation.owner.to_string() != owner.to_string() {
                        debug!("Obligation owner mismatch: {} != {}", obligation.owner, owner);
                        continue;
                    }

                    if obligation.deposits.is_empty() && obligation.borrows.is_empty() {
                        debug!("Skipping empty obligation {}", format_pubkey_for_error(&pubkey));
                        continue;
                    }

                    ret.push(obligation);
                }
                Err(e) => {
                    debug!(
                        "Failed to unpack obligation {}: {}",
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

impl LendingClient<Pubkey, Vec<(Pubkey, Vec<Reserve>)>> for SaveClient {
    fn load_markets(&mut self) -> Result<(), LendingError> {
        self.load_reserves()
    }

    fn fetch_markets(&self) -> Result<Vec<(Pubkey, Vec<Reserve>)>, LendingError> {
        self.fetch_reserves()
    }

    fn set_market_data(&mut self, data: Vec<(Pubkey, Vec<Reserve>)>) {
        // Update the client's state with the fetched market data
        for (pubkey, reserves) in data {
            if let Some(pool) = self.pools.iter_mut().find(|p| p.pubkey == pubkey) {
                pool.reserves = reserves;
            }
        }
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
        "Solend"
    }
}
