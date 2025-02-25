use solana_account_decoder::UiAccountEncoding;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("RPC error: {0}")]
    RpcError(#[from] Box<dyn std::error::Error>),

    #[error("Account deserialization error: {0}")]
    DeserializationError(String),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    #[error("Account not found: {0}")]
    AccountNotFound(String),
}

/// Trait for converting RpcError to other error types
pub trait RpcErrorConverter<E> {
    /// Convert an RpcError to another error type
    fn convert_error(error: RpcError) -> E;
}

/// Builder for Solana RPC calls to simplify common patterns
pub struct SolanaRpcBuilder<'a> {
    rpc_client: &'a RpcClient,
    program_id: Pubkey,
    filters: Vec<RpcFilterType>,
    encoding: Option<UiAccountEncoding>,
    with_context: Option<bool>,
}

impl<'a> SolanaRpcBuilder<'a> {
    /// Create a new RPC builder with the given RPC client and program ID
    pub fn new(rpc_client: &'a RpcClient, program_id: Pubkey) -> Self {
        Self {
            rpc_client,
            program_id,
            filters: Vec::new(),
            encoding: Some(UiAccountEncoding::Base64),
            with_context: None,
        }
    }

    /// Create a new RPC builder with the given RPC URL and program ID
    /// This is a convenience function that creates a new RPC client and returns a builder
    /// Note: This function is not recommended for performance-critical code as it creates a new client
    pub fn new_from_url(
        rpc_url: &str,
        program_id_str: &str,
    ) -> Result<impl Fn() -> Result<Vec<(Pubkey, Account)>, RpcError>, RpcError> {
        let program_id = Pubkey::from_str(program_id_str)
            .map_err(|e| RpcError::InvalidAddress(e.to_string()))?;

        let rpc_client = RpcClient::new(rpc_url.to_string());

        Ok(move || {
            let config = RpcProgramAccountsConfig {
                filters: None,
                account_config: RpcAccountInfoConfig {
                    encoding: Some(UiAccountEncoding::Base64),
                    ..Default::default()
                },
                with_context: None,
            };

            rpc_client
                .get_program_accounts_with_config(&program_id, config)
                .map_err(|e| RpcError::RpcError(Box::new(e)))
        })
    }

    /// Set a different program ID to query
    pub fn with_program_id(mut self, program_id: Pubkey) -> Self {
        self.program_id = program_id;
        self
    }

    /// Add a data size filter
    pub fn with_data_size(mut self, size: u64) -> Self {
        self.filters.push(RpcFilterType::DataSize(size));
        self
    }

    /// Add a data size filter with an additional offset (e.g., for anchor discriminator)
    pub fn with_data_size_with_offset(mut self, size: u64, offset: u64) -> Self {
        self.filters.push(RpcFilterType::DataSize(size + offset));
        self
    }

    /// Add a raw bytes memcmp filter
    pub fn with_memcmp(mut self, offset: usize, bytes: Vec<u8>) -> Self {
        self.filters.push(RpcFilterType::Memcmp(Memcmp::new_raw_bytes(offset, bytes)));
        self
    }

    /// Add a base58-encoded memcmp filter
    pub fn with_memcmp_base58(mut self, offset: usize, base58_str: String) -> Self {
        self.filters.push(RpcFilterType::Memcmp(Memcmp::new(
            offset,
            MemcmpEncodedBytes::Base58(base58_str),
        )));
        self
    }

    /// Add a pubkey memcmp filter
    pub fn with_memcmp_pubkey(mut self, offset: usize, pubkey: &Pubkey) -> Self {
        self.filters
            .push(RpcFilterType::Memcmp(Memcmp::new_raw_bytes(offset, pubkey.to_bytes().to_vec())));
        self
    }

    /// Set the encoding for the response
    pub fn with_encoding(mut self, encoding: UiAccountEncoding) -> Self {
        self.encoding = Some(encoding);
        self
    }

    /// Set whether to include context in the response
    pub fn with_context(mut self, with_context: bool) -> Self {
        self.with_context = Some(with_context);
        self
    }

    /// Optimize the order of filters for better RPC performance
    ///
    /// This method sorts filters by their restrictiveness, putting the most
    /// restrictive filters first to minimize the amount of data processed.
    /// The order is:
    /// 1. Discriminator filters (memcmp at offset 0)
    /// 2. Data size filters
    /// 3. Owner filters (memcmp at offset 32)
    /// 4. Other memcmp filters
    /// 5. Any other filters
    pub fn optimize_filters(mut self) -> Self {
        // Sort filters by their restrictiveness
        self.filters.sort_by(|a, b| {
            // Order: Discriminator > DataSize > Memcmp(owner) > Other Memcmp
            let filter_priority = |filter: &RpcFilterType| -> u8 {
                match filter {
                    // This is a bit of a hack to get the discriminator filter to the front of the list
                    RpcFilterType::Memcmp(memcmp) => {
                        // Convert to string to check the offset
                        let memcmp_str = format!("{:?}", memcmp);
                        if memcmp_str.contains("offset: 0,") {
                            0 // Discriminator (most restrictive)
                        } else if memcmp_str.contains("offset: 32,") {
                            2 // Owner field
                        } else {
                            3 // Other memcmp
                        }
                    }
                    RpcFilterType::DataSize(_) => 1, // Data size
                    _ => 4,                          // Other filters
                }
            };

            filter_priority(a).cmp(&filter_priority(b))
        });

        self
    }

    /// Get program accounts
    pub fn get_program_accounts(self) -> Result<Vec<(Pubkey, Account)>, RpcError> {
        let config = RpcProgramAccountsConfig {
            filters: if self.filters.is_empty() { None } else { Some(self.filters) },
            account_config: RpcAccountInfoConfig { encoding: self.encoding, ..Default::default() },
            with_context: self.with_context,
        };

        self.rpc_client
            .get_program_accounts_with_config(&self.program_id, config)
            .map_err(|e| RpcError::RpcError(Box::new(e)))
    }

    /// Get program accounts with automatic error conversion
    pub fn get_program_accounts_with_conversion<E, C: RpcErrorConverter<E>>(
        self,
    ) -> Result<Vec<(Pubkey, Account)>, E> {
        self.get_program_accounts().map_err(C::convert_error)
    }

    /// Get a single account by pubkey
    pub fn get_account(self, pubkey: &Pubkey) -> Result<Account, RpcError> {
        self.rpc_client.get_account(pubkey).map_err(|e| RpcError::RpcError(Box::new(e)))
    }

    /// Get a single account by pubkey with automatic error conversion
    pub fn get_account_with_conversion<E, C: RpcErrorConverter<E>>(
        self,
        pubkey: &Pubkey,
    ) -> Result<Account, E> {
        self.get_account(pubkey).map_err(C::convert_error)
    }
}
