use common::UserObligation;
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

    #[error("Other error: {0}")]
    Other(String),
}

pub trait LendingClient<T> {
    /// The type of market data returned by fetch_markets
    type MarketData;

    fn load_markets(&mut self) -> Result<(), ClientError>;

    /// Fetches markets without modifying the client's state
    /// Returns the market data that can be used to update the client's state
    fn fetch_markets(&self) -> Result<Self::MarketData, ClientError>;

    /// Updates the client's state with the fetched market data
    fn set_market_data(&mut self, data: Self::MarketData);

    fn get_user_obligations(&self, wallet_pubkey: &str)
        -> Result<Vec<UserObligation>, ClientError>;
    fn program_id(&self) -> T;
    fn protocol_name(&self) -> &'static str;
    fn print_markets(&self) {
        // Default empty implementation
    }
}
