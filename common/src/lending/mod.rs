use crate::UserObligation;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LendingError {
    #[error("RPC error: {0}")]
    RpcError(#[from] Box<dyn std::error::Error>),

    #[error("Account deserialization error: {0}")]
    DeserializationError(String),

    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    #[error("Market not found: {0}")]
    MarketNotFound(String),

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),
}

pub trait LendingClient<Address, MarketData> {
    /// Loads markets into the client's internal state
    fn load_markets(&mut self) -> Result<(), LendingError> {
        // Default implementation that can be overridden
        Ok(())
    }

    /// Fetches markets without modifying the client's state
    /// Returns the market data that can be used to update the client's state
    fn fetch_markets(&self) -> Result<MarketData, LendingError>;

    /// Updates the client's state with the fetched market data
    fn set_market_data(&mut self, data: MarketData);

    fn get_user_obligations(
        &self,
        wallet_address: &str,
    ) -> Result<Vec<UserObligation>, LendingError>;

    fn program_id(&self) -> Address;

    fn protocol_name(&self) -> &'static str;

    fn print_markets(&self) {
        // Default empty implementation
    }
}
