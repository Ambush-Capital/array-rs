#![allow(clippy::empty_line_after_doc_comments)]

use sol_interface::aggregator::client::LendingMarketAggregator;

#[tokio::main]
async fn main() {
    env_logger::init();

    // Initialize with RPC URL
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());

    // Create the aggregator with the RPC URL
    let mut aggregator = LendingMarketAggregator::new(&rpc_url);

    // Load markets and print them
    let _ = aggregator.load_markets();
    aggregator.print_markets();

    // Get and print user obligations
    let obligations =
        aggregator.get_user_obligations("AmrekAq6s3n2frDi67WUaZnbPkBb1h4xaid1Y8QLMAYN");
    aggregator.print_obligations(&obligations.unwrap());

    // USDC mainnet token mint address
    let usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

    // Fetch USDC balance for the same wallet
    let token_balance = sol_interface::aggregator::wallet::fetch_token_balances(
        &rpc_url,
        "AmrekAq6s3n2frDi67WUaZnbPkBb1h4xaid1Y8QLMAYN",
        &[(usdc_mint.to_string(), "USDC".to_string())],
    )
    .await
    .unwrap();

    println!("USDC Balance: {:?}", token_balance);
}
