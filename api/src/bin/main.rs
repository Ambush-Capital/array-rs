use anyhow::Result;
use api::ApiService;
use env_logger::Env;
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // Get database name from environment variable or use default
    let db_name = env::var("DB_FILE").expect("DB_FILE must be set");
    let db_url = format!("sqlite://../{}", db_name);
    let service = ApiService::new(&db_url).await?;

    // Create router with our service
    let app = api::create_router(service).await;

    // Run server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await?;
    log::info!("API server listening on http://0.0.0.0:3001");
    axum::serve(listener, app).await?;

    Ok(())
}
