use crate::error::ErrorCode;
use crate::models::idl::accounts::{SpotMarket, User};
use crate::models::idl::types::{SpotBalanceType, SpotPosition};
use anchor_client::{Client, Program};
use anchor_lang::AccountDeserialize;
use common::{
    lending::{LendingClient, LendingError},
    ObligationType, UserObligation,
};
use log::debug;
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_sdk::{pubkey::Pubkey, signer::Signer};
use std::{ops::Deref, str::FromStr};

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
        let filters = vec![RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            0,
            [100, 177, 8, 107, 168, 65, 65, 39].to_vec(),
        ))];

        let account_config = RpcAccountInfoConfig {
            encoding: Some(solana_account_decoder::UiAccountEncoding::Base64Zstd),
            ..RpcAccountInfoConfig::default()
        };

        let gpa_config = RpcProgramAccountsConfig {
            filters: Some(filters),
            account_config,
            with_context: Some(true),
        };

        let accounts = self
            .program
            .rpc()
            .get_program_accounts_with_config(&self.drift_program_id, gpa_config)
            .map_err(|e| LendingError::RpcError(Box::new(e)))?;

        self.spot_markets = accounts
            .into_iter()
            .filter_map(|(pubkey, account)| {
                SpotMarket::try_deserialize(&mut &account.data[..])
                    .map(|market| (pubkey, market))
                    .ok()
                    .filter(|(_, market)| market.is_active())
            })
            .collect();

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

        let filters = vec![
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(8, owner.to_bytes().to_vec())),
            RpcFilterType::DataSize(std::mem::size_of::<User>() as u64 + 8),
        ];

        let accounts = self
            .program
            .rpc()
            .get_program_accounts_with_config(
                &self.drift_program_id,
                RpcProgramAccountsConfig {
                    filters: Some(filters),
                    account_config: RpcAccountInfoConfig {
                        encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .map_err(|e| LendingError::RpcError(Box::new(e)))?;

        if accounts.is_empty() {
            debug!("No Drift accounts found for {}", owner_pubkey);
            return Ok(Vec::new());
        }

        let mut result = Vec::new();
        for (_, account) in accounts {
            let user = User::try_deserialize(&mut &account.data[..])
                .map_err(|e| LendingError::DeserializationError(e.to_string()))?;

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
