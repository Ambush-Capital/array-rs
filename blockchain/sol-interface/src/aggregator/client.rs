use std::collections::HashMap;

use crate::{
    aggregator::utils::get_valid_assets, common::client_trait::ClientError,
    kamino::client::KaminoClient, marginfi::client::MarginfiClient, save::client::SaveClient,
};
use common::MintAsset;
use drift::client::DriftClient;

// Type alias for results
pub type ArrayResult<T> = Result<T, ClientError>;

pub struct LendingMarketAggregator {
    pub assets: HashMap<String, MintAsset>, // Maps mint string to MintAsset directly
    pub save_client: SaveClient,
    pub marginfi_client: MarginfiClient,
    pub kamino_client: KaminoClient,
    pub drift_client: DriftClient,
    pub rpc_url: String, // Store the RPC URL for use with pooled clients
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

    // Initialize supported tokens
    pub fn init_supported_tokens(&mut self) {
        // Clear existing assets first to avoid type mismatches
        self.assets.clear();

        // Get valid assets and insert them into our assets map
        for (key, value) in get_valid_assets() {
            self.assets.insert(key, value);
        }
    }
}
