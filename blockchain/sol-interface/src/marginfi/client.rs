use std::ops::Deref;

use anchor_client::{Client, Program};
use anchor_lang::AnchorDeserialize;
use common::{ObligationType, UserObligation};
use fixed::types::I80F48;
use log::{debug, info};
use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_sdk::{pubkey::Pubkey, signature::Signer};
use std::str::FromStr;

use super::models::{
    account::{Balance, BalanceSide, MarginfiAccount},
    group::{Bank, MarginfiGroup},
};

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

        let program = client.program(marginfi_program_id).expect("Failed to load Kamino program");

        Self {
            program,
            marginfi_program_id,
            group_pubkeys,
            banks: Vec::new(),
            group: MarginfiGroup::default(),
        }
    }

    pub fn load_banks_for_group(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let filters = vec![
            // RpcFilterType::DataSize(size_of::<Bank>() as u64 + 8), // Add 8 for anchor discriminator
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                8 + size_of::<Pubkey>() + size_of::<u8>(),
                self.group_pubkeys[0].to_bytes().to_vec(), //also possible cause there is just one group for now
            )),
        ];

        let accounts = self
            .program
            .rpc()
            .get_program_accounts_with_config(
                &self.marginfi_program_id,
                solana_client::rpc_config::RpcProgramAccountsConfig {
                    filters: Some(filters),
                    account_config: solana_client::rpc_config::RpcAccountInfoConfig {
                        encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .unwrap();

        self.banks = accounts
            .into_iter()
            .filter_map(|(pubkey, account)| {
                Bank::try_from_slice(&account.data[8..]).map(|bank| (pubkey, bank)).ok()
            })
            .collect();

        Ok(())
    }

    pub fn load_marginfi_group(
        &mut self,
        group_pubkey: &Pubkey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let group_account = self.program.rpc().get_account(group_pubkey).unwrap();
        self.group = MarginfiGroup::try_from_slice(&group_account.data[8..]).unwrap();
        Ok(())
    }

    pub fn get_user_obligations(
        &self,
        wallet_pubkey: &str,
    ) -> Result<Vec<UserObligation>, Box<dyn std::error::Error>> {
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
                    protocol_name: "Marginfi".to_string(),
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

    /// Get the obligations for a given owner
    fn get_obligations(
        &self,
        owner_pubkey: &str,
    ) -> Result<Vec<(Balance, Bank)>, Box<dyn std::error::Error>> {
        let mut result = Vec::new();
        let owner = Pubkey::from_str(owner_pubkey)?;

        let filters = vec![
            RpcFilterType::DataSize(2304 + 8), // Size of MarginfiAccount
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                8 + 32, // Skip discriminator (8) and group pubkey (32) to get to authority
                owner.to_bytes().to_vec(),
            )),
        ];

        let accounts = self.program.rpc().get_program_accounts_with_config(
            &self.marginfi_program_id,
            solana_client::rpc_config::RpcProgramAccountsConfig {
                filters: Some(filters),
                account_config: solana_client::rpc_config::RpcAccountInfoConfig {
                    encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
                    ..Default::default()
                },
                ..Default::default()
            },
        )?;

        if accounts.is_empty() {
            debug!("No marginfi accounts found for {}", owner_pubkey);
            return Ok(result);
        }

        info!("\n{} Marginfi Accounts found for {}:", accounts.len(), owner_pubkey);
        for (_, account) in accounts {
            let marginfi_account = MarginfiAccount::try_from_slice(&account.data[8..])?;

            // Print active balances
            for balance in marginfi_account.lending_account.get_active_balances_iter() {
                if !balance.is_empty(BalanceSide::Assets)
                    || !balance.is_empty(BalanceSide::Liabilities)
                {
                    // Get bank account data
                    if let Ok(bank_account) = self.program.rpc().get_account(&balance.bank_pk) {
                        if let Ok(bank) = Bank::try_from_slice(&bank_account.data[8..]) {
                            result.push((balance.clone(), bank));
                        }
                    }
                }
            }
        }

        Ok(result)
    }
}
