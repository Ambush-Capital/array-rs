use anyhow::Result;
use chrono::Utc;
use common::{LendingReserve, MintAsset};
use log::{debug, error, info};
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use tokio_cron_scheduler::{Job, JobScheduler};

pub struct Worker {
    db_pool: Pool<Sqlite>,
    schedule: String,
}

impl Worker {
    pub async fn new(db_url: &str, schedule: String) -> Result<Self> {
        // Create connection pool
        let pool = SqlitePoolOptions::new().max_connections(5).connect(db_url).await?;
        info!("Connected to database at {}", db_url);

        // Initialize tables
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS lending_markets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                protocol_name VARCHAR(64) NOT NULL,
                market_name VARCHAR(64) NOT NULL,
                token_name VARCHAR(64) NOT NULL,
                token_symbol VARCHAR(10) NOT NULL,
                token_mint VARCHAR(64) NOT NULL,
                market_price UNSIGNED BIGINT NOT NULL,
                total_supply DECIMAL(39,0) NOT NULL,
                total_borrows DECIMAL(39,0) NOT NULL,
                borrow_rate DECIMAL(39,0) NOT NULL,
                supply_rate DECIMAL(39,0) NOT NULL,
                borrow_apy DECIMAL(39,0) NOT NULL,
                supply_apy DECIMAL(39,0) NOT NULL,
                slot UNSIGNED BIGINT NOT NULL,
                timestamp DATETIME NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await?;
        debug!("Ensured lending_markets table exists");

        Ok(Self { db_pool: pool, schedule })
    }

    pub async fn start_market_sync(&self) -> Result<()> {
        let client = reqwest::Client::new();
        let scheduler = JobScheduler::new().await?;

        let db_pool = self.db_pool.clone();
        let job = Job::new_async(self.schedule.as_str(), move |_, _| {
            let client = client.clone();
            let db_pool = db_pool.clone();

            Box::pin(async move {
                debug!("Fetching current lending markets...");
                // Fetch latest market data from API
                match client.get("http://localhost:3000/current_lending_markets").send().await {
                    Ok(response) => match response.json::<Vec<MintAsset>>().await {
                        Ok(assets) => {
                            let mut total_reserves = 0;
                            for asset in &assets {
                                for reserve in &asset.lending_reserves {
                                    if let Err(e) =
                                        store_market_data(&db_pool, asset, reserve).await
                                    {
                                        error!("Failed to store market data: {}", e);
                                    }
                                    total_reserves += 1;
                                }
                            }
                            info!("Successfully saved data for {} lending markets", total_reserves);
                        }
                        Err(e) => error!("Failed to deserialize market data: {}", e),
                    },
                    Err(e) => error!("Failed to fetch market data: {}", e),
                }
            })
        })?;

        scheduler.add(job).await?;
        info!("Starting market sync scheduler with schedule: {}", self.schedule);
        scheduler.start().await?;

        // Keep the scheduler running
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }
}

// Move store_market_data to a standalone function since we can't easily clone self
async fn store_market_data(
    db_pool: &Pool<Sqlite>,
    asset: &MintAsset,
    reserve: &LendingReserve,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO lending_markets (
            protocol_name, market_name, token_name, token_symbol, token_mint,
            market_price, total_supply, total_borrows, borrow_rate, supply_rate,
            borrow_apy, supply_apy, slot, timestamp
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&reserve.protocol_name)
    .bind(&reserve.market_name)
    .bind(&asset.name)
    .bind(&asset.symbol)
    .bind(&asset.mint)
    .bind(asset.market_price_sf as i64)
    .bind(reserve.total_supply.to_string())
    .bind(reserve.total_borrows.to_string())
    .bind(reserve.borrow_rate.to_string())
    .bind(reserve.supply_rate.to_string())
    .bind(reserve.borrow_apy.to_string())
    .bind(reserve.supply_apy.to_string())
    .bind(reserve.slot as i64)
    .bind(Utc::now())
    .execute(db_pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_database_initialization() {
        let db_url = "sqlite::memory:";
        let worker = Worker::new(db_url, "".to_string()).await.unwrap();

        // Verify table exists
        let result = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='lending_markets'",
        )
        .fetch_one(&worker.db_pool)
        .await;

        assert!(result.is_ok());
    }
}
