use std::{collections::HashMap, str::FromStr};

use crate::{
    aggregator::from::{
        DriftReserveWrapper, KaminoReserveWrapper, MarginfiReserveWrapper, SaveReserveWrapper,
    },
    common::client_trait::ClientError,
    kamino::client::KaminoClient,
    marginfi::{client::MarginfiClient, models::group::MarginfiGroup},
    save::client::SaveClient,
};
use common::{lending::LendingClient, LendingReserve, MintAsset, ObligationType, UserObligation};
use common_rpc;
use drift::client::DriftClient;
use log::{error, info, warn};
use solana_sdk::pubkey::Pubkey;

pub struct LendingMarketAggregator {
    pub assets: HashMap<String, MintAsset>, // Maps mint string to MintAsset directly
    // metadata_cache: HashMap<Pubkey, (String, String)>,
    save_client: SaveClient,
    marginfi_client: MarginfiClient,
    kamino_client: KaminoClient,
    drift_client: DriftClient,
    rpc_url: String, // Store the RPC URL for use with pooled clients
}

impl Default for LendingMarketAggregator {
    fn default() -> Self {
        Self::new("https://api.mainnet-beta.solana.com")
    }
}

impl LendingMarketAggregator {
    pub fn new(rpc_url: &str) -> Self {
        // Create clients for each protocol
        let save_client = SaveClient::new(rpc_url);
        let marginfi_client = MarginfiClient::new(rpc_url);
        let kamino_client = KaminoClient::new(rpc_url);
        let drift_client = DriftClient::new(rpc_url);

        // Create the aggregator with initial empty assets
        let mut aggregator = Self {
            assets: HashMap::new(),
            save_client,
            marginfi_client,
            kamino_client,
            drift_client,
            rpc_url: rpc_url.to_string(),
        };

        // Initialize supported tokens
        aggregator.init_supported_tokens();

        aggregator
    }

    // Utility function to extract market name from null-terminated byte array
    fn extract_market_name(name_bytes: &[u8]) -> String {
        String::from_utf8_lossy(name_bytes).trim_matches(char::from(0)).to_string()
    }

