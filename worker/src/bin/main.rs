use anyhow::Result;
use env_logger::Env;
use log::info;
use std::env;
use worker::Worker;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // Initialize worker with SQLite database and schedule
    let db_name = env::var("DB_FILE").expect("DB_FILE must be set");
    let db_url = format!("sqlite://../{}", db_name);
    let schedule = "0 */1 * * * *"; // Run every 60 minutes on the hour
    let worker = Worker::new(&db_url, schedule.to_string()).await?;

    info!("Starting worker with database '{}' and schedule '{}'", db_url, schedule);

    // Start market sync process
    worker.start_market_sync().await?;

    Ok(())
}
