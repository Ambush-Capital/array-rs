use crate::RpcError;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{account::Account, pubkey::Pubkey};
use std::collections::HashMap;

/// Default batch size for RPC requests
pub const DEFAULT_BATCH_SIZE: usize = 100;

/// Fetch multiple accounts in batches to avoid RPC request size limits
///
/// This function splits the pubkeys into smaller batches and makes multiple
/// RPC calls if necessary, combining the results into a single HashMap.
pub fn get_multiple_accounts_batched(
    client: &RpcClient,
    pubkeys: &[Pubkey],
    batch_size: usize,
) -> Result<HashMap<Pubkey, Account>, RpcError> {
    let mut accounts = HashMap::with_capacity(pubkeys.len());

    // Process in batches to avoid RPC request size limits
    for chunk in pubkeys.chunks(batch_size) {
        let batch_accounts =
            client.get_multiple_accounts(chunk).map_err(|e| RpcError::RpcError(Box::new(e)))?;

        for (i, account_option) in batch_accounts.into_iter().enumerate() {
            if let Some(account) = account_option {
                accounts.insert(chunk[i], account);
            }
        }
    }

    Ok(accounts)
}

/// Fetch multiple accounts in batches with a default batch size
pub fn get_multiple_accounts(
    client: &RpcClient,
    pubkeys: &[Pubkey],
) -> Result<HashMap<Pubkey, Account>, RpcError> {
    get_multiple_accounts_batched(client, pubkeys, DEFAULT_BATCH_SIZE)
}

/// Fetch multiple accounts in batches and convert the error type
pub fn get_multiple_accounts_with_conversion<E, C>(
    client: &RpcClient,
    pubkeys: &[Pubkey],
) -> Result<HashMap<Pubkey, Account>, E>
where
    C: crate::RpcErrorConverter<E>,
{
    get_multiple_accounts(client, pubkeys).map_err(C::convert_error)
}
