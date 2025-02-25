// use solana_account_decoder::UiAccountEncoding;
// use solana_client::rpc_client::RpcClient;
// use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
// use solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
// use solana_sdk::account::Account;
// use solana_sdk::pubkey::Pubkey;
// use std::str::FromStr;

// use crate::lending::LendingError;

// /// Builder for Solana RPC calls to simplify common patterns
// pub struct SolanaRpcBuilder {
//     rpc_client: RpcClient,
//     program_id: Pubkey,
//     filters: Vec<RpcFilterType>,
//     encoding: Option<UiAccountEncoding>,
//     with_context: Option<bool>,
// }

// impl SolanaRpcBuilder {
//     /// Create a new RPC builder with the given RPC client and program ID
//     pub fn new(rpc_client: RpcClient, program_id: Pubkey) -> Self {
//         Self {
//             rpc_client,
//             program_id,
//             filters: Vec::new(),
//             encoding: Some(UiAccountEncoding::Base64),
//             with_context: None,
//         }
//     }

//     /// Create a new RPC builder with the given RPC URL and program ID
//     pub fn new_from_url(rpc_url: &str, program_id_str: &str) -> Result<Self, LendingError> {
//         let rpc_client = RpcClient::new(rpc_url.to_string());
//         let program_id = Pubkey::from_str(program_id_str)
//             .map_err(|e| LendingError::InvalidAddress(e.to_string()))?;

//         Ok(Self::new(rpc_client, program_id))
//     }

//     /// Set a different program ID to query
//     pub fn with_program_id(mut self, program_id: Pubkey) -> Self {
//         self.program_id = program_id;
//         self
//     }

//     /// Add a data size filter
//     pub fn with_data_size(mut self, size: u64) -> Self {
//         self.filters.push(RpcFilterType::DataSize(size));
//         self
//     }

//     /// Add a data size filter with an additional offset (e.g., for anchor discriminator)
//     pub fn with_data_size_with_offset(mut self, size: u64, offset: u64) -> Self {
//         self.filters.push(RpcFilterType::DataSize(size + offset));
//         self
//     }

//     /// Add a raw bytes memcmp filter
//     pub fn with_memcmp(mut self, offset: usize, bytes: Vec<u8>) -> Self {
//         self.filters.push(RpcFilterType::Memcmp(Memcmp::new_raw_bytes(offset, bytes)));
//         self
//     }

//     /// Add a base58-encoded memcmp filter
//     pub fn with_memcmp_base58(mut self, offset: usize, base58_str: String) -> Self {
//         self.filters.push(RpcFilterType::Memcmp(Memcmp::new(
//             offset,
//             MemcmpEncodedBytes::Base58(base58_str),
//         )));
//         self
//     }

//     /// Add a pubkey memcmp filter
//     pub fn with_memcmp_pubkey(mut self, offset: usize, pubkey: &Pubkey) -> Self {
//         self.filters
//             .push(RpcFilterType::Memcmp(Memcmp::new_raw_bytes(offset, pubkey.to_bytes().to_vec())));
//         self
//     }

//     /// Set the encoding for the response
//     pub fn with_encoding(mut self, encoding: UiAccountEncoding) -> Self {
//         self.encoding = Some(encoding);
//         self
//     }

//     /// Set whether to include context in the response
//     pub fn with_context(mut self, with_context: bool) -> Self {
//         self.with_context = Some(with_context);
//         self
//     }

//     /// Get program accounts
//     pub fn get_program_accounts(self) -> Result<Vec<(Pubkey, Account)>, LendingError> {
//         let config = RpcProgramAccountsConfig {
//             filters: if self.filters.is_empty() { None } else { Some(self.filters) },
//             account_config: RpcAccountInfoConfig { encoding: self.encoding, ..Default::default() },
//             with_context: self.with_context,
//         };

//         self.rpc_client
//             .get_program_accounts_with_config(&self.program_id, config)
//             .map_err(|e| LendingError::RpcError(Box::new(e)))
//     }

//     /// Get a single account by pubkey
//     pub fn get_account(self, pubkey: &Pubkey) -> Result<Account, LendingError> {
//         self.rpc_client.get_account(pubkey).map_err(|e| LendingError::RpcError(Box::new(e)))
//     }
// }

// /// Utility to extract and print discriminators
// pub fn get_discriminator(account_data: &[u8]) -> [u8; 8] {
//     let mut discriminator = [0u8; 8];
//     discriminator.copy_from_slice(&account_data[0..8]);
//     discriminator
// }

// /// Utility to print a discriminator for debugging
// pub fn print_discriminator(discriminator: [u8; 8], type_name: &str) {
//     println!("Discriminator for {}: {:?}", type_name, discriminator);
// }
