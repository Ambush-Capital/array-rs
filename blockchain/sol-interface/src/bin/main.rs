#![allow(clippy::empty_line_after_doc_comments)]

use anchor_client::{Client, Cluster};
use solana_sdk::{commitment_config::CommitmentConfig, signature::read_keypair_file};

use sol_interface::aggregator::LendingMarketAggregator;

fn main() {
    env_logger::init();
    // Initialize clients
    let rpc_url = std::env::var("RPC_URL").expect("Missing RPC_URL environment variable");
    let payer = read_keypair_file("/Users/aaronhenshaw/.config/solana/id.json")
        .expect("Failed to read keypair file");
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
