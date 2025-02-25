use std::mem::size_of;
use std::ops::Deref;

use crate::common::rpc_utils::{
    format_pubkey_for_error, with_pooled_client, LendingErrorConverter,
};
use anchor_client::{Client, Program};
use anchor_lang::AnchorDeserialize;
use common::{
    lending::{LendingClient, LendingError},
    ObligationType, UserObligation,
};
use common_rpc::SolanaRpcBuilder;
use fixed::types::I80F48;
use log::debug;
use solana_sdk::{pubkey::Pubkey, signature::Signer};
use std::str::FromStr;

use super::models::{
    account::{Balance, BalanceSide, MarginfiAccount},
    group::{Bank, MarginfiGroup},
};

// Define discriminators as constants
const MARGINFI_ACCOUNT_DISCRIMINATOR: [u8; 8] = [67, 178, 130, 109, 126, 114, 28, 42];
const MARGINFI_BANK_DISCRIMINATOR: [u8; 8] = [142, 49, 166, 242, 50, 66, 97, 188];

pub struct MarginfiClient<C> {
    program: Program<C>,
    marginfi_program_id: Pubkey,
    group_pubkeys: Vec<Pubkey>,
    pub banks: Vec<(Pubkey, Bank)>,
    pub group: MarginfiGroup, //well keep this simple for now since we are just loading a single group
}

impl<C: Clone + Deref<Target = impl Signer>> MarginfiClient<C> {
    pub fn new(client: &Client<C>) -> Self {
        let marginfi_program_id = Pubkey::from_str("MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA")
            .expect("Invalid Marginfi Lending Program ID");

        let group_pubkeys: Vec<Pubkey> = vec![
            "4qp6Fx6tnZkY5Wropq9wUYgtFxXKwE6viZxFHg3rdAG8".parse().unwrap(), //main market
        ];

        let program = client.program(marginfi_program_id).expect("Failed to load Marginfi program");

        Self {
            program,
            marginfi_program_id,
            group_pubkeys,
            banks: Vec::new(),
            group: MarginfiGroup::default(),
        }
    }

    /// Get the RPC URL from the program
    fn get_rpc_url(&self) -> String {
        self.program.rpc().url().to_string()
    }

    pub fn load_banks_for_group(&mut self) -> Result<(), LendingError> {
        // Get the RPC URL
        let rpc_url = self.get_rpc_url();

        // Use the RPC builder with optimized filters
        let accounts = with_pooled_client(&rpc_url, |client| {
            SolanaRpcBuilder::new(client, self.marginfi_program_id)
                .with_memcmp(0, MARGINFI_BANK_DISCRIMINATOR.to_vec())
                .with_memcmp_pubkey(
                    8 + size_of::<Pubkey>() + size_of::<u8>(),
                    &self.group_pubkeys[0],
                )
                .optimize_filters() // Apply filter optimization
                .get_program_accounts_with_conversion::<LendingError, LendingErrorConverter>()
        })?;

        // Pre-allocate with capacity
        self.banks = Vec::with_capacity(accounts.len());

        for (pubkey, account) in accounts {
            match Bank::try_from_slice(&account.data[8..]) {
                Ok(bank) => self.banks.push((pubkey, bank)),
                Err(e) => {
                    debug!(
                        "Failed to deserialize bank {}: {}",
                        format_pubkey_for_error(&pubkey),
                        e
                    );
                }
            }
        }

        Ok(())
    }

    pub fn load_marginfi_group(&mut self, group_pubkey: &Pubkey) -> Result<(), LendingError> {
        // Get the RPC URL
        let rpc_url = self.get_rpc_url();

        // Use the RPC builder to get the group account
        let group_account = with_pooled_client(&rpc_url, |client| {
            SolanaRpcBuilder::new(client, self.marginfi_program_id)
                .get_account_with_conversion::<LendingError, LendingErrorConverter>(group_pubkey)
        })?;

        self.group = MarginfiGroup::try_from_slice(&group_account.data[8..]).map_err(|e| {
            LendingError::DeserializationError(format!(
                "Failed to deserialize marginfi group {}: {}",
                format_pubkey_for_error(group_pubkey),
                e
            ))
        })?;

        Ok(())
    }

    pub fn get_user_obligations(
        &self,
        wallet_pubkey: &str,
    ) -> Result<Vec<UserObligation>, LendingError> {
        let mut obligations = Vec::new();
        let marginfi_accounts = self.get_obligations(wallet_pubkey)?;

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

                obligations.push(UserObligation {
                    symbol: "".to_string(),
                    mint: bank.mint.to_string(),
                    mint_decimals: bank.mint_decimals as u32,
                    amount: I80F48::to_num(amount),
                    protocol_name: self.protocol_name().to_string(),
                    market_name: "General".to_string(),
                    obligation_type: match side {
                        BalanceSide::Assets => ObligationType::Asset,
                        BalanceSide::Liabilities => ObligationType::Liability,
                    },
                });
            }
        }

        Ok(obligations)
    }

    fn get_obligations(&self, owner_pubkey: &str) -> Result<Vec<(Balance, Bank)>, LendingError> {
        let owner = Pubkey::from_str(owner_pubkey).map_err(|e| {
            LendingError::InvalidAddress(format!("Invalid owner pubkey {}: {}", owner_pubkey, e))
        })?;

        // Get the RPC URL
        let rpc_url = self.get_rpc_url();

        // Use the RPC builder with optimized filters
        let accounts = with_pooled_client(&rpc_url, |client| {
            SolanaRpcBuilder::new(client, self.marginfi_program_id)
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
        let bank_accounts = with_pooled_client(&rpc_url, |client| {
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

impl<C: Clone + Deref<Target = impl Signer>> LendingClient<Pubkey> for MarginfiClient<C> {
    fn load_markets(&mut self) -> Result<(), LendingError> {
        self.load_banks_for_group()?;
        Ok(())
    }

    fn get_user_obligations(
        &self,
        wallet_address: &str,
    ) -> Result<Vec<UserObligation>, LendingError> {
        self.get_user_obligations(wallet_address)
    }

    fn program_id(&self) -> Pubkey {
        self.marginfi_program_id
    }

    fn protocol_name(&self) -> &'static str {
        "Marginfi"
    }
}
