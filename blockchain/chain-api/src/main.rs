use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use common::{MintAsset, TokenBalance, UserObligation};
use sol_interface::{
    aggregator::client::LendingMarketAggregator, common::client_trait::ClientError,
};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
struct LendingService {
    aggregator: Arc<RwLock<LendingMarketAggregator>>,
}

impl LendingService {
    pub fn new() -> Self {
        let rpc_url = std::env::var("RPC_URL")
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());

        Self { aggregator: Arc::new(RwLock::new(LendingMarketAggregator::new(&rpc_url))) }
    }

    pub async fn get_current_lending_markets(&self) -> Result<Vec<MintAsset>, ClientError> {
        let mut aggregator = self.aggregator.write().await;

        aggregator.load_markets_async().await?;

        let assets = aggregator.assets.values().cloned().collect();

        Ok(assets)
    }

    pub async fn get_user_obligations(
        &self,
        pubkey: &str,
    ) -> Result<Vec<UserObligation>, ClientError> {
        let aggregator = self.aggregator.read().await;
        aggregator.get_user_obligations_async(pubkey).await
    }

    pub async fn get_wallet_token_balances(
        &self,
        wallet_pubkey: &str,
    ) -> Result<Vec<TokenBalance>, ClientError> {
        let aggregator = self.aggregator.read().await;

        // Call the fetch_wallet_token_balances method and convert any errors to ClientError
        aggregator
            .fetch_wallet_token_balances(wallet_pubkey)
            .await
            .map_err(|e| ClientError::Other(format!("Failed to fetch wallet balances: {}", e)))
    }
}

async fn get_current_lending_markets(
    State(service): State<LendingService>,
) -> (StatusCode, Json<Vec<MintAsset>>) {
    match service.get_current_lending_markets().await {
        Ok(markets) => (StatusCode::OK, Json(markets)),
        Err(e) => {
            eprintln!("Error fetching assets: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(vec![]))
        }
    }
}

async fn get_user_obligations(
    State(service): State<LendingService>,
    Path(pubkey): Path<String>,
) -> (StatusCode, Json<Vec<UserObligation>>) {
    match service.get_user_obligations(&pubkey).await {
        Ok(obligations) => (StatusCode::OK, Json(obligations)),
        Err(e) => {
            eprintln!("Error fetching obligations: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(vec![]))
        }
    }
}

async fn get_wallet_balance(
    State(service): State<LendingService>,
    Path(pubkey): Path<String>,
) -> (StatusCode, Json<Vec<TokenBalance>>) {
    match service.get_wallet_token_balances(&pubkey).await {
        Ok(balances) => (StatusCode::OK, Json(balances)),
        Err(e) => {
            eprintln!("Error fetching wallet balances for {}: {}", pubkey, e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(vec![]))
        }
    }
}

// basic handler that responds with a static string
async fn root() -> &'static str {
    "Hello, World!"
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let service = LendingService::new();

    // build our application with a route
    let app = Router::new()
        // `GET /` goes to `root`
        .route("/", get(root))
        // `POST /users` goes to `create_user`
        .route("/current_lending_markets", get(get_current_lending_markets))
        .route("/obligations/{pubkey}", get(get_user_obligations))
        .route("/wallet_balance/{pubkey}", get(get_wallet_balance))
        .with_state(service);

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Server listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
