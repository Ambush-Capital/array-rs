use anyhow::Result;
use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use common::{LendingReserve, MintAsset};
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
    pub total_supply: u128,
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

pub async fn create_router(service: ApiService) -> Router {
    Router::new()
        .route("/current_markets", get(get_current_markets))
        .route("/historical_markets", get(get_historical_markets))
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
