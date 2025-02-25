use crate::error::ErrorCode;
use crate::models::idl::accounts::{SpotMarket, User};
use crate::models::idl::types::{SpotBalanceType, SpotPosition};
use anchor_client::{Client, Program};
use anchor_lang::AccountDeserialize;
use common::{
    lending::{LendingClient, LendingError},
    ObligationType, UserObligation,
};
use common_rpc::{with_rpc_client, RpcError, RpcErrorConverter};
use log::debug;
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use std::{ops::Deref, str::FromStr};

// Define discriminators as constants
const DRIFT_SPOT_MARKET_DISCRIMINATOR: [u8; 8] = [100, 177, 8, 107, 168, 65, 65, 39];
const DRIFT_USER_DISCRIMINATOR: [u8; 8] = [159, 117, 95, 227, 239, 151, 58, 236]; // Correct User discriminator

// Implement the RpcErrorConverter trait for LendingError
struct DriftErrorConverter;

impl RpcErrorConverter<LendingError> for DriftErrorConverter {
    fn convert_error(error: RpcError) -> LendingError {
        match error {
            RpcError::RpcError(e) => LendingError::RpcError(e),
            RpcError::DeserializationError(e) => LendingError::DeserializationError(e),
            RpcError::InvalidAddress(e) => LendingError::InvalidAddress(e),
            RpcError::AccountNotFound(e) => LendingError::AccountNotFound(e),
        }
    }
}

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

    pub fn load_spot_markets(&mut self) -> Result<(), LendingError> {
        // Get the RPC URL from the program
        let rpc_url = self.program.rpc().url().to_string();

        // Use the RPC builder with optimized filters
        let accounts = with_rpc_client(&rpc_url, |client| {
            common_rpc::SolanaRpcBuilder::new(client, self.drift_program_id)
                .with_memcmp(0, DRIFT_SPOT_MARKET_DISCRIMINATOR.to_vec())
                .with_context(true)
                .optimize_filters() // Apply filter optimization
                .get_program_accounts_with_conversion::<LendingError, DriftErrorConverter>()
        })?;

        // Pre-allocate with capacity
        self.spot_markets = Vec::with_capacity(accounts.len());

        // Process accounts
        for (pubkey, account) in accounts {
            if let Ok(market) = SpotMarket::try_deserialize(&mut &account.data[..]) {
                if market.is_active() {
                    self.spot_markets.push((pubkey, market));
                }
            }
        }

        Ok(())
    }

    pub fn get_user_obligations(
        &self,
        owner_pubkey: &str,
    ) -> Result<Vec<UserObligation>, LendingError> {
        let spot_positions = self.get_obligations(owner_pubkey)?;
        let mut obligations = Vec::new();

        for position in spot_positions {
            let obligation_type = if position.balance_type == SpotBalanceType::Deposit {
                ObligationType::Asset
            } else {
                ObligationType::Liability
            };

            let (symbol, mint, mint_decimals, market_name) = self
                .spot_markets
                .iter()
                .find(|(_, m)| m.market_index == position.market_index)
                .map(|(_, market)| {
                    let name = String::from_utf8_lossy(&market.name)
                        .trim_matches(char::from(0))
                        .to_string();
                    (name.clone(), market.mint, market.decimals, name)
                })
                .unwrap_or_else(|| {
                    let default_mint = Pubkey::default();
                    (
                        "UNKNOWN".to_string(),
                        default_mint,
                        6, // Default to 6 decimals which is common for many tokens
                        format!("UNKNOWN-{}", position.market_index),
                    )
                });

            obligations.push(UserObligation {
                symbol,
                mint: mint.to_string(),
                mint_decimals,
                amount: position.scaled_balance,
                protocol_name: self.protocol_name().to_string(),
                market_name,
                obligation_type,
            });
        }

        Ok(obligations)
    }

    fn get_obligations(&self, owner_pubkey: &str) -> Result<Vec<SpotPosition>, LendingError> {
        let owner = Pubkey::from_str(owner_pubkey)
            .map_err(|e| LendingError::InvalidAddress(e.to_string()))?;

        // Get the RPC URL from the program
        let rpc_url = self.program.rpc().url().to_string();

        // Use the RPC builder with optimized filters
        let accounts = with_rpc_client(&rpc_url, |client| {
            common_rpc::SolanaRpcBuilder::new(client, self.drift_program_id)
                .with_memcmp(0, DRIFT_USER_DISCRIMINATOR.to_vec())
                .with_memcmp_pubkey(8, &owner)
                .with_data_size(std::mem::size_of::<User>() as u64 + 8)
                .optimize_filters() // Apply filter optimization
                .get_program_accounts_with_conversion::<LendingError, DriftErrorConverter>()
        })?;

        if accounts.is_empty() {
            debug!("No Drift accounts found for {}", owner_pubkey);
            return Ok(Vec::new());
        }

        // Pre-allocate with estimated capacity
        let mut result = Vec::with_capacity(accounts.len() * 5); // Estimate 5 positions per account

        for (_pubkey, account) in accounts {
            let user = User::try_deserialize(&mut &account.data[..])
                .map_err(|e| LendingError::DeserializationError(e.to_string()))?;

            // Filter and extend in one operation to avoid intermediate allocations
            result.extend(user.spot_positions.iter().filter(|p| p.scaled_balance > 0).copied());
        }

        Ok(result)
    }
}

impl<C: Clone + Deref<Target = impl Signer>> LendingClient<Pubkey> for DriftClient<C> {
    fn load_markets(&mut self) -> Result<(), LendingError> {
        self.load_spot_markets()
    }

    fn get_user_obligations(
        &self,
        wallet_address: &str,
    ) -> Result<Vec<UserObligation>, LendingError> {
        self.get_user_obligations(wallet_address)
    }

    fn program_id(&self) -> Pubkey {
        self.drift_program_id
    }

    fn protocol_name(&self) -> &'static str {
        "Drift"
    }
}

impl From<Box<dyn std::error::Error>> for ErrorCode {
    fn from(_: Box<dyn std::error::Error>) -> Self {
        ErrorCode::CastingFailure
    }
}
