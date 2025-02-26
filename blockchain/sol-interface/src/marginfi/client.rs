use std::mem::size_of;

use crate::common::rpc_utils::{
    format_pubkey_for_error, with_pooled_client, LendingErrorConverter,
};
use anchor_lang::AnchorDeserialize;
use common::{
    asset_utils::get_symbol_for_mint,
    lending::{LendingClient, LendingError},
    ObligationType, UserObligation,
};
use common_rpc::SolanaRpcBuilder;
use fixed::types::I80F48;
use log::debug;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use super::models::{
    account::{Balance, BalanceSide, MarginfiAccount},
    group::{Bank, MarginfiGroup},
};

// Define discriminators as constants
const MARGINFI_ACCOUNT_DISCRIMINATOR: [u8; 8] = [67, 178, 130, 109, 126, 114, 28, 42];
const MARGINFI_BANK_DISCRIMINATOR: [u8; 8] = [142, 49, 166, 242, 50, 66, 97, 188];

pub struct MarginfiClient {
    pub program_id: Pubkey,
    pub rpc_url: String,
    pub group_pubkeys: Vec<Pubkey>,
    pub banks: Vec<(Pubkey, Bank)>,
    pub group: MarginfiGroup,
}

impl Clone for MarginfiClient {
    fn clone(&self) -> Self {
        Self {
            program_id: self.program_id,
            rpc_url: self.rpc_url.clone(),
            group_pubkeys: self.group_pubkeys.clone(),
            banks: self.banks.clone(),
            group: self.group.clone(),
        }
    }
}

impl MarginfiClient {
    pub fn new(rpc_url: &str) -> Self {
        let program_id = Pubkey::from_str("MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA")
            .expect("Invalid Marginfi Lending Program ID");

        let group_pubkeys: Vec<Pubkey> = vec![
            "4qp6Fx6tnZkY5Wropq9wUYgtFxXKwE6viZxFHg3rdAG8".parse().unwrap(), //main market
        ];

        Self {
            program_id,
            rpc_url: rpc_url.to_string(),
            group_pubkeys,
            banks: Vec::new(),
            group: MarginfiGroup::default(),
        }
    }

    /// Updates the client's state with the fetched market data
    pub fn set_market_data(&mut self, data: (Vec<(Pubkey, Bank)>, MarginfiGroup)) {
        let (banks, group) = data;
        self.banks = banks;
        self.group = group;
    }

    pub fn fetch_banks_for_group(&self) -> Result<Vec<(Pubkey, Bank)>, LendingError> {
        // Use the RPC builder with optimized filters
        let accounts = with_pooled_client(&self.rpc_url, |client| {
            SolanaRpcBuilder::new(client, self.program_id)
                .with_memcmp(0, MARGINFI_BANK_DISCRIMINATOR.to_vec())
                .with_memcmp_pubkey(
                    8 + size_of::<Pubkey>() + size_of::<u8>(),
                    &self.group_pubkeys[0],
                )
                .optimize_filters() // Apply filter optimization
                .get_program_accounts_with_conversion::<LendingError, LendingErrorConverter>()
        })?;

        // Pre-allocate with capacity
        let mut banks = Vec::with_capacity(accounts.len());

        for (pubkey, account) in accounts {
            match Bank::try_from_slice(&account.data[8..]) {
                Ok(bank) => banks.push((pubkey, bank)),
                Err(e) => {
                    debug!(
                        "Failed to deserialize bank {}: {}",
                        format_pubkey_for_error(&pubkey),
                        e
                    );
                }
            }
        }

        Ok(banks)
    }

    pub fn load_banks_for_group(&mut self) -> Result<(), LendingError> {
        self.banks = self.fetch_banks_for_group()?;
        Ok(())
    }

    pub fn fetch_marginfi_group(
        &self,
        group_pubkey: &Pubkey,
    ) -> Result<MarginfiGroup, LendingError> {
        // Use the RPC builder to get the group account
        let group_account = with_pooled_client(&self.rpc_url, |client| {
            SolanaRpcBuilder::new(client, self.program_id)
                .get_account_with_conversion::<LendingError, LendingErrorConverter>(group_pubkey)
        })?;

        let group = MarginfiGroup::try_from_slice(&group_account.data[8..]).map_err(|e| {
            LendingError::DeserializationError(format!(
                "Failed to deserialize marginfi group {}: {}",
                format_pubkey_for_error(group_pubkey),
                e
            ))
        })?;

        Ok(group)
    }

    pub fn load_marginfi_group(&mut self, group_pubkey: &Pubkey) -> Result<(), LendingError> {
        self.group = self.fetch_marginfi_group(group_pubkey)?;
        Ok(())
    }

