
use axum::{routing::get, Json, Router};
use serde_json::json;
use std::net::SocketAddr;

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "service": "rust-fhir-server"
    }))
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/health", get(health));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind address");

    println!("FHIR server running at http://{addr}");

    axum::serve(listener, app)
        .await
        .expect("Server failed");
}
