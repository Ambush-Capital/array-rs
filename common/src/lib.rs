use serde::{Deserialize, Serialize};

pub mod asset_utils;
pub mod lending;
pub mod rpc;
pub use lending::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LendingReserve {
    pub protocol_name: String,
    pub market_name: String,
    pub total_supply: u128,
    pub total_borrows: u128,

    pub borrow_rate: u128,
    pub supply_rate: u128,
    pub borrow_apy: u128,
    pub supply_apy: u128,

    pub slot: u64,

    // i think we need to know the collateral assets available for each reserve
    pub collateral_assets: Vec<MintAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintAsset {
    pub name: String,
    pub symbol: String,
    pub market_price_sf: u64,
    pub mint: String,
    pub lending_reserves: Vec<LendingReserve>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub owner: String,
    pub obligations: Vec<UserObligation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObligationType {
    Asset,     // Deposit
    Liability, // Loan
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserObligation {
    pub symbol: String,
    pub mint: String,
    pub mint_decimals: u32,
    pub amount: u64,
    pub protocol_name: String,
    pub market_name: String,
    pub obligation_type: ObligationType,
}

/// Represents a token balance for a specific wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBalance {
    /// Token symbol (e.g., "USDC")
    pub symbol: String,
    /// Mint address of the token
    pub mint: String,
    /// Raw token amount (needs to be divided by 10^decimals for human-readable form)
    pub amount: u64,
    /// Number of decimal places for this token
    pub decimals: u8,
    /// Token account address that holds this balance
    pub token_account: String,
}
