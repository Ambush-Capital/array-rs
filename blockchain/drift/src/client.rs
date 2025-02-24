use crate::error::ErrorCode;
use crate::models::idl::accounts::{SpotMarket, User};
use crate::models::idl::types::{SpotBalanceType, SpotPosition};
use anchor_client::{Client, Program};
use anchor_lang::AccountDeserialize;
use common::{
    lending::{LendingClient, LendingError},
    ObligationType, UserObligation,
};
use log::{debug, info};
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

        // Diagnostic check of all program accounts
        info!("Running diagnostic check of all program accounts...");

        // Search for accounts with specific mint
        let usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"; // Example: USDC mint
        let mint_pubkey = Pubkey::from_str(usdc_mint).unwrap();

        let mint_accounts = self
            .program
            .rpc()
            .get_program_accounts_with_config(
                &self.drift_program_id,
                RpcProgramAccountsConfig {
                    filters: Some(vec![
                        // Only check for specific mint - offset needs to account for fields before mint
                        RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                            8 + 32 + 32, // 8 (discriminator) + 32 (pubkey) + 32 (oracle)
                            mint_pubkey.to_bytes().to_vec(),
                        )),
                    ]),
                    account_config: RpcAccountInfoConfig {
                        encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .map_err(|e| LendingError::RpcError(Box::new(e)))?;

        info!(
            "Found {} accounts with mint {} (no discriminator check)",
            mint_accounts.len(),
            usdc_mint
        );

        // Examine matching accounts
        for (pubkey, account) in mint_accounts {
            info!("Account {} - Size: {} bytes", pubkey, account.data.len());

            // Try to get first 8 bytes as discriminator for debugging
            let discriminator = if account.data.len() >= 8 {
                let actual_discriminator = &account.data[0..8];
                // Known discriminator from our filter
                let expected_discriminator = [100, 177, 8, 107, 168, 65, 65, 39];

                info!("Discriminator comparison:");
                info!("  Expected: {:?}", expected_discriminator);
                info!("  Actual:   {:?}", actual_discriminator);
                info!("  Matches:  {}", actual_discriminator == expected_discriminator);

                format!("{:?}", actual_discriminator)
            } else {
                "account too small".to_string()
            };

            match SpotMarket::try_deserialize(&mut &account.data[..]) {
                Ok(market) => {
                    info!("Automatic deserialization succeeded:");
                    info!(
                        "  ✓ SpotMarket - Market Index: {}, Active: {}, Name: {}, Mint: {}",
                        market.market_index,
                        market.is_active(),
                        String::from_utf8_lossy(&market.name).trim_matches(char::from(0)),
                        market.mint
                    );
                }
                Err(e) => {
                    info!("Automatic deserialization failed: {:?}", e);

                    // Try to get more detailed error info if possible
                    match e {
                        anchor_lang::error::Error::AnchorError(err) => {
                            info!("Detailed Anchor error:");
                            info!("  Error code: {}", err.error_code_number);
                            info!("  Error name: {}", err.error_name);
                            info!("  Error msg: {}", err.error_msg);
                            if let Some(origin) = err.error_origin {
                                info!("  Error origin: {:?}", origin);
                            }
                            if let Some(compared) = err.compared_values {
                                info!("  Compared values: {:?}", compared);
                            }
                        }
                        _ => info!("Not an Anchor error: {:?}", e),
                    }
                }
            }

            // Size checks before manual deserialization
            info!("\nSize analysis:");
            info!("Account data size: {}", account.data.len());
            info!("Expected size from struct: {}", std::mem::size_of::<SpotMarket>() + 8);
            info!(
                "Size difference: {}",
                account.data.len() as i64 - (std::mem::size_of::<SpotMarket>() + 8) as i64
            );

            // Always do manual deserialization for comparison
            info!("\nPerforming manual deserialization for comparison:");
            if account.data.len() < 8 {
                info!("  Account data too small for discriminator");
                continue;
            }

            let data = &account.data[8..]; // Skip discriminator
            let mut offset = 0;

            // Basic pubkey fields
            info!("Basic fields:");

            macro_rules! try_deserialize_pubkey {
                ($field:expr) => {
                    if data.len() >= offset + 32 {
                        match Pubkey::try_from(&data[offset..offset + 32]) {
                            Ok(key) => {
                                info!("  ✓ {}: {}", $field, key);
                                offset += 32;
                            }
                            Err(e) => {
                                info!("  ✗ Failed to deserialize {}: {:?}", $field, e);
                                continue;
                            }
                        }
                    } else {
                        info!("  ✗ Not enough data for {}", $field);
                        continue;
                    }
                };
            }

            try_deserialize_pubkey!("pubkey");
            try_deserialize_pubkey!("oracle");
            try_deserialize_pubkey!("mint");
            try_deserialize_pubkey!("vault");

            // Name field [u8; 32]
            if data.len() >= offset + 32 {
                let name = String::from_utf8_lossy(&data[offset..offset + 32]);
                info!("  ✓ name: {}", name.trim_matches(char::from(0)));
                offset += 32;
            } else {
                info!("  ✗ Not enough data for name");
                continue;
            }

            // Define all deserialization macros
            macro_rules! try_deserialize_u128 {
                ($field:expr) => {
                    if data.len() >= offset + 16 {
                        let bytes = &data[offset..offset + 16];
                        let value = u128::from_le_bytes(bytes.try_into().unwrap());
                        info!("  ✓ {}: {}", $field, value);
                        offset += 16;
                    } else {
                        info!("  ✗ Not enough data for {}", $field);
                        continue;
                    }
                };
            }

            macro_rules! try_deserialize_u64 {
                ($field:expr) => {
                    if data.len() >= offset + 8 {
                        let bytes = &data[offset..offset + 8];
                        let value = u64::from_le_bytes(bytes.try_into().unwrap());
                        info!("  ✓ {}: {}", $field, value);
                        offset += 8;
                    } else {
                        info!("  ✗ Not enough data for {}", $field);
                        continue;
                    }
                };
            }

            macro_rules! try_deserialize_i64 {
                ($field:expr) => {
                    if data.len() >= offset + 8 {
                        let bytes = &data[offset..offset + 8];
                        let value = i64::from_le_bytes(bytes.try_into().unwrap());
                        info!("  ✓ {}: {}", $field, value);
                        offset += 8;
                    } else {
                        info!("  ✗ Not enough data for {}", $field);
                        continue;
                    }
                };
            }

            macro_rules! try_deserialize_u32 {
                ($field:expr) => {
                    if data.len() >= offset + 4 {
                        let bytes = &data[offset..offset + 4];
                        let value = u32::from_le_bytes(bytes.try_into().unwrap());
                        info!("  ✓ {}: {}", $field, value);
                        offset += 4;
                    } else {
                        info!("  ✗ Not enough data for {}", $field);
                        continue;
                    }
                };
            }

            macro_rules! try_deserialize_u16 {
                ($field:expr) => {
                    if data.len() >= offset + 2 {
                        let bytes = &data[offset..offset + 2];
                        let value = u16::from_le_bytes(bytes.try_into().unwrap());
                        info!("  ✓ {}: {}", $field, value);
                        offset += 2;
                    } else {
                        info!("  ✗ Not enough data for {}", $field);
                        continue;
                    }
                };
            }

            macro_rules! try_deserialize_i16 {
                ($field:expr) => {
                    if data.len() >= offset + 2 {
                        let bytes = &data[offset..offset + 2];
                        let value = i16::from_le_bytes(bytes.try_into().unwrap());
                        info!("  ✓ {}: {}", $field, value);
                        offset += 2;
                    } else {
                        info!("  ✗ Not enough data for {}", $field);
                        continue;
                    }
                };
            }

            macro_rules! try_deserialize_u8 {
                ($field:expr) => {
                    if data.len() >= offset + 1 {
                        let value = data[offset];
                        info!("  ✓ {}: {}", $field, value);
                        offset += 1;
                    } else {
                        info!("  ✗ Not enough data for {}", $field);
                        continue;
                    }
                };
            }

            // Start HistoricalOracleData
            info!("  --- Start HistoricalOracleData ---");
            try_deserialize_i64!("historical_oracle_data.last_oracle_price_twap");
            try_deserialize_i64!("historical_oracle_data.last_oracle_price_twap_5min");
            try_deserialize_i64!("historical_oracle_data.last_oracle_price");
            try_deserialize_i64!("historical_oracle_data.last_oracle_conf");
            try_deserialize_i64!("historical_oracle_data.last_oracle_delay");
            try_deserialize_i64!("historical_oracle_data.last_oracle_price_twap_ts");
            try_deserialize_i64!("historical_oracle_data.last_oracle_price_twap_5min_ts");
            try_deserialize_i64!("historical_oracle_data.last_oracle_price_ts");
            info!("  --- End HistoricalOracleData ---");

            // Start HistoricalIndexData
            info!("  --- Start HistoricalIndexData ---");
            try_deserialize_u64!("historical_index_data.last_index_bid_price");
            try_deserialize_u64!("historical_index_data.last_index_ask_price");
            try_deserialize_u64!("historical_index_data.last_index_price_twap");
            try_deserialize_u64!("historical_index_data.last_index_price_twap_5min");
            try_deserialize_u64!("historical_index_data.last_index_price_twap_ts");
            try_deserialize_u64!("historical_index_data.last_index_price_twap_5min_ts");
            info!("  --- End HistoricalIndexData ---");

            // Start PoolBalance (revenue_pool)
            info!("  --- Start revenue_pool (PoolBalance) ---");
            try_deserialize_u128!("revenue_pool.scaled_balance");
            try_deserialize_u128!("revenue_pool.market_index");
            try_deserialize_u8!("revenue_pool.balance_type");
            info!("  --- End revenue_pool ---");

            // Start PoolBalance (spot_fee_pool)
            info!("  --- Start spot_fee_pool (PoolBalance) ---");
            try_deserialize_u128!("spot_fee_pool.scaled_balance");
            try_deserialize_u128!("spot_fee_pool.market_index");
            try_deserialize_u8!("spot_fee_pool.balance_type");
            info!("  --- End spot_fee_pool ---");

            // Start InsuranceFund
            info!("  --- Start InsuranceFund ---");
            try_deserialize_u128!("insurance_fund.user_shares");
            try_deserialize_u128!("insurance_fund.total_shares");
            try_deserialize_u128!("insurance_fund.user_last_withdraw_request_shares");
            try_deserialize_u64!("insurance_fund.last_withdraw_request_value");
            try_deserialize_u64!("insurance_fund.last_withdraw_request_ts");
            try_deserialize_u64!("insurance_fund.revenue_settle_period_start");
            try_deserialize_u64!("insurance_fund.total_factor");
            try_deserialize_u64!("insurance_fund.user_factor");
            info!("  --- End InsuranceFund ---");

            // Try all remaining numeric fields in sequence
            try_deserialize_u128!("total_spot_fee");
            try_deserialize_u128!("deposit_balance");
            try_deserialize_u128!("borrow_balance");
            try_deserialize_u128!("cumulative_deposit_interest");
            try_deserialize_u128!("cumulative_borrow_interest");
            try_deserialize_u128!("total_social_loss");
            try_deserialize_u128!("total_quote_social_loss");

            try_deserialize_u64!("withdraw_guard_threshold");
            try_deserialize_u64!("max_token_deposits");
            try_deserialize_u64!("deposit_token_twap");
            try_deserialize_u64!("borrow_token_twap");
            try_deserialize_u64!("utilization_twap");
            try_deserialize_u64!("last_interest_ts");
            try_deserialize_u64!("last_twap_ts");
            try_deserialize_i64!("expiry_ts");
            try_deserialize_u64!("order_step_size");
            try_deserialize_u64!("order_tick_size");
            try_deserialize_u64!("min_order_size");
            try_deserialize_u64!("max_position_size");
            try_deserialize_u64!("next_fill_record_id");
            try_deserialize_u64!("next_deposit_record_id");

            try_deserialize_u32!("initial_asset_weight");
            try_deserialize_u32!("maintenance_asset_weight");
            try_deserialize_u32!("initial_liability_weight");
            try_deserialize_u32!("maintenance_liability_weight");
            try_deserialize_u32!("imf_factor");
            try_deserialize_u32!("liquidator_fee");
            try_deserialize_u32!("if_liquidation_fee");
            try_deserialize_u32!("optimal_utilization");
            try_deserialize_u32!("optimal_borrow_rate");
            try_deserialize_u32!("max_borrow_rate");
            try_deserialize_u32!("decimals");

            try_deserialize_u16!("market_index");
            try_deserialize_u8!("orders_enabled"); // bool is u8
            try_deserialize_u8!("oracle_source"); // enum is u8
            try_deserialize_u8!("status"); // enum is u8
            try_deserialize_u8!("asset_tier"); // enum is u8
            try_deserialize_u8!("paused_operations");
            try_deserialize_u8!("if_paused_operations");
            try_deserialize_i16!("fee_adjustment");
            try_deserialize_u16!("max_token_borrows_fraction");
            try_deserialize_u64!("flash_loan_amount");
            try_deserialize_u64!("flash_loan_initial_token_amount");
            try_deserialize_u64!("total_swap_fee");
            try_deserialize_u64!("scale_initial_asset_weight_start");
            try_deserialize_u8!("min_borrow_rate");
            try_deserialize_u8!("fuel_boost_deposits");
            try_deserialize_u8!("fuel_boost_borrows");
            try_deserialize_u8!("fuel_boost_taker");
            try_deserialize_u8!("fuel_boost_maker");
            try_deserialize_u8!("fuel_boost_insurance");
            try_deserialize_u8!("token_program");
            try_deserialize_u8!("pool_id");

            // Check padding explicitly
            info!("Checking padding...");
            let expected_padding_size = 40;
            let actual_padding_size = data.len() - offset;
            info!(
                "Padding size - Expected: {}, Actual: {}, Difference: {}",
                expected_padding_size,
                actual_padding_size,
                expected_padding_size as i64 - actual_padding_size as i64
            );

            if actual_padding_size < expected_padding_size {
                info!("⚠️  Warning: Account has less padding than expected!");
                info!("This might cause deserialization issues with anchor's AccountDeserialize");
            } else if actual_padding_size > expected_padding_size {
                info!("⚠️  Warning: Account has more padding than expected!");
            }

            // Show padding bytes
            if data.len() - offset > 0 {
                let padding_bytes = &data[offset..];
                info!("Padding bytes: {:?}", padding_bytes);

                // Check if padding bytes are all zeros
                let all_zeros = padding_bytes.iter().all(|&x| x == 0);
                info!("All padding bytes are zeros: {}", all_zeros);

                if !all_zeros {
                    info!("⚠️  Warning: Some padding bytes are non-zero!");
                    // Print positions of non-zero bytes
                    for (i, &byte) in padding_bytes.iter().enumerate() {
                        if byte != 0 {
                            info!("  Non-zero byte at position {}: {}", i, byte);
                        }
                    }
                }
            }

            // At the end, after padding check, add comparison if automatic deserialization succeeded
            if let Ok(market) = SpotMarket::try_deserialize(&mut &account.data[..]) {
                info!("\nComparing automatic vs manual deserialization:");
                info!("Total account data length: {}", account.data.len());
                info!("Manual deserialization ended at offset: {}", offset);
                info!("Remaining bytes after manual deserialization: {}", data.len() - offset);
                info!("Expected total size from struct: {}", std::mem::size_of::<SpotMarket>() + 8);
                // +8 for discriminator
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

        info!("Found {} Drift obligations", spot_positions.len());

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
