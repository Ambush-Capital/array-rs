use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post, put},
    Router,
};
use common::{LendingReserve, MintAsset, ObligationType, UserObligation};
use log::{debug, error, info};
use serde::Serialize;
use sqlx::{Pool, Sqlite};

fn format_rate(rate: u128) -> String {
    let rate_f64 = (rate as f64) / 1e19;
    format!("{:.10}", rate_f64).trim_end_matches('0').trim_end_matches('.').to_string()
}

#[derive(Serialize)]
pub struct ApiLendingReserve {
    pub protocol_name: String,
    pub market_name: String,
    #[serde(serialize_with = "serialize_token_amount")]
    pub total_supply: u128,
    #[serde(serialize_with = "serialize_token_amount")]
    pub total_borrows: u128,
    #[serde(skip_serializing)]
    pub borrow_rate: String,
    pub supply_rate: String,
    #[serde(serialize_with = "serialize_rate")]
    pub supply_rate_7d: f64,
    #[serde(serialize_with = "serialize_rate")]
    pub supply_rate_30d: f64,
    #[serde(skip_serializing)]
    pub borrow_apy: String,
    #[serde(skip_serializing)]
    pub supply_apy: String,
    #[serde(skip_serializing)]
    pub slot: u64,
}

impl From<LendingReserve> for ApiLendingReserve {
    fn from(reserve: LendingReserve) -> Self {
        Self {
            protocol_name: reserve.protocol_name,
            market_name: reserve.market_name,
            total_supply: reserve.total_supply,
            total_borrows: reserve.total_borrows,
            borrow_rate: format_rate(reserve.borrow_rate),
            supply_rate: format_rate(reserve.supply_rate),
            borrow_apy: format_rate(reserve.borrow_apy),
            supply_apy: format_rate(reserve.supply_apy),
            slot: reserve.slot,
            supply_rate_30d: 0.0,
            supply_rate_7d: 0.0,
        }
    }
}

#[derive(Serialize)]
pub struct ApiMintAsset {
    pub name: String,
    pub symbol: String,
    pub market_price_sf: u64,
    pub mint: String,
    pub lending_reserves: Vec<ApiLendingReserve>,
}

impl From<MintAsset> for ApiMintAsset {
    fn from(asset: MintAsset) -> Self {
        Self {
            name: asset.name,
            symbol: asset.symbol,
            market_price_sf: asset.market_price_sf,
            mint: asset.mint,
            lending_reserves: asset
                .lending_reserves
                .into_iter()
                .map(ApiLendingReserve::from)
                .collect(),
        }
    }
}

#[derive(Serialize)]
pub struct ApiUserObligation {
    pub symbol: String,
    #[serde(skip)]
    pub mint: String,
    pub protocol_name: String,
    pub market_name: String,
    #[serde(serialize_with = "serialize_dollar_amount")]
    pub amount: (u64, u32), // (amount, mint_decimals)
    pub obligation_type: String,
}

impl From<UserObligation> for ApiUserObligation {
    fn from(obligation: UserObligation) -> Self {
        let obligation_type = match obligation.obligation_type {
            ObligationType::Asset => "Supply",
            ObligationType::Liability => "Borrow",
        };

        Self {
            symbol: obligation.symbol,
            mint: obligation.mint,
            protocol_name: obligation.protocol_name,
            market_name: obligation.market_name,
            amount: (obligation.amount, obligation.mint_decimals),
            obligation_type: obligation_type.to_string(),
        }
    }
}

#[derive(Clone)]
pub struct ApiService {
    db_pool: Pool<Sqlite>,
    client: reqwest::Client,
}

impl ApiService {
    pub async fn new(db_url: &str) -> Result<Self> {
        info!("Initializing ApiService with database URL: {}", db_url);
        // Create connection pool
        let pool = sqlx::Pool::<Sqlite>::connect(db_url).await?;
        let client = reqwest::Client::new();
        info!("ApiService initialized successfully");

        Ok(Self { db_pool: pool, client })
    }

