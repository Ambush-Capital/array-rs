use crate::error::ErrorCode;
use crate::models::idl::accounts::{SpotMarket, User};
use crate::models::idl::types::{SpotBalanceType, SpotPosition};
use anchor_lang::AccountDeserialize;
use common::{
    asset_utils::get_symbol_for_mint,
    lending::{LendingClient, LendingError},
    ObligationType, UserObligation,
};
use common_rpc::{with_rpc_client, RpcError, RpcErrorConverter};
use log::debug;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

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

pub struct DriftClient {
    program_id: Pubkey,
    rpc_url: String,
    pub spot_markets: Vec<(Pubkey, SpotMarket)>,
}

impl Clone for DriftClient {
    fn clone(&self) -> Self {
        Self {
            program_id: self.program_id,
            rpc_url: self.rpc_url.clone(),
            spot_markets: self.spot_markets.clone(),
        }
    }
}

impl DriftClient {
    pub fn new(rpc_url: &str) -> Self {
        let program_id = Pubkey::from_str("dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH")
            .expect("Invalid Drift Program ID");

        Self { program_id, rpc_url: rpc_url.to_string(), spot_markets: Vec::new() }
    }

    /// Updates the client's state with the fetched market data
    pub fn set_market_data(&mut self, spot_markets: Vec<(Pubkey, SpotMarket)>) {
        self.spot_markets = spot_markets;
    }

    pub fn fetch_spot_markets(&self) -> Result<Vec<(Pubkey, SpotMarket)>, LendingError> {
        // Use the RPC builder with optimized filters
        let accounts = with_rpc_client(&self.rpc_url, |client| {
            common_rpc::SolanaRpcBuilder::new(client, self.program_id)
                .with_memcmp(0, DRIFT_SPOT_MARKET_DISCRIMINATOR.to_vec())
                .with_context(true)
                .optimize_filters() // Apply filter optimization
                .get_program_accounts_with_conversion::<LendingError, DriftErrorConverter>()
        })?;

        // Pre-allocate with capacity
        let mut spot_markets = Vec::with_capacity(accounts.len());

        // Process accounts
        for (pubkey, account) in accounts {
            if let Ok(market) = SpotMarket::try_deserialize(&mut &account.data[..]) {
                if market.is_active() {
                    spot_markets.push((pubkey, market));
                }
            }
        }

        Ok(spot_markets)
    }

    pub fn load_spot_markets(&mut self) -> Result<(), LendingError> {
        self.spot_markets = self.fetch_spot_markets()?;
        Ok(())
    }

    pub fn get_user_obligations(
        &self,
        owner_pubkey: &str,
    ) -> Result<Vec<UserObligation>, LendingError> {
        let spot_positions = self.fetch_raw_obligations(owner_pubkey)?;
        let mut obligations = Vec::new();

        // Cache protocol name to avoid repeated allocations
        let protocol_name = self.protocol_name().to_string();

        for position in spot_positions {
            let obligation_type = if position.balance_type == SpotBalanceType::Deposit {
                ObligationType::Asset
            } else {
                ObligationType::Liability
            };

            let (market_symbol, mint, mint_decimals, market_name, amount) = self
                .spot_markets
                .iter()
                .find(|(_, m)| m.market_index == position.market_index)
                .map(|(_, market)| {
                    let name = String::from_utf8_lossy(&market.name).trim().to_string();
                    let token_amount = match crate::models::spot_market::get_token_amount(
                        position.scaled_balance as u128,
                        market,
                        &position.balance_type,
                    ) {
                        Ok(amount) => amount as u64, // Convert back to u64 for UserObligation
                        Err(_) => position.scaled_balance, // Fallback to scaled_balance if conversion fails
                    };
                    (name.clone(), market.mint.to_string(), market.decimals, name, token_amount)
                })
                .unwrap_or_else(|| {
                    let default_mint = Pubkey::default().to_string();
                    (
                        "UNKNOWN".to_string(),
                        default_mint,
                        6, // Default to 6 decimals which is common for many tokens
                        format!("UNKNOWN-{}", position.market_index),
                        position.scaled_balance,
                    )
                });

            // Look up symbol from asset map, fallback to market_symbol
            let symbol = get_symbol_for_mint(&mint).unwrap_or(market_symbol);

            obligations.push(UserObligation {
                symbol,
                mint,
                mint_decimals,
                amount,
                protocol_name: protocol_name.clone(),
                market_name,
                obligation_type,
            });
        }

        Ok(obligations)
    }

    fn fetch_raw_obligations(&self, owner_pubkey: &str) -> Result<Vec<SpotPosition>, LendingError> {
        let owner = Pubkey::from_str(owner_pubkey)
            .map_err(|e| LendingError::InvalidAddress(e.to_string()))?;

        // Use the RPC builder with optimized filters
        let accounts = with_rpc_client(&self.rpc_url, |client| {
            common_rpc::SolanaRpcBuilder::new(client, self.program_id)
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

impl LendingClient<Pubkey, Vec<(Pubkey, SpotMarket)>> for DriftClient {
    fn load_markets(&mut self) -> Result<(), LendingError> {
        self.spot_markets = self.fetch_markets()?;
        Ok(())
    }

    fn fetch_markets(&self) -> Result<Vec<(Pubkey, SpotMarket)>, LendingError> {
        self.fetch_spot_markets()
    }

    fn set_market_data(&mut self, data: Vec<(Pubkey, SpotMarket)>) {
        self.spot_markets = data;
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
        "Drift"
    }
}

impl From<Box<dyn std::error::Error>> for ErrorCode {
    fn from(_: Box<dyn std::error::Error>) -> Self {
        ErrorCode::CastingFailure
    }
}
