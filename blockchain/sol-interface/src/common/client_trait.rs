use common::UserObligation;
use solana_sdk::pubkey::Pubkey;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("RPC error: {0}")]
    RpcError(#[from] Box<solana_client::client_error::ClientError>),

    #[error("Account deserialization error: {0}")]
    DeserializationError(String),

    #[error("Invalid pubkey: {0}")]
    InvalidPubkey(String),

    #[error("Market not found: {0}")]
    MarketNotFound(String),

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),
}

pub trait LendingClient {
    fn load_markets(&mut self) -> Result<(), ClientError>;
    fn get_user_obligations(&self, wallet_pubkey: &str)
        -> Result<Vec<UserObligation>, ClientError>;
    fn program_id(&self) -> Pubkey;
    fn protocol_name(&self) -> &'static str;
    fn print_markets(&self) {
        // Default empty implementation
    }
}