    pub async fn get_current_markets(&self) -> Result<Vec<ApiMintAsset>> {
        debug!("Fetching current markets from chain-api");
        // Forward request to chain-api
        let response =
            self.client.get("http://localhost:3000/current_lending_markets").send().await?;

        let markets = response
            .json::<Vec<MintAsset>>()
            .await?
            .into_iter()
            .map(ApiMintAsset::from)
            .collect::<Vec<ApiMintAsset>>();

        // Get historical market data to populate 7d and 30d averages
        let historical_markets = self.get_historical_markets().await?;

        // Update markets with historical rate data
        let markets: Vec<ApiMintAsset> = markets
            .into_iter()
            .map(|mut market| {
                // Update each lending reserve with historical rates if available
                market.lending_reserves = market
                    .lending_reserves
                    .into_iter()
                    .map(|mut reserve| {
                        // Find matching historical data
                        if let Some(historical) = historical_markets.iter().find(|h| {
                            h.protocol_name == reserve.protocol_name
                                && h.market_name == reserve.market_name
                                && h.token_mint == market.mint
                        }) {
                            reserve.supply_rate_7d = historical.supply_rate_7d;
                            reserve.supply_rate_30d = historical.supply_rate_30d;
                        }
                        reserve
                    })
                    .collect();
                market
            })
            .collect();

        info!("Retrieved {} current markets from chain-api", markets.len());
        Ok(markets)
    }

    pub async fn get_historical_markets(&self) -> Result<Vec<HistoricalMarketDataAverage>> {
        debug!("Fetching historical markets from database");
        // Query average supply rates for 7 and 30 day periods
        let markets = sqlx::query_as::<_, HistoricalMarketDataAverage>(
            r#"
            WITH recent_data AS (
                SELECT 
                    protocol_name,
                    market_name,
                    token_mint,
                    supply_rate,
                    timestamp,
                    token_name,
                    token_symbol
                FROM lending_markets
                WHERE timestamp >= datetime('now', '-30 days')
            ),
            averages AS (
                SELECT 
                    protocol_name,
                    market_name,
                    token_mint,
                    token_name,
                    token_symbol,
                    AVG(CAST(supply_rate AS FLOAT)) as supply_rate_30d,
                    AVG(CASE 
                        WHEN timestamp >= datetime('now', '-7 days') 
                        THEN CAST(supply_rate AS FLOAT) 
                    END) as supply_rate_7d
                FROM recent_data
                GROUP BY protocol_name, market_name, token_mint
            )
            SELECT 
                protocol_name,
                market_name,
                token_name,
                token_symbol,
                token_mint,
                supply_rate_7d,
                supply_rate_30d
            FROM averages
            ORDER BY protocol_name, market_name
            "#,
        )
        .fetch_all(&self.db_pool)
        .await?;

        info!("Retrieved market averages for {} markets", markets.len());
        Ok(markets)
    }

    pub async fn get_user_obligations(&self, pubkey: &str) -> Result<Vec<ApiUserObligation>> {
        debug!("Fetching user obligations from chain-api for pubkey: {}", pubkey);

        // Forward request to chain-api
        let url = format!("http://localhost:3000/obligations/{}", pubkey);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            error!("Failed to fetch obligations: HTTP {}", response.status());
            return Err(anyhow::anyhow!("Failed to fetch obligations: HTTP {}", response.status()));
        }

        let obligations = response
            .json::<Vec<UserObligation>>()
            .await?
            .into_iter()
            .map(ApiUserObligation::from)
            .collect::<Vec<ApiUserObligation>>();

        info!("Retrieved {} obligations for pubkey {}", obligations.len(), pubkey);
        Ok(obligations)
    }

    pub async fn get_wallet_balances(&self, pubkey: &str) -> Result<Vec<common::TokenBalance>> {
        debug!("Fetching wallet balances from chain-api for pubkey: {}", pubkey);

        // Forward request to chain-api
        let url = format!("http://localhost:3000/wallet_balance/{}", pubkey);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            error!("Failed to fetch wallet balances: HTTP {}", response.status());
            return Err(anyhow::anyhow!(
                "Failed to fetch wallet balances: HTTP {}",
                response.status()
            ));
        }

        let balances = response.json::<Vec<common::TokenBalance>>().await?;
        info!("Retrieved {} token balances for pubkey {}", balances.len(), pubkey);
        Ok(balances)
    }

    pub async fn get_wallet_data(&self, pubkey: &str) -> Result<WalletData> {
        // Get both wallet balances and positions in parallel
        let (balances, positions) =
            tokio::join!(self.get_wallet_balances(pubkey), self.get_user_obligations(pubkey));

        // Convert common::TokenBalance to ApiTokenBalance
        let api_balances =
            balances?.into_iter().map(ApiTokenBalance::from).collect::<Vec<ApiTokenBalance>>();

        Ok(WalletData { wallet_balances: api_balances, wallet_positions: positions? })
    }
}

