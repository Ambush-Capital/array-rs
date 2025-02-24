use std::{ops::Deref, str::FromStr};

use crate::{
    aggregator::from::{
        DriftReserveWrapper, KaminoReserveWrapper, MarginfiReserveWrapper, SaveReserveWrapper,
    },
    kamino::{client::KaminoClient, utils::errors::LendingError as KaminoLendingError},
    marginfi::client::MarginfiClient,
    save::{client::SaveClient, error::LendingError},
};
use anchor_client::Client;
use common::{LendingReserve, MintAsset, ObligationType, UserObligation};
use drift::{client::DriftClient, error::ErrorCode};
use log::info;
use solana_program::program_error::ProgramError;
use solana_sdk::{pubkey::Pubkey, signature::Signer};

pub struct LendingMarketAggregator<C> {
    pub assets: Vec<MintAsset>,
    // metadata_cache: HashMap<Pubkey, (String, String)>,
    save_client: SaveClient<C>,
    marginfi_client: MarginfiClient<C>,
    kamino_client: KaminoClient<C>,
    drift_client: DriftClient<C>,
}

impl<C: Clone + Deref<Target = impl Signer>> Default for LendingMarketAggregator<C> {
    fn default() -> Self {
        unimplemented!("Default implementation not available - use new() instead")
    }
}

impl<C: Clone + Deref<Target = impl Signer>> LendingMarketAggregator<C> {
    pub fn new(client: &Client<C>) -> Self {
        Self {
            assets: Self::get_valid_assets(),
            // metadata_cache: HashMap::new(),
            save_client: SaveClient::new(client),
            marginfi_client: MarginfiClient::new(client),
            kamino_client: KaminoClient::new(client),
            drift_client: DriftClient::new(client),
        }
    }

    fn get_valid_assets() -> Vec<MintAsset> {
        vec![
            MintAsset {
                name: "USDC".to_string(),
                symbol: "USD Coin".to_string(),
                market_price_sf: 0,
                mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".parse().unwrap(),
                lending_reserves: vec![],
            },
            // MintAsset {
            //     name: "SOL".to_string(),
            //     symbol: "Wrapped SOL".to_string(),
            //     market_price_sf: 0,
            //     mint: "So11111111111111111111111111111111111111112".parse().unwrap(),
            //     lending_reserves: vec![],
            // },
            // MintAsset {
            //     name: "USDT".to_string(),
            //     symbol: "USDT".to_string(),
            //     market_price_sf: 0,
            //     mint: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB".parse().unwrap(),
            //     lending_reserves: vec![],
            // },
            // MintAsset {
            //     name: "USDS".to_string(),
            //     symbol: "USDC".to_string(),
            //     market_price_sf: 0,
            //     mint: "USDSwr9ApdHk5bvJKMjzff41FfuX8bSxdKcR81vTwcA".parse().unwrap(),
            //     lending_reserves: vec![],
            // },
            // MintAsset {
            //     name: "mSOL".to_string(),
            //     symbol: "Marinade staked SOL (mSOL)".to_string(),
            //     market_price_sf: 0,
            //     mint: "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So".parse().unwrap(),
            //     lending_reserves: vec![],
            // },
            // MintAsset {
            //     name: "jitoSOL".to_string(),
            //     symbol: "Jito Staked SOL".to_string(),
            //     market_price_sf: 0,
            //     mint: "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn".parse().unwrap(),
            //     lending_reserves: vec![],
            // },
            // MintAsset {
            //     name: "pyusd".to_string(),
            //     symbol: "PayPal USD".to_string(),
            //     market_price_sf: 0,
            //     mint: "2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo".parse().unwrap(),
            //     lending_reserves: vec![],
            // },
        ]
    }

