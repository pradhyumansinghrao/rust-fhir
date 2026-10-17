use rust_fhir::{create_router, AppState, DbBackend, FhirStorage, MemoryStorage, ServerConfig, SqlStorage};
use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Read server configuration from environment
    let config = ServerConfig::from_env();
    let addr: SocketAddr = format!("{}:{}", config.host, config.port)
        .parse()
        .expect("Invalid host or port configuration");

    // Initialize database backend based on configuration
    let (storage, backend_name): (Arc<dyn FhirStorage>, String) = match config.storage_backend {
        DbBackend::Memory => (
            Arc::new(MemoryStorage::new()),
            "In-Memory (Volatile)".to_string(),
        ),
        DbBackend::Postgres => {
            let sql_store = SqlStorage::connect(&config.database_url)
                .await
                .expect("Failed to connect to remote PostgreSQL database");
            (
                Arc::new(sql_store),
                format!("Remote PostgreSQL ({})", config.database_url),
            )
        }
        DbBackend::Sqlite => {
            let sql_store = SqlStorage::connect(&config.database_url)
                .await
                .expect("Failed to initialize local SQLite database");
            (
                Arc::new(sql_store),
                format!("Local SQLite ({})", config.database_url),
            )
        }
    };

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
    println!(" Database: {}", backend_name);
    println!(" Conformance metadata: {}/metadata", config.base_url);
    println!(" Health check: {}/health", config.base_url);
    println!("====================================================");

    axum::serve(listener, app)
        .await
        .expect("Server terminated unexpectedly");
}
