use solana_sdk::pubkey::Pubkey;

#[derive(Debug, Clone)]
pub struct LendingReserve {
    pub protocol_name: String,
    pub market_name: String,
    pub total_supply: u128,
    pub total_borrows: u128,

    pub borrow_rate: u128, //todo make this a Decimal object
    pub supply_rate: u128,
    pub borrow_apy: u128, //these are slot adjusted
    pub supply_apy: u128, //these are slot adjusted

    // i think we need to know the collateral assets available for each reserve
    pub collateral_assets: Vec<MintAsset>,
}

#[derive(Debug, Clone)]
pub struct MintAsset {
    pub name: String,
    pub symbol: String,
    pub market_price_sf: u64,
    pub mint: Pubkey,
    pub lending_reserves: Vec<LendingReserve>,
}

#[derive(Debug, Clone)]
pub struct User {
    pub owner: Pubkey,
    pub obligations: Vec<UserObligation>,
}

#[derive(Debug, Clone)]
pub enum ObligationType {
    Asset,     // Deposit
    Liability, // Loan
}

#[derive(Debug, Clone)]
pub struct UserObligation {
    pub symbol: String,
    pub market_price_sf: u64,
    pub mint: Pubkey,
    pub mint_decimals: u32,
    pub amount: u64,
    pub protocol_name: String,
    pub market_name: String,
    pub obligation_type: ObligationType,
}
