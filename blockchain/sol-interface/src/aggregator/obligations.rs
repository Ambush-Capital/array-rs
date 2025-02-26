use crate::{aggregator::client::LendingMarketAggregator, common::client_trait::ClientError};
use common::{ObligationType, UserObligation};
use log::{error, info, warn};
use std::sync::Arc;

// Type alias for results
type ArrayResult<T> = Result<T, ClientError>;

impl LendingMarketAggregator {
    pub fn get_user_obligations(&self, wallet_pubkey: &str) -> ArrayResult<Vec<UserObligation>> {
        // Create a runtime for executing the parallel tasks if we're not already in one
        match tokio::runtime::Runtime::new() {
            Ok(runtime) => {
                // We're not in a runtime, so create one and use it
                runtime.block_on(self.get_user_obligations_async(wallet_pubkey))
            }
            Err(_) => {
                // We might be in a runtime already, fall back to sequential implementation
                // to avoid the "Cannot start a runtime from within a runtime" error
                warn!("Failed to create Tokio runtime, falling back to sequential implementation");
                self.get_user_obligations_sequential(wallet_pubkey)
            }
        }
    }

    /// Async version of get_user_obligations - use this when already in an async context
    pub async fn get_user_obligations_async(
        &self,
        wallet_pubkey: &str,
    ) -> ArrayResult<Vec<UserObligation>> {
        use futures::future;

        info!("Fetching user obligations for {} in parallel", wallet_pubkey);

        // Clone the clients for use in separate tasks
        let save_client = self.save_client.clone();
        let marginfi_client = self.marginfi_client.clone();
        let kamino_client = self.kamino_client.clone();
        let drift_client = self.drift_client.clone();

        // Create a shared reference to the wallet pubkey
        let wallet_pubkey = wallet_pubkey.to_string();
        let wallet_pubkey = Arc::new(wallet_pubkey);

        // Create futures for each protocol
        let save_future = {
            let wallet_pubkey = Arc::clone(&wallet_pubkey);
            tokio::spawn(async move {
                info!("Fetching Save obligations for {}", wallet_pubkey);
                match save_client.get_user_obligations(&wallet_pubkey) {
                    Ok(obligations) => {
                        info!("Found {} Save obligations", obligations.len());
                        obligations
                    }
                    Err(e) => {
                        error!("Error fetching Save obligations: {}", e);
                        Vec::new()
                    }
                }
            })
        };

        let marginfi_future = {
            let wallet_pubkey = Arc::clone(&wallet_pubkey);
            tokio::spawn(async move {
                info!("Fetching Marginfi obligations for {}", wallet_pubkey);
                match marginfi_client.get_user_obligations(&wallet_pubkey) {
                    Ok(obligations) => {
                        info!("Found {} Marginfi obligations", obligations.len());
                        obligations
                    }
                    Err(e) => {
                        error!("Error fetching Marginfi obligations: {}", e);
                        Vec::new()
                    }
                }
            })
        };

        let kamino_future = {
            let wallet_pubkey = Arc::clone(&wallet_pubkey);
            tokio::spawn(async move {
                info!("Fetching Kamino obligations for {}", wallet_pubkey);
                match kamino_client.get_user_obligations(&wallet_pubkey) {
                    Ok(obligations) => {
                        info!("Found {} Kamino obligations", obligations.len());
                        obligations
                    }
                    Err(e) => {
                        error!("Error fetching Kamino obligations: {}", e);
                        Vec::new()
                    }
                }
            })
        };

        let drift_future = {
            let wallet_pubkey = Arc::clone(&wallet_pubkey);
            tokio::spawn(async move {
                info!("Fetching Drift obligations for {}", wallet_pubkey);
                match drift_client.get_user_obligations(&wallet_pubkey) {
                    Ok(obligations) => {
                        info!("Found {} Drift obligations", obligations.len());
                        obligations
                    }
                    Err(e) => {
                        error!("Error fetching Drift obligations: {}", e);
                        Vec::new()
                    }
                }
            })
        };

        // Collect results from all tasks
        let mut obligations = Vec::new();

        // Await all futures and collect results
        let results =
            future::join4(save_future, marginfi_future, kamino_future, drift_future).await;

        for (protocol, result) in ["Save", "Marginfi", "Kamino", "Drift"]
            .iter()
            .zip([results.0, results.1, results.2, results.3])
        {
            match result {
                Ok(protocol_obligations) => obligations.extend(protocol_obligations),
                Err(e) => error!("Error joining {} task: {}", protocol, e),
            }
        }

        Ok(obligations)
    }

    pub fn get_user_obligations_sequential(
        &self,
        wallet_pubkey: &str,
    ) -> ArrayResult<Vec<UserObligation>> {
        info!("Fetching user obligations for {} sequentially", wallet_pubkey);
        let mut obligations = Vec::new();

        // Fetch Save obligations
        info!("Fetching Save obligations for {}", wallet_pubkey);
        match self.save_client.get_user_obligations(wallet_pubkey) {
            Ok(save_obligations) => {
                info!("Found {} Save obligations", save_obligations.len());
                obligations.extend(save_obligations);
            }
            Err(e) => {
                error!("Error fetching Save obligations: {}", e);
            }
        }

        // Fetch Marginfi obligations
        info!("Fetching Marginfi obligations for {}", wallet_pubkey);
        match self.marginfi_client.get_user_obligations(wallet_pubkey) {
            Ok(marginfi_obligations) => {
                info!("Found {} Marginfi obligations", marginfi_obligations.len());
                obligations.extend(marginfi_obligations);
            }
            Err(e) => {
                error!("Error fetching Marginfi obligations: {}", e);
            }
        }

        // Fetch Kamino obligations
        info!("Fetching Kamino obligations for {}", wallet_pubkey);
        match self.kamino_client.get_user_obligations(wallet_pubkey) {
            Ok(kamino_obligations) => {
                info!("Found {} Kamino obligations", kamino_obligations.len());
                obligations.extend(kamino_obligations);
            }
            Err(e) => {
                error!("Error fetching Kamino obligations: {}", e);
            }
        }

        // Fetch Drift obligations
        info!("Fetching Drift obligations for {}", wallet_pubkey);
        match self.drift_client.get_user_obligations(wallet_pubkey) {
            Ok(drift_obligations) => {
                info!("Found {} Drift obligations", drift_obligations.len());
                obligations.extend(drift_obligations);
            }
            Err(e) => {
                error!("Error fetching Drift obligations: {}", e);
            }
        }

        Ok(obligations)
    }

    pub fn print_obligations(&self, obligations: &[UserObligation]) {
        use prettytable::{row, Table};

        if obligations.is_empty() {
            info!("No obligations found");
            return;
        }

        let mut table = Table::new();
        table.add_row(row!["Protocol", "Market", "Token", "Amount", "Type"]);

        for obligation in obligations {
            // Format amount with appropriate decimal places
            let formatted_amount = format!(
                "{:.3}",
                obligation.amount as f64 / 10_f64.powi(obligation.mint_decimals as i32)
            );

            // Determine obligation type string
            let obligation_type = match obligation.obligation_type {
                ObligationType::Asset => "Supply",
                ObligationType::Liability => "Borrow",
            };

            table.add_row(row![
                obligation.protocol_name,
                obligation.market_name,
                obligation.symbol,
                formatted_amount,
                obligation_type
            ]);
        }

        table.printstd();
    }
}
