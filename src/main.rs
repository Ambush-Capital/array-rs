#![allow(clippy::empty_line_after_doc_comments)]
use std::str::FromStr;

use anchor_client::{Client, Cluster};
use anchor_lang::AccountDeserialize;
use drift::models::idl::accounts::SpotMarket;
use solana_account_decoder::UiAccountEncoding;
use solana_client::{
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_filter::{Memcmp, RpcFilterType},
};
use solana_sdk::{
    commitment_config::CommitmentConfig, pubkey::Pubkey, signature::read_keypair_file,
};

pub mod aggregator;
pub mod kamino;
pub mod marginfi;
pub mod save;

fn main() {
    let rpc_url = "https://mainnet.helius-rpc.com/?api-key=80edcf87-c27e-4dba-a1d8-1ec3a1426752"; // Custom RPC URL here

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

    let program = client
        .program(Pubkey::from_str("dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH").unwrap())
        .unwrap();

    let filters = vec![RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
        0,
        [100, 177, 8, 107, 168, 65, 65, 39].to_vec(),
    ))];

    let account_config = RpcAccountInfoConfig {
        commitment: Some(program.rpc().commitment()),
        encoding: Some(UiAccountEncoding::Base64Zstd),
        ..RpcAccountInfoConfig::default()
    };

    let gpa_config = RpcProgramAccountsConfig {
        filters: Some(filters),
        account_config: account_config.clone(),
        with_context: Some(true),
    };

    let accounts = program.rpc().get_program_accounts_with_config(&program.id(), gpa_config);

    match accounts {
        Ok(spot_market_accounts) => {
            for (pubkey, account) in spot_market_accounts {
                match SpotMarket::try_deserialize(&mut &account.data[..]) {
                    Ok(spot_market) => {
                        println!(
                            "Spot Market {} - Name: {:?}, Pubkey Length: {}, Market Index: {}",
                            pubkey,
                            pubkey.to_string().len(),
                            String::from_utf8_lossy(&spot_market.name).trim_matches(char::from(0)),
                            spot_market.market_index
                        );
                    }
                    Err(e) => println!("Failed to deserialize spot market {}: {}", pubkey, e),
                }
            }
        }
        Err(e) => println!("Failed to fetch accounts: {}", e),
    }

    // let mut aggregator = LendingMarketAggregator::new();
    // let _ = aggregator.load_markets();
    // aggregator.print_markets();
}
