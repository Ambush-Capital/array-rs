use common::lending::LendingError;
use common_rpc::{with_rpc_client, RpcError, RpcErrorConverter, CONNECTION_POOL};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::time::Duration;

/// Centralized error converter for Lending clients
pub struct LendingErrorConverter;

impl RpcErrorConverter<LendingError> for LendingErrorConverter {
    fn convert_error(error: RpcError) -> LendingError {
        match error {
            RpcError::RpcError(e) => LendingError::RpcError(e),
            RpcError::DeserializationError(e) => LendingError::DeserializationError(e),
            RpcError::InvalidAddress(e) => LendingError::InvalidAddress(e.to_string()),
            RpcError::AccountNotFound(e) => LendingError::AccountNotFound(e),
        }
    }
}

/// Helper function to create an RPC client with the given URL
/// Uses the connection pool for better performance
pub fn create_rpc_client(rpc_url: &str) -> RpcClient {
    CONNECTION_POOL.get_client(rpc_url)
}

/// Helper function to create an RPC client with the given URL and timeout
/// Uses the connection pool for better performance
pub fn create_rpc_client_with_timeout(rpc_url: &str, timeout: Duration) -> RpcClient {
    // For custom timeouts, we still create a new client
    // This is because the pool uses a fixed timeout
    RpcClient::new_with_timeout(rpc_url.to_string(), timeout)
}

/// Helper function to execute a function with an RPC client and automatically return it to the pool
pub fn with_pooled_client<F, R>(rpc_url: &str, f: F) -> R
where
    F: FnOnce(&RpcClient) -> R,
{
    with_rpc_client(rpc_url, f)
}

/// Helper function to format a pubkey for error messages
pub fn format_pubkey_for_error(pubkey: &Pubkey) -> String {
    format!("{} ({:.8})", pubkey, pubkey)
}
