use crate::{
    aggregator::{
        client::LendingMarketAggregator,
        from::{
            DriftReserveWrapper, KaminoReserveWrapper, MarginfiReserveWrapper, SaveReserveWrapper,
        },
        utils::extract_market_name,
    },
    common::client_trait::ClientError,
    marginfi::models::group::MarginfiGroup,
};
use common::{lending::LendingClient, LendingReserve};
use common_rpc;
use log::{info, warn};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

// Type alias for results
type ArrayResult<T> = Result<T, ClientError>;

impl LendingMarketAggregator {
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
            let market_name = extract_market_name(&market.name);

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
                let market_name = extract_market_name(&market.name);
                asset.lending_reserves.push(LendingReserve::from(DriftReserveWrapper {
                    market,
                    market_name: &market_name,
                    slot: current_slot,
                }));
            }
        }
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
                    crate::aggregator::utils::format_large_number(
                        reserve.total_supply as f64 / SUPPLY_SCALE_FACTOR
                    ),
                    crate::aggregator::utils::format_large_number(
                        reserve.total_borrows as f64 / SUPPLY_SCALE_FACTOR
                    ),
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
