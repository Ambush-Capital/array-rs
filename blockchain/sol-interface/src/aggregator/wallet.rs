use crate::aggregator::client::LendingMarketAggregator;
use crate::common::rpc_utils::create_rpc_client;
use common::TokenBalance;
use log::debug;
use solana_client::rpc_request::TokenAccountsFilter;
use solana_program::program_pack::Pack;
use solana_sdk::pubkey::Pubkey;
use spl_token::state::Account as TokenAccount;
use std::{error::Error, str::FromStr};

impl LendingMarketAggregator {
    /// Fetch token balances for all supported assets for a specific wallet
    ///
    /// This function retrieves token balances for all supported assets in the aggregator,
    /// using the RPC client pool for better performance.
    pub async fn fetch_wallet_token_balances(
        &self,
        wallet_pubkey_str: &str,
    ) -> Result<Vec<TokenBalance>, Box<dyn Error>> {
        // Prepare token info from supported assets
        let token_info: Vec<(String, String)> =
            self.assets.iter().map(|(mint, asset)| (mint.clone(), asset.symbol.clone())).collect();

        // Call the fetch_token_balances function with our token info
        fetch_token_balances(&self.rpc_url, wallet_pubkey_str, &token_info).await
    }
}

/// Fetch token balances for a specific wallet and multiple token mints
///
/// This function retrieves token balances for multiple token mints in a single call,
/// using the RPC client pool for better performance.
/// TODO: parallelize the token account queries
pub async fn fetch_token_balances(
    rpc_url: &str,
    wallet_pubkey_str: &str,
    token_info: &[(String, String)], // Vec of (token_mint_str, token_symbol)
) -> Result<Vec<TokenBalance>, Box<dyn Error>> {
    // Parse the wallet pubkey
    let wallet_pubkey = Pubkey::from_str(wallet_pubkey_str)?;

    // Get a client from the connection pool
    let client = create_rpc_client(rpc_url);

    // Prepare the result vector
    let mut token_balances = Vec::with_capacity(token_info.len());

    // Process each token mint
    for (token_mint_str, token_symbol) in token_info {
        // Parse the token mint pubkey
        let token_mint = match Pubkey::from_str(token_mint_str) {
            Ok(pubkey) => pubkey,
            Err(err) => {
                debug!("Failed to parse token mint {}: {}", token_mint_str, err);
                // Add a zero balance for invalid mints
                token_balances.push(TokenBalance {
                    symbol: token_symbol.to_string(),
                    mint: token_mint_str.to_string(),
                    amount: 0,
                    decimals: 6, // Default to 6 decimals
                    token_account: String::new(),
                });
                continue;
            }
        };

        // Query token accounts by owner filtering with the token mint
        let token_accounts = match client
            .get_token_accounts_by_owner(&wallet_pubkey, TokenAccountsFilter::Mint(token_mint))
        {
            Ok(accounts) => accounts,
            Err(err) => {
                debug!("Failed to get token accounts for {}: {}", token_symbol, err);
                // Add a zero balance for failed queries
                token_balances.push(TokenBalance {
                    symbol: token_symbol.to_string(),
                    mint: token_mint_str.to_string(),
                    amount: 0,
                    decimals: 6,
                    token_account: String::new(),
                });
                continue;
            }
        };

        // If no accounts found, add zero balance
        if token_accounts.is_empty() {
            token_balances.push(TokenBalance {
                symbol: token_symbol.to_string(),
                mint: token_mint_str.to_string(),
                amount: 0,
                decimals: 6,
                token_account: String::new(),
            });
            continue;
        }

        // For each token account, create a TokenBalance
        for account in &token_accounts {
            // Fetch account data
            let pubkey = Pubkey::from_str(&account.pubkey)?;
            let account_data = match client.get_account_data(&pubkey) {
                Ok(data) => data,
                Err(err) => {
                    debug!("Failed to get account data for {}: {}", account.pubkey, err);
                    continue;
                }
            };

            // Decode account data
            let token_account = match TokenAccount::unpack(&account_data) {
                Ok(account) => account,
                Err(err) => {
                    debug!("Failed to unpack token account {}: {}", account.pubkey, err);
                    continue;
                }
            };

            // Add the token balance to our results
            token_balances.push(TokenBalance {
                symbol: token_symbol.to_string(),
                mint: token_mint_str.to_string(),
                amount: token_account.amount,
                decimals: token_account.mint.to_string().parse().unwrap_or(6),
                token_account: account.pubkey.clone(),
            });
        }
    }

    Ok(token_balances)
}