    fn get_valid_assets() -> HashMap<String, MintAsset> {
        let mut assets = HashMap::new();

        // Define common assets with their symbols and mint addresses
        let tokens = [
            // ("SOL", "So11111111111111111111111111111111111111112"),
            ("USDC", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
            // ("USDT", "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"),
            // Add more tokens as needed
        ];

        // Create MintAsset objects for each token
        for (symbol, mint) in tokens {
            assets.insert(
                mint.to_string(),
                MintAsset {
                    name: symbol.to_string(),
                    symbol: symbol.to_string(),
                    market_price_sf: 0,
                    mint: mint.to_string(),
                    lending_reserves: Vec::new(),
                },
            );
        }

        assets
    }

    // Initialize supported tokens
    fn init_supported_tokens(&mut self) {
        self.assets = Self::get_valid_assets();
    }

    pub fn load_markets(&mut self) -> ArrayResult<()> {
        // Use parallel execution by default
        self.load_markets_parallel()
    }

    pub fn load_markets_parallel(&mut self) -> ArrayResult<()> {
        // Create a runtime for executing the parallel tasks if we're not already in one
        match tokio::runtime::Runtime::new() {
            Ok(runtime) => {
                // We're not in a runtime, so create one and use it
                runtime.block_on(self.load_markets_async())
            }
            Err(_) => {
                // We might be in a runtime already, fall back to sequential implementation
                // to avoid the "Cannot start a runtime from within a runtime" error
                warn!("Failed to create Tokio runtime, falling back to sequential implementation");
                self.load_markets_sequential()
            }
        }
    }

    pub async fn load_markets_async(&mut self) -> ArrayResult<()> {
        info!("Loading all lending markets in parallel");

        // Initialize supported tokens
        self.init_supported_tokens();

        // Get the current slot using the pooled client approach
        let current_slot = common_rpc::with_rpc_client(&self.rpc_url, |client| {
            client.get_slot().map_err(|e| ClientError::RpcError(Box::new(e)))
        })?;

        // Clone clients for use in separate tasks
        let save_client = self.save_client.clone();
        let marginfi_client = self.marginfi_client.clone();
        let kamino_client = self.kamino_client.clone();
        let drift_client = self.drift_client.clone();

        // Create futures for each protocol using the generic fetch_markets method
        let save_future = tokio::spawn(async move {
            info!("Loading Save reserves");
            match save_client.fetch_markets() {
                Ok(reserves) => reserves,
                Err(e) => {
                    warn!("Failed to load Save reserves: {}", e);
                    Vec::new()
                }
            }
        });

        // Handle Kamino separately due to different return type
        let kamino_future = tokio::spawn(async move {
            info!("Loading Kamino reserves");
            match kamino_client.fetch_markets() {
                Ok(markets) => markets,
                Err(e) => {
                    warn!("Failed to load Kamino reserves: {}", e);
                    Vec::new()
                }
            }
        });

        // Now we can use the generic fetch_markets for MarginfiClient too
        let marginfi_future = tokio::spawn(async move {
            info!("Loading Marginfi markets");
            match marginfi_client.fetch_markets() {
                Ok(data) => data,
                Err(e) => {
                    warn!("Failed to load Marginfi markets: {}", e);
                    (Vec::new(), MarginfiGroup::default())
                }
            }
        });

        let drift_future = tokio::spawn(async move {
            info!("Loading Drift reserves");
            match drift_client.fetch_markets() {
                Ok(markets) => markets,
                Err(e) => {
                    warn!("Failed to load Drift reserves: {}", e);
                    Vec::new()
                }
            }
        });

        // Await futures separately to handle different types
        let save_reserves = match save_future.await {
            Ok(reserves) => reserves,
            Err(e) => {
                warn!("Failed to join Save task: {}", e);
                Vec::new()
            }
        };

        let kamino_markets = match kamino_future.await {
            Ok(markets) => markets,
            Err(e) => {
                warn!("Failed to join Kamino task: {}", e);
                Vec::new()
            }
        };

        let marginfi_data = match marginfi_future.await {
            Ok(data) => data,
            Err(e) => {
                warn!("Failed to join Marginfi task: {}", e);
                (Vec::new(), MarginfiGroup::default())
            }
        };

        let drift_markets = match drift_future.await {
            Ok(markets) => markets,
            Err(e) => {
                warn!("Failed to join Drift task: {}", e);
                Vec::new()
            }
        };

        // Update client state with fetched data using the generic set_market_data method
        self.save_client.set_market_data(save_reserves);
        self.kamino_client.set_market_data(kamino_markets);
        self.marginfi_client.set_market_data(marginfi_data);
        self.drift_client.set_market_data(drift_markets);

        info!("Done loading all lending markets.");

        // Process reserves
        self.process_all_reserves(current_slot);

        Ok(())
    }

    // New helper method to process all reserves
    fn process_all_reserves(&mut self, current_slot: u64) {
        // Process Save reserves
        self.process_save_reserves(current_slot);

        // Process Marginfi banks
        self.process_marginfi_banks(current_slot);

        // Process Kamino markets
        self.process_kamino_markets(current_slot);

        // Process Drift markets
        self.process_drift_markets(current_slot);
    }

    pub fn load_markets_sequential(&mut self) -> ArrayResult<()> {
        info!("Loading all lending markets sequentially");

        // Initialize supported tokens
        self.init_supported_tokens();

        // Get the current slot using the pooled client approach
        let current_slot = common_rpc::with_rpc_client(&self.rpc_url, |client| {
            client.get_slot().map_err(|e| ClientError::RpcError(Box::new(e)))
        })?;

        // Fetch markets for each protocol sequentially using the generic fetch_markets method
        info!("Loading Save reserves");
        let save_reserves = match self.save_client.fetch_markets() {
            Ok(reserves) => reserves,
            Err(e) => {
                warn!("Failed to load Save reserves: {}", e);
                Vec::new()
            }
        };

        info!("Loading Kamino reserves");
        let kamino_markets = match self.kamino_client.fetch_markets() {
            Ok(markets) => markets,
            Err(e) => {
                warn!("Failed to load Kamino reserves: {}", e);
                Vec::new()
            }
        };

        // Now we can use the generic fetch_markets for MarginfiClient too
        info!("Loading Marginfi markets");
        let marginfi_data = match self.marginfi_client.fetch_markets() {
            Ok(data) => data,
            Err(e) => {
                warn!("Failed to load Marginfi markets: {}", e);
                (Vec::new(), MarginfiGroup::default())
            }
        };

        info!("Loading Drift reserves");
        let drift_markets = match self.drift_client.fetch_markets() {
            Ok(markets) => markets,
            Err(e) => {
                warn!("Failed to load Drift reserves: {}", e);
                Vec::new()
            }
        };

        // Update client state with fetched data using the generic set_market_data method
        self.save_client.set_market_data(save_reserves);
        self.kamino_client.set_market_data(kamino_markets);
        self.marginfi_client.set_market_data(marginfi_data);
        self.drift_client.set_market_data(drift_markets);

        info!("Done loading all lending markets.");

        // Process reserves
        self.process_all_reserves(current_slot);

        Ok(())
    }

    // Helper methods to process each protocol's reserves
    fn process_save_reserves(&mut self, current_slot: u64) {
        for pool in &self.save_client.pools {
            for reserve in &pool.reserves {
                if let Ok(mint_pubkey) =
                    Pubkey::from_str(&reserve.liquidity.mint_pubkey.to_string())
                {
                    let mint_str = mint_pubkey.to_string();
                    if let Some(asset) = self.assets.get_mut(&mint_str) {
                        asset.lending_reserves.push(LendingReserve::from(SaveReserveWrapper {
                            reserve,
                            market_name: &pool.name,
                            slot: current_slot,
                        }));
                    }
                }
            }
        }
    }

    fn process_marginfi_banks(&mut self, current_slot: u64) {
        for (_, bank) in &self.marginfi_client.banks {
            let mint_str = bank.mint.to_string();
            if let Some(asset) = self.assets.get_mut(&mint_str) {
                asset.lending_reserves.push(LendingReserve::from(MarginfiReserveWrapper {
                    bank,
                    group: &self.marginfi_client.group,
                    market_name: "Global Pool",
                    slot: current_slot,
                }));
            }
        }
    }

    fn process_kamino_markets(&mut self, current_slot: u64) {
        for (_, market, reserves) in &self.kamino_client.markets {
            let market_name = Self::extract_market_name(&market.name);

            for (_, reserve) in reserves {
                if let Ok(mint_pubkey) =
                    Pubkey::from_str(&reserve.liquidity.mint_pubkey.to_string())
                {
                    let mint_str = mint_pubkey.to_string();
                    if let Some(asset) = self.assets.get_mut(&mint_str) {
                        asset.lending_reserves.push(LendingReserve::from(KaminoReserveWrapper {
                            reserve,
                            market_name: &market_name,
                            slot: current_slot,
                        }));
                    }
                }
            }
        }
    }

    fn process_drift_markets(&mut self, current_slot: u64) {
        for (_, market) in &self.drift_client.spot_markets {
            let mint_str = market.mint.to_string();
            if let Some(asset) = self.assets.get_mut(&mint_str) {
                let market_name = Self::extract_market_name(&market.name);
                asset.lending_reserves.push(LendingReserve::from(DriftReserveWrapper {
                    market,
                    market_name: &market_name,
                    slot: current_slot,
                }));
            }
        }
    }

    pub fn get_user_obligations(&self, wallet_pubkey: &str) -> ArrayResult<Vec<UserObligation>> {
        // Create a runtime for executing the parallel tasks if we're not already in one
        match tokio::runtime::Runtime::new() {
            Ok(runtime) => {
                // We're not in a runtime, so create one and use it
                runtime.block_on(self.get_user_obligations_async(wallet_pubkey))
            }
            Err(_) => {
                // We might be in a runtime already, fall back to sequential implementation
                // to avoid the "Cannot start a runtime from within a runtime" error
                warn!("Failed to create Tokio runtime, falling back to sequential implementation");
                self.get_user_obligations_sequential(wallet_pubkey)
            }
        }
    }

    /// Async version of get_user_obligations - use this when already in an async context
    pub async fn get_user_obligations_async(
        &self,
        wallet_pubkey: &str,
    ) -> ArrayResult<Vec<UserObligation>> {
        use futures::future;
        use std::sync::Arc;

        info!("Fetching user obligations for {} in parallel", wallet_pubkey);

        // Clone the clients for use in separate tasks
        let save_client = self.save_client.clone();
        let marginfi_client = self.marginfi_client.clone();
        let kamino_client = self.kamino_client.clone();
        let drift_client = self.drift_client.clone();

        // Create a shared reference to the wallet pubkey
        let wallet_pubkey = wallet_pubkey.to_string();
        let wallet_pubkey = Arc::new(wallet_pubkey);

        // Create futures for each protocol
        let save_future = {
            let wallet_pubkey = Arc::clone(&wallet_pubkey);
            tokio::spawn(async move {
                info!("Fetching Save obligations for {}", wallet_pubkey);
                match save_client.get_user_obligations(&wallet_pubkey) {
                    Ok(obligations) => {
                        info!("Found {} Save obligations", obligations.len());
                        obligations
                    }
                    Err(e) => {
                        error!("Error fetching Save obligations: {}", e);
                        Vec::new()
                    }
                }
            })
        };

        let marginfi_future = {
            let wallet_pubkey = Arc::clone(&wallet_pubkey);
            tokio::spawn(async move {
                info!("Fetching Marginfi obligations for {}", wallet_pubkey);
                match marginfi_client.get_user_obligations(&wallet_pubkey) {
                    Ok(obligations) => {
                        info!("Found {} Marginfi obligations", obligations.len());
                        obligations
                    }
                    Err(e) => {
                        error!("Error fetching Marginfi obligations: {}", e);
                        Vec::new()
                    }
                }
            })
        };

        let kamino_future = {
            let wallet_pubkey = Arc::clone(&wallet_pubkey);
            tokio::spawn(async move {
                info!("Fetching Kamino obligations for {}", wallet_pubkey);
                match kamino_client.get_user_obligations(&wallet_pubkey) {
                    Ok(obligations) => {
                        info!("Found {} Kamino obligations", obligations.len());
                        obligations
                    }
                    Err(e) => {
                        error!("Error fetching Kamino obligations: {}", e);
                        Vec::new()
                    }
                }
            })
        };

        let drift_future = {
            let wallet_pubkey = Arc::clone(&wallet_pubkey);
            tokio::spawn(async move {
                info!("Fetching Drift obligations for {}", wallet_pubkey);
                match drift_client.get_user_obligations(&wallet_pubkey) {
                    Ok(obligations) => {
                        info!("Found {} Drift obligations", obligations.len());
                        obligations
                    }
                    Err(e) => {
                        error!("Error fetching Drift obligations: {}", e);
                        Vec::new()
                    }
                }
            })
        };

        // Collect results from all tasks
        let mut obligations = Vec::new();

        // Await all futures and collect results
        let results =
            future::join4(save_future, marginfi_future, kamino_future, drift_future).await;

        for (protocol, result) in ["Save", "Marginfi", "Kamino", "Drift"]
            .iter()
            .zip([results.0, results.1, results.2, results.3])
        {
            match result {
                Ok(protocol_obligations) => obligations.extend(protocol_obligations),
                Err(e) => error!("Error joining {} task: {}", protocol, e),
            }
        }

        Ok(obligations)
    }

    pub fn get_user_obligations_sequential(
        &self,
        wallet_pubkey: &str,
    ) -> ArrayResult<Vec<UserObligation>> {
        info!("Fetching user obligations for {} sequentially", wallet_pubkey);
        let mut obligations = Vec::new();

        // Fetch Save obligations
        info!("Fetching Save obligations for {}", wallet_pubkey);
        match self.save_client.get_user_obligations(wallet_pubkey) {
            Ok(save_obligations) => {
                info!("Found {} Save obligations", save_obligations.len());
                obligations.extend(save_obligations);
            }
            Err(e) => {
                error!("Error fetching Save obligations: {}", e);
            }
        }

        // Fetch Marginfi obligations
        info!("Fetching Marginfi obligations for {}", wallet_pubkey);
        match self.marginfi_client.get_user_obligations(wallet_pubkey) {
            Ok(marginfi_obligations) => {
                info!("Found {} Marginfi obligations", marginfi_obligations.len());
                obligations.extend(marginfi_obligations);
            }
            Err(e) => {
                error!("Error fetching Marginfi obligations: {}", e);
            }
        }

        // Fetch Kamino obligations
        info!("Fetching Kamino obligations for {}", wallet_pubkey);
        match self.kamino_client.get_user_obligations(wallet_pubkey) {
            Ok(kamino_obligations) => {
                info!("Found {} Kamino obligations", kamino_obligations.len());
                obligations.extend(kamino_obligations);
            }
            Err(e) => {
                error!("Error fetching Kamino obligations: {}", e);
            }
        }

        // Fetch Drift obligations
        info!("Fetching Drift obligations for {}", wallet_pubkey);
        match self.drift_client.get_user_obligations(wallet_pubkey) {
            Ok(drift_obligations) => {
                info!("Found {} Drift obligations", drift_obligations.len());
                obligations.extend(drift_obligations);
            }
            Err(e) => {
                error!("Error fetching Drift obligations: {}", e);
            }
        }

        Ok(obligations)
    }

    pub fn print_obligations(&self, obligations: &[UserObligation]) {
        use prettytable::{row, Table};

        if obligations.is_empty() {
            info!("No obligations found");
            return;
        }

        let mut table = Table::new();
        table.add_row(row!["Protocol", "Market", "Token", "Amount", "Type"]);

        for obligation in obligations {
            // Format amount with appropriate decimal places
            let formatted_amount = format!(
                "{:.3}",
                obligation.amount as f64 / 10_f64.powi(obligation.mint_decimals as i32)
            );

            // Determine obligation type string
            let obligation_type = match obligation.obligation_type {
                ObligationType::Asset => "Supply",
                ObligationType::Liability => "Borrow",
            };

            table.add_row(row![
                obligation.protocol_name,
                obligation.market_name,
                obligation.symbol,
                formatted_amount,
                obligation_type
            ]);
        }

        table.printstd();
    }

    pub fn print_markets(&self) {
        use prettytable::{row, Table};

        if self.assets.is_empty() {
            info!("No markets loaded");
            return;
        }

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

        // Iterate through all assets in the HashMap
        for asset in self.assets.values() {
            for reserve in &asset.lending_reserves {
                // Format collateral assets list
                let collateral_assets = reserve
                    .collateral_assets
                    .iter()
                    .map(|c| c.symbol.clone())
                    .collect::<Vec<_>>()
                    .join(", ");

                // Truncate if too long
                let collateral_display = if collateral_assets.len() > 25 {
                    format!("{}...", &collateral_assets[..25])
                } else {
                    collateral_assets
                };

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
                    collateral_display
                ]);
            }
        }

        table.printstd();
    }
}

pub fn format_large_number(num: f64) -> String {
    const BILLION: f64 = 1_000_000_000.0;
    const MILLION: f64 = 1_000_000.0;
    const THOUSAND: f64 = 1_000.0;

    match num {
        n if n >= BILLION => format!("{:.2}bn", n / BILLION),
        n if n >= MILLION => format!("{:.2}m", n / MILLION),
        n if n >= THOUSAND => format!("{:.2}k", n / THOUSAND),
        _ => format!("{:.2}", num),
    }
}

// Result type alias
pub type ArrayResult<T> = Result<T, ClientError>;
