#![allow(clippy::empty_line_after_doc_comments)]

use aggregator::LendingMarketAggregator;
use anchor_client::{Client, Cluster};
use drift::client::DriftClient;
use kamino::client::KaminoClient;
use marginfi::client::MarginfiClient;
use save::client::SaveClient;
use solana_sdk::{commitment_config::CommitmentConfig, signature::read_keypair_file};

pub mod aggregator;
pub mod kamino;
pub mod marginfi;
pub mod save;

fn main() {
    let rpc_url = match std::env::var("RPC_URL") {
        Ok(rpc) => rpc,
        Err(_) => {
            eprintln!("Error: Failed to load RPC_URL. Ensure the environment variable is set.");
            return;
        }
    };

    // Load the wallet keypair
    // Attempt to load the wallet keypair
    let payer = match read_keypair_file("/Users/aaronhenshaw/.config/solana/id.json") {
        Ok(keypair) => keypair,
        Err(_) => {
            eprintln!("Error: Failed to load keypair. Ensure the file exists and has correct permissions.");
            return;
        }
    };

    // Initialize the Anchor client with the custom RPC URL
    let client = Client::new_with_options(
        Cluster::Custom(rpc_url.to_string(), rpc_url.to_string()),
        &payer,
        CommitmentConfig::confirmed(),
    );

    let kamino_client = KaminoClient::new(&client);
    match kamino_client.get_obligations("AmrekAq6s3n2frDi67WUaZnbPkBb1h4xaid1Y8QLMAYN") {
        Ok(_) => (),
        Err(e) => println!("Failed to fetch obligations: {}", e),
    }

    let marginfi_client = MarginfiClient::new(&client);
    match marginfi_client.get_obligations("AmrekAq6s3n2frDi67WUaZnbPkBb1h4xaid1Y8QLMAYN") {
        Ok(_) => (),
        Err(e) => println!("Failed to fetch marginfi obligations: {}", e),
    }

    let drift_client = DriftClient::new(&client);
    match drift_client.get_obligations("AmrekAq6s3n2frDi67WUaZnbPkBb1h4xaid1Y8QLMAYN") {
        Ok(_) => (),
        Err(e) => println!("Failed to fetch drift obligations: {}", e),
    }

    let save_client = SaveClient::new(&client);
    match save_client.get_obligations("AmrekAq6s3n2frDi67WUaZnbPkBb1h4xaid1Y8QLMAYN") {
        Ok(_) => (),
        Err(e) => println!("Failed to fetch save obligations: {}", e),
    }

    let mut aggregator = LendingMarketAggregator::new();
    let _ = aggregator.load_markets();
    aggregator.print_markets();
}
