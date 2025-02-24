#![allow(clippy::empty_line_after_doc_comments)]

use anchor_client::{Client, Cluster};
use sol_interface::aggregator::client::LendingMarketAggregator;
use solana_sdk::{commitment_config::CommitmentConfig, signature::read_keypair_file};

fn main() {
    env_logger::init();
    // Initialize clients
    let rpc_url = std::env::var("RPC_URL").expect("Missing RPC_URL environment variable");
    let keypair_path =
        std::env::var("KEYPAIR_PATH").expect("Missing KEYPAIR_PATH environment variable");
    let payer = read_keypair_file(keypair_path).expect("Failed to read keypair file");
    let client = Client::new_with_options(
        Cluster::Custom(rpc_url.clone(), rpc_url),
        &payer,
        CommitmentConfig::confirmed(),
    );
    // AmrekAq6s3n2frDi67WUaZnbPkBb1h4xaid1Y8QLMAYN
    let mut aggregator = LendingMarketAggregator::new(&client);
    let _ = aggregator.load_markets();
    aggregator.print_markets();

    let obligations =
        aggregator.get_user_obligations("AmrekAq6s3n2frDi67WUaZnbPkBb1h4xaid1Y8QLMAYN");
    aggregator.print_obligations(&obligations.unwrap());
}
