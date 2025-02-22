use anchor_client::{Client, Cluster};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use common::{MintAsset, UserObligation};
use sol_interface::aggregator::client::{ArrayError, LendingMarketAggregator};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    signature::{read_keypair_file, Keypair},
};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
struct LendingService {
    aggregator: Arc<RwLock<LendingMarketAggregator<Arc<Keypair>>>>,
}

impl LendingService {
    pub fn new() -> Self {
        let rpc_url = std::env::var("RPC_URL").expect("Missing RPC_URL");
        let keypair_path =
            std::env::var("KEYPAIR_PATH").expect("Missing KEYPAIR_PATH environment variable");
        let payer = Arc::new(read_keypair_file(keypair_path).expect("Failed to load keypair"));

        let client = Arc::new(Client::new_with_options(
            Cluster::Custom(rpc_url.clone(), rpc_url),
            payer.clone(),
            CommitmentConfig::confirmed(),
        ));

        Self { aggregator: Arc::new(RwLock::new(LendingMarketAggregator::new(&client))) }
    }

    pub async fn get_current_lending_markets(&self) -> Result<Vec<MintAsset>, ArrayError> {
        let mut aggregator = self.aggregator.write().await;

        aggregator.load_markets()?;

        let assets = aggregator.assets.clone();

        Ok(assets)
    }

    pub async fn get_user_obligations(
        &self,
        pubkey: &str,
    ) -> Result<Vec<UserObligation>, ArrayError> {
        let aggregator = self.aggregator.read().await;
        aggregator.get_user_obligations(pubkey)
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

// basic handler that responds with a static string
async fn root() -> &'static str {
    "Hello, World!"
}

#[tokio::main]
async fn main() {
    let service = LendingService::new();

    // build our application with a route
    let app = Router::new()
        // `GET /` goes to `root`
        .route("/", get(root))
        // `POST /users` goes to `create_user`
        .route("/current_lending_markets", get(get_current_lending_markets))
        .route("/obligations/{pubkey}", get(get_user_obligations))
        .with_state(service);

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Server listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