#[derive(serde::Serialize, sqlx::FromRow)]
pub struct HistoricalMarketData {
    pub protocol_name: String,
    pub market_name: String,
    pub token_name: String,
    pub token_symbol: String,
    pub token_mint: String,
    pub market_price: i64,
    pub total_supply: i64,
    pub total_borrows: i64,
    #[serde(serialize_with = "serialize_rate")]
    pub borrow_rate: f64,
    #[serde(serialize_with = "serialize_rate")]
    pub supply_rate: f64,
    #[serde(serialize_with = "serialize_rate")]
    pub supply_rate_7d: f64,
    #[serde(serialize_with = "serialize_rate")]
    pub supply_rate_30d: f64,
    #[serde(serialize_with = "serialize_rate")]
    pub borrow_apy: f64,
    #[serde(serialize_with = "serialize_rate")]
    pub supply_apy: f64,
    pub slot: u64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(serde::Serialize, sqlx::FromRow)]
pub struct HistoricalMarketDataAverage {
    pub protocol_name: String,
    pub market_name: String,
    pub token_name: String,
    pub token_symbol: String,
    pub token_mint: String,
    #[serde(serialize_with = "serialize_rate")]
    pub supply_rate_7d: f64,
    #[serde(serialize_with = "serialize_rate")]
    pub supply_rate_30d: f64,
}

fn serialize_rate<S>(rate: &f64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let rate_val = *rate / 1e19;

    // Convert to string with appropriate precision and no trailing zeros
    let formatted =
        format!("{:.10}", rate_val).trim_end_matches('0').trim_end_matches('.').to_string();

    serializer.serialize_str(&formatted)
}

fn serialize_token_amount<S>(amount: &u128, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let amount_f64 = *amount as f64 / 1_000_000_000_000_000_000_000_000.0;
    serializer.serialize_str(&amount_f64.to_string())
}

fn serialize_dollar_amount<S>(amount_data: &(u64, u32), serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let (amount, mint_decimals) = amount_data;
    let amount_f64 = *amount as f64 / 10_f64.powi(*mint_decimals as i32);
    serializer.serialize_str(&format!("{:.4}", amount_f64))
}

pub async fn create_router(service: ApiService) -> Router {
    Router::new()
        .route("/current_markets", get(get_current_markets))
        .route("/historical_markets", get(get_historical_markets))
        .route("/wallet/{pubkey}", get(get_wallet_data))
        .route("/user_obligations/{pubkey}", get(get_user_obligations))
        .route("/user", post(create_user))
        .route("/user/{wallet_address}", get(get_user))
        .route("/user/{wallet_address}", put(update_user))
        .with_state(service)
}

