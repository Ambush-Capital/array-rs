use anyhow::Result;
use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use common::MintAsset;
use log::{debug, error, info};
use sqlx::{Pool, Sqlite};

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

    pub async fn get_current_markets(&self) -> Result<Vec<MintAsset>> {
        debug!("Fetching current markets from chain-api");
        // Forward request to chain-api
        let response =
            self.client.get("http://localhost:3000/current_lending_markets").send().await?;

        let markets = response.json::<Vec<MintAsset>>().await?;
        info!("Retrieved {} current markets from chain-api", markets.len());
        Ok(markets)
    }

    pub async fn get_historical_markets(&self) -> Result<Vec<HistoricalMarketData>> {
        debug!("Fetching historical markets from database");
        // Query all market data for the last 5 unique timestamps
        let markets = sqlx::query_as::<_, HistoricalMarketData>(
            r#"
            WITH recent_timestamps AS (
                SELECT DISTINCT timestamp
                FROM lending_markets
                ORDER BY timestamp DESC
                LIMIT 5
            )
            SELECT 
                protocol_name, market_name, token_name, token_symbol, token_mint,
                market_price, total_supply, total_borrows, borrow_rate, supply_rate,
                borrow_apy, supply_apy, slot, timestamp
            FROM lending_markets
            WHERE timestamp IN (SELECT timestamp FROM recent_timestamps)
            ORDER BY timestamp DESC, market_name ASC
            "#,
        )
        .fetch_all(&self.db_pool)
        .await?;

        info!("Retrieved {} historical market entries across 5 timestamps", markets.len());
        if !markets.is_empty() {
            debug!(
                "Time range: from {} to {}",
                markets.last().unwrap().timestamp,
                markets.first().unwrap().timestamp
            );
        }
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
    pub total_supply: String,
    pub total_borrows: String,
    pub borrow_rate: String,
    pub supply_rate: String,
    pub borrow_apy: String,
    pub supply_apy: String,
    pub slot: u64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

pub async fn create_router(service: ApiService) -> Router {
    Router::new()
        .route("/current_markets", get(get_current_markets))
        .route("/historical_markets", get(get_historical_markets))
        .with_state(service)
}

async fn get_current_markets(
    State(service): State<ApiService>,
) -> (StatusCode, Json<Vec<MintAsset>>) {
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
) -> (StatusCode, Json<Vec<HistoricalMarketData>>) {
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