    pub fn load_markets(&mut self) -> ArrayResult<()> {
        // Initialize assets with hardcoded list of supported tokens
        // This defines the tokens we track across all lending protocols
        // Each MintAsset represents a token with its metadata and will accumulate lending reserves
        self.assets = Self::get_valid_assets();

        // Get current slot from RPC
        let current_slot = self.save_client.program.rpc().get_slot()?;

        println!("Starting loading all lending markets from slot {}...", current_slot);
        // Load Save/Kamino reserves
        println!("Loading Save reserves");
        self.save_client.load_all_reserves()?;

        println!("Loading Kamino reserves");
        self.kamino_client.load_markets()?;

        println!("Loading Marginfi reserves");
        self.marginfi_client.load_banks_for_group()?;

        println!("Loading Drift reserves");
        self.drift_client.load_spot_markets()?;

        println!("Done loading all lending markets.");

        for pool in &self.save_client.pools {
            for reserve in &pool.reserves {
                let mint_pubkey =
                    Pubkey::from_str(&reserve.liquidity.mint_pubkey.to_string()).unwrap();
                // Convert reserve to LendingReserve and add to correct asset
                if let Some(asset) =
                    self.assets.iter_mut().find(|a| a.mint == mint_pubkey.to_string())
                {
                    asset.lending_reserves.push(LendingReserve::from(SaveReserveWrapper {
                        reserve,
                        market_name: &pool.name,
                        slot: current_slot,
                    }));
                }
            }
        }

        // Load Marginfi banks
        for (_, bank) in &self.marginfi_client.banks {
            if let Some(asset) = self.assets.iter_mut().find(|a| a.mint == bank.mint.to_string()) {
                asset.lending_reserves.push(LendingReserve::from(MarginfiReserveWrapper {
                    bank,
                    group: &self.marginfi_client.group,
                    market_name: "Global Pool",
                    slot: current_slot,
                }));
            }
        }

        // Load Kamino markets
        for (_, market, reserves) in &self.kamino_client.markets {
            let market_name = String::from_utf8(
                market.name.iter().take_while(|&&c| c != 0).copied().collect::<Vec<u8>>(),
            )
            .unwrap_or_default();

            for (_, reserve) in reserves {
                let mint_pubkey =
                    Pubkey::from_str(&reserve.liquidity.mint_pubkey.to_string()).unwrap();
                if let Some(asset) =
                    self.assets.iter_mut().find(|a| a.mint == mint_pubkey.to_string())
                {
                    asset.lending_reserves.push(LendingReserve::from(KaminoReserveWrapper {
                        reserve,
                        market_name: &market_name,
                        slot: current_slot,
                    }));
                }
            }
        }

        for (_, market) in &self.drift_client.spot_markets {
            let mint_pubkey = market.mint;
            if let Some(asset) = self.assets.iter_mut().find(|a| a.mint == mint_pubkey.to_string())
            {
                let market_name = String::from_utf8(
                    market.name.iter().take_while(|&&c| c != 0).copied().collect::<Vec<u8>>(),
                )
                .unwrap_or_default();

                asset.lending_reserves.push(LendingReserve::from(DriftReserveWrapper {
                    market,
                    market_name: &market_name,
                    slot: current_slot,
                }));
            }
        }

        Ok(())
    }

    pub fn get_user_obligations(&self, wallet_pubkey: &str) -> ArrayResult<Vec<UserObligation>> {
        let mut obligations = Vec::new();
        info!("Fetching Save obligations for {}", wallet_pubkey);
        if let Ok(save_obligations) = self.save_client.get_user_obligations(wallet_pubkey) {
            obligations.extend(save_obligations);
        }

        // Get Marginfi obligations
        info!("Fetching Marginfi obligations for {}", wallet_pubkey);
        if let Ok(marginfi_obligations) = self.marginfi_client.get_user_obligations(wallet_pubkey) {
            obligations.extend(marginfi_obligations);
        }

        // Get Kamino obligations
        info!("Fetching Kamino obligations for {}", wallet_pubkey);
        if let Ok(kamino_obligations) = self.kamino_client.get_user_obligations(wallet_pubkey) {
            obligations.extend(kamino_obligations);
        }

        // Get Drift obligations
        info!("Fetching Drift obligations for {}", wallet_pubkey);
        if let Ok(drift_obligations) = self.drift_client.get_user_obligations(wallet_pubkey) {
            obligations.extend(drift_obligations);
        }

        Ok(obligations)
    }

