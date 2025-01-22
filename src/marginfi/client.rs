use std::ops::Deref;

use anchor_client::{Client, Program};
use anchor_lang::AnchorDeserialize;
use solana_client::rpc_filter::{Memcmp, RpcFilterType};
use solana_sdk::{pubkey::Pubkey, signature::Signer};
use std::str::FromStr;

use super::models::group::{Bank, MarginfiGroup};

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
}