async fn get_current_markets(
    State(service): State<ApiService>,
) -> (StatusCode, Json<Vec<ApiMintAsset>>) {
    match service.get_current_markets().await {
        Ok(markets) => {
            info!("Successfully returned {} current markets", markets.len());
            (StatusCode::OK, Json(markets))
        }
        Err(e) => {
            error!("Error fetching current markets: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(vec![]))
        }
    }
}

async fn get_historical_markets(
    State(service): State<ApiService>,
) -> (StatusCode, Json<Vec<HistoricalMarketDataAverage>>) {
    match service.get_historical_markets().await {
        Ok(markets) => {
            info!("Successfully returned {} historical market entries", markets.len());
            (StatusCode::OK, Json(markets))
        }
        Err(e) => {
            error!("Error fetching historical markets: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(vec![]))
        }
    }
}

async fn get_wallet_data(
    State(service): State<ApiService>,
    Path(pubkey): Path<String>,
) -> (StatusCode, Json<WalletData>) {
    match service.get_wallet_data(&pubkey).await {
        Ok(wallet_data) => {
            info!(
                "Successfully returned wallet data for pubkey {}: {} balances, {} positions",
                pubkey,
                wallet_data.wallet_balances.len(),
                wallet_data.wallet_positions.len()
            );
            (StatusCode::OK, Json(wallet_data))
        }
        Err(e) => {
            error!("Error fetching wallet data for pubkey {}: {}", pubkey, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WalletData { wallet_balances: vec![], wallet_positions: vec![] }),
            )
        }
    }
}

async fn get_user_obligations(
    State(service): State<ApiService>,
    Path(pubkey): Path<String>,
) -> (StatusCode, Json<Vec<ApiUserObligation>>) {
    match service.get_user_obligations(&pubkey).await {
        Ok(obligations) => {
            info!("Successfully returned {} obligations for pubkey {}", obligations.len(), pubkey);
            (StatusCode::OK, Json(obligations))
        }
        Err(e) => {
            error!("Error fetching obligations for pubkey {}: {}", pubkey, e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(vec![]))
        }
    }
}

#[derive(serde::Serialize)]
pub struct WalletData {
    pub wallet_balances: Vec<ApiTokenBalance>,
    pub wallet_positions: Vec<ApiUserObligation>,
}

#[derive(serde::Serialize)]
pub struct ApiTokenBalance {
    #[serde(skip)]
    pub mint: String,
    pub symbol: String,
    #[serde(serialize_with = "serialize_dollar_amount")]
    pub amount: (u64, u32),
}

impl From<common::TokenBalance> for ApiTokenBalance {
    fn from(balance: common::TokenBalance) -> Self {
        Self {
            mint: balance.mint,
            symbol: balance.symbol,
            amount: (balance.amount, balance.decimals as u32),
        }
    }
}

// User-related code
use common::{RiskLevel, UserDB};
use serde::Deserialize;

// Request models for user endpoints
#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub wallet_address: String,
    pub email: Option<String>,
    pub risk_level: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub risk_level: Option<String>,
    pub update_login_time: Option<bool>,
}