    pub fn print_obligations(&self, obligations: &[UserObligation]) {
        use prettytable::{row, Table};

        let mut table = Table::new();
        table.add_row(row!["Protocol", "Market", "Token", "Amount", "Market Value", "Type"]);

        for obligation in obligations {
            table.add_row(row![
                obligation.protocol_name,
                obligation.market_name,
                obligation.symbol,
                format!(
                    "{:.3}",
                    obligation.amount as f64 / 10_f64.powi(obligation.mint_decimals as i32)
                ),
                match obligation.obligation_type {
                    ObligationType::Asset => "Supply",
                    ObligationType::Liability => "Borrow",
                }
            ]);
        }

        table.printstd();
    }

    pub fn print_markets(&self) {
        use prettytable::{row, Table};

        let mut table = Table::new();
        table.add_row(row![
            "Protocol",
            "Market",
            "Token",
            "Total Supply",
            "Total Borrows",
            "Supply APY",
            "Borrow APY",
            "Valid Collateral"
        ]);

        const SCALE_SHIFT: u32 = 12;
        const SUPPLY_SCALE_FACTOR: f64 = 1_000_000_000_000_000_000_000_000.0;
        for asset in &self.assets {
            for reserve in &asset.lending_reserves {
                table.add_row(row![
                    reserve.protocol_name,
                    reserve.market_name,
                    asset.name,
                    format_large_number(reserve.total_supply as f64 / SUPPLY_SCALE_FACTOR),
                    format_large_number(reserve.total_borrows as f64 / SUPPLY_SCALE_FACTOR),
                    format!(
                        "{:.2}%",
                        reserve.supply_apy as f64 / (1u64 << SCALE_SHIFT) as f64 * 100.0
                    ),
                    format!(
                        "{:.2}%",
                        reserve.borrow_apy as f64 / (1u64 << SCALE_SHIFT) as f64 * 100.0
                    ),
                    reserve
                        .collateral_assets
                        .iter()
                        .map(|c| c.symbol.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                        .chars()
                        .take(25)
                        .collect::<String>()
                        + if reserve.collateral_assets.len() > 25 { "..." } else { "" }
                ]);
            }
        }

        // let mut writer = std::io::stdout();
        // table.to_csv(&mut writer).unwrap();
        table.printstd();
    }
}

pub fn format_large_number(num: f64) -> String {
    if num >= 1_000_000_000.0 {
        format!("{:.2}bn", num / 1_000_000_000.0)
    } else if num >= 1_000_000.0 {
        format!("{:.2}m", num / 1_000_000.0)
    } else if num >= 100_000.0 {
        format!("{:.2}k", num / 1_000.0)
    } else {
        format!("{:.2}", num)
    }
}

#[derive(Debug)]
pub enum ArrayError {
    Drift(ErrorCode),
    Other(Box<dyn std::error::Error>),
}

// Convert Drift errors automatically
impl From<ErrorCode> for ArrayError {
    fn from(err: ErrorCode) -> Self {
        ArrayError::Drift(err)
    }
}

// Convert any standard error
impl From<Box<dyn std::error::Error>> for ArrayError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        ArrayError::Other(err)
    }
}

// Add this to your ArrayError implementations
impl From<String> for ArrayError {
    fn from(err: String) -> Self {
        ArrayError::Other(err.into())
    }
}

// Also implement for &str to avoid manual conversion
impl From<&str> for ArrayError {
    fn from(err: &str) -> Self {
        ArrayError::Other(err.into())
    }
}

impl From<ProgramError> for ArrayError {
    fn from(err: ProgramError) -> Self {
        ArrayError::Other(Box::new(err))
    }
}

impl From<LendingError> for ArrayError {
    fn from(err: LendingError) -> Self {
        ArrayError::Other(Box::new(err))
    }
}

impl From<KaminoLendingError> for ArrayError {
    fn from(err: KaminoLendingError) -> Self {
        ArrayError::Other(Box::new(err))
    }
}

// Add this implementation
impl From<solana_client::client_error::ClientError> for ArrayError {
    fn from(err: solana_client::client_error::ClientError) -> Self {
        ArrayError::Other(Box::new(err))
    }
}

