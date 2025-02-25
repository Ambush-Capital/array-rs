use crate::common::rpc_utils::{
    format_pubkey_for_error, with_pooled_client, LendingErrorConverter,
};
use crate::save::models::{Obligation, Reserve};
use anchor_client::{Client, Program};
use common::{
    lending::{LendingClient, LendingError},
    ObligationType, UserObligation,
};
use common_rpc::SolanaRpcBuilder;
use log::debug;
use solana_program::program_pack::Pack;
use solana_sdk::{pubkey::Pubkey, signature::Signer};
use std::ops::Deref;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SolendPool {
    pub name: String,
    pub pubkey: Pubkey,
    pub reserves: Vec<Reserve>,
}

pub struct SaveClient<C> {
    pub program: Program<C>,
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

    /// Get the RPC URL from the program
    fn get_rpc_url(&self) -> String {
        self.program.rpc().url().to_string()
    }

    pub fn load_reserves_for_pool(
        program: &Program<C>,
        pool: &SolendPool,
    ) -> Result<Vec<Reserve>, LendingError> {
        // Get the RPC URL
        let rpc_url = program.rpc().url().to_string();

        // Use the RPC builder with optimized filters
        let accounts = with_pooled_client(&rpc_url, |client| {
            SolanaRpcBuilder::new(client, program.id())
                .with_data_size(Reserve::LEN as u64)
                .with_memcmp_base58(10, pool.pubkey.to_string())
                .optimize_filters() // Apply filter optimization
                .get_program_accounts_with_conversion::<LendingError, LendingErrorConverter>()
        })?;

        let reserves = accounts
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

    pub fn load_all_reserves(&mut self) -> Result<(), LendingError> {
        for pool in &mut self.pools {
            let reserves = SaveClient::<C>::load_reserves_for_pool(&self.program, pool)?;
            pool.reserves = reserves;
        }
        Ok(())
    }

    pub fn get_user_obligations(
        &self,
        owner_pubkey: &str,
    ) -> Result<Vec<UserObligation>, LendingError> {
        let obligations = self.get_obligations(owner_pubkey)?;

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
        let rpc_url = self.get_rpc_url();
        let reserves = with_pooled_client(&rpc_url, |client| {
            common_rpc::get_multiple_accounts_with_conversion::<LendingError, LendingErrorConverter>(
                client,
                &reserve_pubkeys,
            )
        })?;

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

                    user_obligations.push(UserObligation {
                        symbol: reserve.liquidity.mint_pubkey.to_string(),
                        mint: reserve.liquidity.mint_pubkey.to_string(),
                        mint_decimals: reserve.liquidity.mint_decimals as u32,
                        amount,
                        protocol_name: self.protocol_name().to_string(),
                        market_name: "Main Pool".to_string(), // Could be improved by looking up actual pool name
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

                    user_obligations.push(UserObligation {
                        symbol: reserve.liquidity.mint_pubkey.to_string(),
                        mint: reserve.liquidity.mint_pubkey.to_string(),
                        mint_decimals: reserve.liquidity.mint_decimals as u32,
                        amount: borrow.borrowed_amount_wads.try_round_u64().unwrap_or(0),
                        protocol_name: self.protocol_name().to_string(),
                        market_name: "Main Pool".to_string(), // Could be improved by looking up actual pool name
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

    fn get_obligations(&self, owner_pubkey: &str) -> Result<Vec<Obligation>, LendingError> {
        let mut ret = Vec::new();
        let owner = owner_pubkey.parse::<Pubkey>().map_err(|e| {
            LendingError::InvalidAddress(format!("Invalid owner pubkey {}: {}", owner_pubkey, e))
        })?;

        // Get the RPC URL
        let rpc_url = self.get_rpc_url();

        // Use the RPC builder with optimized filters
        let accounts = with_pooled_client(&rpc_url, |client| {
            SolanaRpcBuilder::new(client, self.program.id())
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

impl<C: Clone + Deref<Target = impl Signer>> LendingClient<Pubkey> for SaveClient<C> {
    fn load_markets(&mut self) -> Result<(), LendingError> {
        self.load_all_reserves()
    }

    fn get_user_obligations(
        &self,
        wallet_address: &str,
    ) -> Result<Vec<UserObligation>, LendingError> {
        self.get_user_obligations(wallet_address)
    }

    fn program_id(&self) -> Pubkey {
        self.program.id()
    }

    fn protocol_name(&self) -> &'static str {
        "Solend"
    }
}