// Database query structure for users
#[derive(sqlx::FromRow)]
struct DbUser {
    wallet_address: String,
    email: Option<String>,
    risk_level: String,
    created_date: chrono::DateTime<chrono::Utc>,
    last_logged_in: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<DbUser> for UserDB {
    fn from(db_user: DbUser) -> Self {
        let risk_level = match db_user.risk_level.to_lowercase().as_str() {
            "low" => RiskLevel::Low,
            "medium" => RiskLevel::Medium,
            "high" => RiskLevel::High,
            _ => RiskLevel::Low, // Default fallback
        };

        UserDB {
            wallet_address: db_user.wallet_address,
            email: db_user.email,
            risk_level,
            created_date: db_user.created_date,
            last_logged_in: db_user.last_logged_in,
        }
    }
}

// Helper function to convert RiskLevel enum to string for database storage
fn risk_level_to_str(risk_level: &RiskLevel) -> &str {
    match risk_level {
        RiskLevel::Low => "low",
        RiskLevel::Medium => "medium",
        RiskLevel::High => "high",
    }
}

// Generic API response wrapper
#[derive(Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

// Add user methods to ApiService
impl ApiService {
    pub async fn create_user(&self, user_data: CreateUserRequest) -> Result<UserDB> {
        debug!("Creating new user with wallet address: {}", user_data.wallet_address);

        let risk_level = match user_data.risk_level.to_lowercase().as_str() {
            "low" => RiskLevel::Low,
            "medium" => RiskLevel::Medium,
            "high" => RiskLevel::High,
            _ => return Err(anyhow::anyhow!("Invalid risk level: {}", user_data.risk_level)),
        };

        let now = chrono::Utc::now();

        // Insert user into database
        sqlx::query(
            r#"
            INSERT INTO users (wallet_address, email, risk_level, created_date, last_logged_in)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(&user_data.wallet_address)
        .bind(&user_data.email)
        .bind(risk_level_to_str(&risk_level))
        .bind(now)
        .bind(now) // Initial login time is same as creation time
        .execute(&self.db_pool)
        .await?;

        info!("Successfully created new user with wallet address: {}", user_data.wallet_address);

        // Return the created user
        Ok(UserDB {
            wallet_address: user_data.wallet_address,
            email: user_data.email,
            risk_level,
            created_date: now,
            last_logged_in: Some(now),
        })
    }

    pub async fn get_user(&self, wallet_address: &str) -> Result<UserDB> {
        debug!("Fetching user with wallet address: {}", wallet_address);

        let user = sqlx::query_as::<_, DbUser>(
            r#"
            SELECT wallet_address, email, risk_level, created_date, last_logged_in
            FROM users
            WHERE wallet_address = ?
            "#,
        )
        .bind(wallet_address)
        .fetch_optional(&self.db_pool)
        .await?;

        match user {
            Some(user) => {
                info!("Successfully retrieved user with wallet address: {}", wallet_address);
                Ok(user.into())
            }
            None => Err(anyhow::anyhow!("User not found with wallet address: {}", wallet_address)),
        }
    }

    pub async fn update_user(
        &self,
        wallet_address: &str,
        user_data: UpdateUserRequest,
    ) -> Result<UserDB> {
        debug!("Updating user with wallet address: {}", wallet_address);

        // First check if user exists
        // Check if user exists, return error if not found
        if let Err(_) = self.get_user(wallet_address).await {
            return Err(anyhow::anyhow!(
                "Cannot update non-existent user with wallet address: {}",
                wallet_address
            ));
        }

        // Update the user
        sqlx::query(
            r#"
            UPDATE users
            SET email = COALESCE(?, email),
                risk_level = COALESCE(?, risk_level),
                last_logged_in = CASE WHEN ? = 1 THEN datetime('now') ELSE last_logged_in END
            WHERE wallet_address = ?
            "#,
        )
        .bind(&user_data.email)
        .bind(&user_data.risk_level)
        .bind(user_data.update_login_time.unwrap_or(false))
        .bind(wallet_address)
        .execute(&self.db_pool)
        .await?;

        info!("Successfully updated user with wallet address: {}", wallet_address);

        // Return the updated user
        self.get_user(wallet_address).await
    }
}

// Handler functions for user endpoints
async fn create_user(
    State(service): State<ApiService>,
    Json(user_data): Json<CreateUserRequest>,
) -> (StatusCode, Json<ApiResponse<UserDB>>) {
    match service.create_user(user_data).await {
        Ok(user) => {
            info!("Successfully created user with wallet address: {}", user.wallet_address);
            (
                StatusCode::CREATED,
                Json(ApiResponse { success: true, data: Some(user), error: None }),
            )
        }
        Err(e) => {
            error!("Error creating user: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse { success: false, data: None, error: Some(e.to_string()) }),
            )
        }
    }
}

async fn get_user(
    State(service): State<ApiService>,
    Path(wallet_address): Path<String>,
) -> (StatusCode, Json<ApiResponse<UserDB>>) {
    match service.get_user(&wallet_address).await {
        Ok(user) => {
            info!("Successfully retrieved user with wallet address: {}", wallet_address);
            (StatusCode::OK, Json(ApiResponse { success: true, data: Some(user), error: None }))
        }
        Err(e) => {
            error!("Error retrieving user with wallet address {}: {}", wallet_address, e);
            let status = if e.to_string().contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(ApiResponse { success: false, data: None, error: Some(e.to_string()) }))
        }
    }
}

async fn update_user(
    State(service): State<ApiService>,
    Path(wallet_address): Path<String>,
    Json(user_data): Json<UpdateUserRequest>,
) -> (StatusCode, Json<ApiResponse<UserDB>>) {
    match service.update_user(&wallet_address, user_data).await {
        Ok(user) => {
            info!("Successfully updated user with wallet address: {}", wallet_address);
            (StatusCode::OK, Json(ApiResponse { success: true, data: Some(user), error: None }))
        }
        Err(e) => {
            error!("Error updating user with wallet address {}: {}", wallet_address, e);
            let status = if e.to_string().contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(ApiResponse { success: false, data: None, error: Some(e.to_string()) }))
        }
    }
}
