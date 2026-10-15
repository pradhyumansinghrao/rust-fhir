use rust_fhir::{create_router, AppState, MemoryStorage, ServerConfig};
use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Read server configuration from environment
    let config = ServerConfig::from_env();
    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .expect("Invalid host or port configuration");

    // Initialize in-memory storage engine
    let storage = Arc::new(MemoryStorage::new());

    let state = AppState {
        config: config.clone(),
        storage,
    };

    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind TCP listener");

    println!("====================================================");
    println!(" Rust FHIR R4 Server");
    println!(" Running at: {}", config.base_url);
    println!(" Conformance metadata: {}/metadata", config.base_url);
    println!(" Health check: {}/health", config.base_url);
    println!("====================================================");

    axum::serve(listener, app)
        .await
        .expect("Server terminated unexpectedly");
}