// Result type alias
pub type ArrayResult<T> = Result<T, ArrayError>;

// Optional: Implement Display for cleaner error messages
impl std::fmt::Display for ArrayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArrayError::Drift(e) => write!(f, "Drift error: {:?}", e),
            ArrayError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl From<common::LendingError> for ArrayError {
    fn from(err: common::LendingError) -> Self {
        ArrayError::Other(Box::new(err))
    }
}

// Implement Error trait to work with ? operator
impl std::error::Error for ArrayError {}

// fn load_token_metadata(
//     &mut self,
//     program: &Program<&Keypair>,
//     mint: &Pubkey,
// ) -> Result<(String, String), String> {
//     if let Some(metadata) = self.metadata_cache.get(mint) {
//         return Ok(metadata.clone());
//     }

//     let metadata_pda = self.get_metadata_pda(mint);
//     let metadata_acc = program
//         .rpc()
//         .get_account(&metadata_pda)
//         .map_err(|e| format!("Failed to fetch metadata: {}", e))?;

//     let metadata = Metadata::from_bytes(&metadata_acc.data)
//         .map_err(|e| format!("Failed to parse metadata: {}", e))?;

//     let result = (
//         metadata.name.trim_matches(char::from(0)).to_string(),
//         metadata.symbol.trim_matches(char::from(0)).to_string(),
//     );

//     // Cache the result
//     self.metadata_cache.insert(*mint, result.clone());

//     Ok(result)
// }

// fn get_metadata_pda(&self, mint: &Pubkey) -> Pubkey {
//     let metadata_program_id =
//         Pubkey::from_str("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s").unwrap();
//     let seeds = &[b"metadata", metadata_program_id.as_ref(), mint.as_ref()];
//     let (metadata_pda, _) = Pubkey::find_program_address(seeds, &metadata_program_id);
//     metadata_pda
// }

// let valid_collateral: Vec<MintAsset> = pool
//     .reserves
//     .iter()
//     .filter(|reserve| reserve.config.liquidation_threshold > 0)
//     .map(|reserve| {
//         let mint_pubkey =
//             Pubkey::from_str(&reserve.liquidity.mint_pubkey.to_string()).unwrap();
//         let (name, symbol) = self
//             .load_token_metadata(&program, &mint_pubkey)
//             .unwrap_or(("Unknown".to_string(), "Unknown".to_string()));
//         MintAsset {
//             name,
//             symbol,
//             market_price_sf: 0,
//             mint: reserve.liquidity.mint_pubkey.to_string(),
//             lending_reserves: vec![],
//         }
//     })
//     .collect();

// // Load Drift markets
// let mut pool_assets: HashMap<u8, Vec<MintAsset>> = HashMap::new();

// // Group spot markets by pool_id and create MintAssets
// for (_, market) in &self.drift_client.spot_markets.clone() {
//     if market.optimal_utilization > 0 {
//         let mint_pubkey = market.mint;
//         let (name, symbol) = self
//             .load_token_metadata(&program, &mint_pubkey)
//             .unwrap_or(("Unknown".to_string(), "Unknown".to_string()));

//         let mint_asset = MintAsset {
//             name,
//             symbol,
//             market_price_sf: 0,
//             mint: mint_pubkey.to_string(),
//             lending_reserves: vec![],
//         };

//         pool_assets.entry(market.pool_id).or_default().push(mint_asset);
//     }
// }

// let valid_collateral: Vec<MintAsset> = reserves
//     .iter()
//     .filter(|(_, reserve)| reserve.config.liquidation_threshold_pct > 0)
//     .map(|(_, reserve)| {
//         let mint_pubkey =
//             Pubkey::from_str(&reserve.liquidity.mint_pubkey.to_string()).unwrap();
//         let (name, symbol) = self
//             .load_token_metadata(&program, &mint_pubkey)
//             .unwrap_or(("Unknown".to_string(), "Unknown".to_string()));
//         MintAsset {
//             name,
//             symbol,
//             market_price_sf: 0,
//             mint: reserve.liquidity.mint_pubkey.to_string(),
//             lending_reserves: vec![],
//         }
//     })
//     .collect();
