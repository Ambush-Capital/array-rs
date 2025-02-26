use common::MintAsset;
use std::collections::HashMap;

// Utility function to extract market name from null-terminated byte array
pub fn extract_market_name(name_bytes: &[u8]) -> String {
    String::from_utf8_lossy(name_bytes).trim_matches(char::from(0)).to_string()
}

// Get valid assets for the aggregator
pub fn get_valid_assets() -> HashMap<String, MintAsset> {
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