    pub fn get_user_obligations(
        &self,
        wallet_pubkey: &str,
    ) -> Result<Vec<UserObligation>, LendingError> {
        let mut obligations = Vec::new();
        let marginfi_accounts = self.fetch_raw_obligations(wallet_pubkey)?;

        // Cache protocol name to avoid repeated allocations
        let protocol_name = self.protocol_name().to_string();
        // Use a constant market name
        let market_name = "General".to_string();

        for (balance, bank) in marginfi_accounts {
            // Process active balances
            if let Some(side) = balance.get_side() {
                let amount = match side {
                    BalanceSide::Assets => {
                        I80F48::from(balance.asset_shares) * I80F48::from(bank.asset_share_value)
                    }
                    BalanceSide::Liabilities => {
                        I80F48::from(balance.liability_shares)
                            * I80F48::from(bank.liability_share_value)
                    }
                };

                // Get mint once
                let mint = bank.mint.to_string();

                // Look up symbol from asset map, fallback to empty string
                let symbol = get_symbol_for_mint(&mint).unwrap_or_default();

                obligations.push(UserObligation {
                    symbol,
                    mint,
                    mint_decimals: bank.mint_decimals as u32,
                    amount: I80F48::to_num(amount),
                    protocol_name: protocol_name.clone(),
                    market_name: market_name.clone(),
                    obligation_type: match side {
                        BalanceSide::Assets => ObligationType::Asset,
                        BalanceSide::Liabilities => ObligationType::Liability,
                    },
                });
            }
        }

        Ok(obligations)
    }

    fn fetch_raw_obligations(
        &self,
        owner_pubkey: &str,
    ) -> Result<Vec<(Balance, Bank)>, LendingError> {
        let owner = Pubkey::from_str(owner_pubkey).map_err(|e| {
            LendingError::InvalidAddress(format!("Invalid owner pubkey {}: {}", owner_pubkey, e))
        })?;

        // Use the RPC builder with optimized filters
        let accounts = with_pooled_client(&self.rpc_url, |client| {
            SolanaRpcBuilder::new(client, self.program_id)
                .with_memcmp(0, MARGINFI_ACCOUNT_DISCRIMINATOR.to_vec())
                .with_data_size(2304 + 8) // Size of MarginfiAccount
                .with_memcmp_pubkey(8 + 32, &owner) // Skip discriminator (8) and group pubkey (32) to get to authority
                .optimize_filters() // Apply filter optimization
                .get_program_accounts_with_conversion::<LendingError, LendingErrorConverter>()
        })?;

        if accounts.is_empty() {
            debug!("No marginfi accounts found for {}", owner_pubkey);
            return Ok(Vec::new());
        }

        // Collect all bank pubkeys first
        let mut bank_pubkeys = Vec::new();
        let mut marginfi_accounts = Vec::with_capacity(accounts.len());

        for (pubkey, account) in accounts {
            let marginfi_account =
                MarginfiAccount::try_from_slice(&account.data[8..]).map_err(|e| {
                    LendingError::DeserializationError(format!(
                        "Failed to deserialize marginfi account {}: {}",
                        format_pubkey_for_error(&pubkey),
                        e
                    ))
                })?;

            // Collect bank pubkeys for batch fetching
            for balance in marginfi_account.lending_account.get_active_balances_iter() {
                if !balance.is_empty(BalanceSide::Assets)
                    || !balance.is_empty(BalanceSide::Liabilities)
                {
                    bank_pubkeys.push(balance.bank_pk);
                }
            }

            marginfi_accounts.push(marginfi_account);
        }

        // Fetch all bank accounts in a single batch
        let bank_accounts = with_pooled_client(&self.rpc_url, |client| {
            common_rpc::get_multiple_accounts_with_conversion::<LendingError, LendingErrorConverter>(
                client,
                &bank_pubkeys,
            )
        })?;

        // Process the results
        let mut result = Vec::with_capacity(bank_pubkeys.len());

        for marginfi_account in marginfi_accounts {
            // Process active balances
            for balance in marginfi_account.lending_account.get_active_balances_iter() {
                if !balance.is_empty(BalanceSide::Assets)
                    || !balance.is_empty(BalanceSide::Liabilities)
                {
                    if let Some(bank_account) = bank_accounts.get(&balance.bank_pk) {
                        match Bank::try_from_slice(&bank_account.data[8..]) {
                            Ok(bank) => result.push((balance.clone(), bank)),
                            Err(e) => {
                                debug!(
                                    "Failed to deserialize bank {}: {}",
                                    format_pubkey_for_error(&balance.bank_pk),
                                    e
                                );
                                continue;
                            }
                        }
                    } else {
                        debug!(
                            "Failed to fetch bank account {}",
                            format_pubkey_for_error(&balance.bank_pk)
                        );
                    }
                }
            }
        }

        Ok(result)
    }
}

impl LendingClient<Pubkey, (Vec<(Pubkey, Bank)>, MarginfiGroup)> for MarginfiClient {
    fn load_markets(&mut self) -> Result<(), LendingError> {
        // Fetch the data first
        let (banks, group) = self.fetch_markets()?;

        // Then update the state
        self.banks = banks;
        self.group = group;

        Ok(())
    }

    fn fetch_markets(&self) -> Result<(Vec<(Pubkey, Bank)>, MarginfiGroup), LendingError> {
        // Fetch banks and group in separate calls
        let banks = self.fetch_banks_for_group()?;
        let group = self.fetch_marginfi_group(&self.group_pubkeys[0])?;

        // Return as a tuple
        Ok((banks, group))
    }

    fn set_market_data(&mut self, data: (Vec<(Pubkey, Bank)>, MarginfiGroup)) {
        let (banks, group) = data;
        self.banks = banks;
        self.group = group;
    }

    fn get_user_obligations(
        &self,
        wallet_address: &str,
    ) -> Result<Vec<UserObligation>, LendingError> {
        self.get_user_obligations(wallet_address)
    }

    fn program_id(&self) -> Pubkey {
        self.program_id
    }

    fn protocol_name(&self) -> &'static str {
        "Marginfi"
    }
}
