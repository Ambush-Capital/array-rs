use crate::MintAsset;
use std::collections::HashMap;
use std::sync::OnceLock;

// Struct to hold asset information
#[derive(Debug, Clone)]
pub struct AssetInfo {
    pub symbol: String,
    pub is_valid: bool,
}

// Static asset map using OnceLock for safe initialization
static ASSET_MAP: OnceLock<HashMap<String, AssetInfo>> = OnceLock::new();

// Utility function to extract market name from null-terminated byte array
pub fn extract_market_name(name_bytes: &[u8]) -> String {
    String::from_utf8_lossy(name_bytes).trim_matches(char::from(0)).to_string()
}

// Get the asset map with all assets (valid and invalid)
pub fn get_asset_map() -> &'static HashMap<String, AssetInfo> {
    ASSET_MAP.get_or_init(|| {
        let mut map = HashMap::new();

        // Define all assets with their symbols, mint addresses, and validity
        let tokens = [
            ("SOL", "So11111111111111111111111111111111111111112", false),
            ("USDC", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", true),
            ("USDT", "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB", false),
            // Add more tokens as needed
        ];

        // Create AssetInfo objects for each token
        for (symbol, mint, is_valid) in tokens {
            map.insert(mint.to_string(), AssetInfo { symbol: symbol.to_string(), is_valid });
        }

        map
    })
}

// Helper function to get symbol for a mint address from the asset map
pub fn get_symbol_for_mint(mint: &str) -> Option<String> {
    get_asset_map().get(mint).map(|info| info.symbol.clone())
}

// Get valid assets for the aggregator (filtered by is_valid)
pub fn get_valid_assets() -> HashMap<String, MintAsset> {
    let asset_map = get_asset_map();
    let mut assets = HashMap::new();

    // Filter for valid assets only and create MintAsset objects
    for (mint, info) in asset_map {
        if info.is_valid {
            assets.insert(
                mint.to_string(),
                MintAsset {
                    name: info.symbol.to_string(),
                    symbol: info.symbol.to_string(),
                    market_price_sf: 0,
                    mint: mint.to_string(),
                    lending_reserves: Vec::new(),
                },
            );
        }
    }

    assets
}

// Format large numbers for display
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
