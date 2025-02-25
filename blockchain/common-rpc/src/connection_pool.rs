use lazy_static::lazy_static;
use solana_client::rpc_client::RpcClient;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

/// A thread-safe pool of RPC clients for reuse
///
/// This pool maintains a set of RPC clients for different endpoints,
/// allowing them to be reused instead of creating new connections for each request.
pub struct RpcConnectionPool {
    clients: Mutex<VecDeque<(String, RpcClient)>>,
    max_clients_per_endpoint: usize,
    timeout: Duration,
}

impl RpcConnectionPool {
    /// Create a new connection pool with the specified maximum clients per endpoint and timeout
    pub fn new(max_clients_per_endpoint: usize, timeout: Duration) -> Self {
        Self { clients: Mutex::new(VecDeque::new()), max_clients_per_endpoint, timeout }
    }

    /// Get a client for the specified endpoint
    ///
    /// If a client for this endpoint is available in the pool, it will be returned.
    /// Otherwise, a new client will be created.
    pub fn get_client(&self, endpoint: &str) -> RpcClient {
        let endpoint_str = endpoint.to_string();
        let mut clients = self.clients.lock().unwrap();

        // Try to find an existing client for this endpoint
        for i in 0..clients.len() {
            if clients[i].0 == endpoint_str {
                let (_, client) = clients.remove(i).unwrap();
                return client;
            }
        }

        // Create a new client if none available
        RpcClient::new_with_timeout(endpoint_str, self.timeout)
    }

    /// Return a client to the pool for future reuse
    ///
    /// The client will only be kept if we're under the maximum limit for this endpoint.
    pub fn return_client(&self, endpoint: &str, client: RpcClient) {
        let endpoint_str = endpoint.to_string();
        let mut clients = self.clients.lock().unwrap();

        // Count existing clients for this endpoint
        let count = clients.iter().filter(|(url, _)| url == &endpoint_str).count();

        // Only keep the client if we're under the limit
        if count < self.max_clients_per_endpoint {
            clients.push_back((endpoint_str, client));
        }
        // Otherwise let it drop
    }
}

// Global singleton instance
lazy_static! {
    pub static ref CONNECTION_POOL: RpcConnectionPool = RpcConnectionPool::new(
        5, // 5 clients per endpoint
        Duration::from_secs(30), // 30 second timeout
    );
}

/// Helper function to get a client and automatically return it when done
///
/// This function handles getting a client from the pool, executing the provided
/// function with it, and then returning the client to the pool.
pub fn with_rpc_client<F, R>(endpoint: &str, f: F) -> R
where
    F: FnOnce(&RpcClient) -> R,
{
    let client = CONNECTION_POOL.get_client(endpoint);
    let result = f(&client);
    CONNECTION_POOL.return_client(endpoint, client);
    result
}
