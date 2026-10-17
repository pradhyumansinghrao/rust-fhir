# rust-fhir ⚡🏥

[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg?logo=rust)](https://www.rust-lang.org)
[![FHIR](https://img.shields.io/badge/FHIR-R4%20(v4.0.1)-firebrick.svg)](https://hl7.org/fhir/R4/)
[![Axum](https://img.shields.io/badge/axum-0.8-blue.svg)](https://github.com/tokio-rs/axum)
[![SQLx](https://img.shields.io/badge/sqlx-sqlite%20%7C%20postgres-teal.svg)](https://github.com/launchbadge/sqlx)
[![License: MIT / Apache-2.0](https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue.svg)](#license)

A high-performance, specification-compliant **HL7® FHIR® R4 (v4.0.1)** RESTful server and modular library written in Rust, powered by **Axum**, **Tokio**, and **SQLx**.

Designed for speed, safety, and flexibility, `rust-fhir` can be deployed as a standalone healthcare data service or embedded directly into Rust applications as a FHIR storage engine and HTTP routing layer.

---

## 🌟 Key Features

- **FHIR R4 RESTful API**: Full CRUD lifecycle (`create`, `read`, `vread`, `update`, `delete`).
- **Resource Versioning & Audit Trail**: Immutable revision tracking with version IDs and historical queries (`_history` at instance, type, and system levels).
- **Concurrency & Caching**: Optimistic locking via HTTP `ETag` and `If-Match` headers, plus 304 caching via `If-None-Match`.
- **Pluggable Storage Engine**:
  - 🏎️ **In-Memory (`MemoryStorage`)**: Concurrent, thread-safe volatile storage backed by `parking_lot::RwLock`—ideal for unit tests, CI, and ultra-low-latency caches.
  - 🗄️ **SQLite (`SqlStorage`)**: Fast embedded relational persistence with automatic WAL mode, schema initialization, and zero external dependencies.
  - 🐘 **PostgreSQL (`SqlStorage`)**: Production-ready relational storage with connection pooling for cloud and enterprise deployments.
- **Search Framework**:
  - Standard FHIR search parameters: `_id`, `_lastUpdated`, `_count`, `_offset` / `_page`, and `_sort`.
  - Date prefixes: `eq`, `ne`, `gt`, `lt`, `ge`, `le`, `sa`, and `eb`.
  - Token, string (prefix/substring), and reference search indexing across core resources (`Patient`, `Observation`, `Encounter`, `Condition`, `DiagnosticReport`, `Practitioner`, `Organization`, etc.).
  - Searchset Bundle pagination with RFC-compliant `self`, `next`, and `previous` links.
- **Transactions & Batches**: Atomic multi-operation FHIR `Bundle` processing with automatic rollback on error.
- **Conformance & Validation**:
  - `GET /metadata`: Automatically generated FHIR R4 `CapabilityStatement`.
  - `POST /{resource_type}/$validate`: Structural and resource-type validation returning standard `OperationOutcome`.
- **Standardized Error Handling**: Detailed FHIR `OperationOutcome` responses mapped to HTTP status codes (`400`, `404`, `409`, `410 Gone`, `412 Precondition Failed`, `422`, `500`).
- **CORS & Observability**: Configured CORS middleware exposing FHIR headers (`ETag`, `Location`, `Last-Modified`), plus structured logging with `tracing`.

---

## 🏗️ Architecture

```mermaid
flowchart TD
    Client[HTTP Client / EHR / App] -->|application/fhir+json| AxumRouter[Axum Router & CORS Layer]
    
    subgraph Server Layer [Server Layer]
        AxumRouter --> Extractors[Header & Query Extractors\nETag / If-Match / SearchParams]
        Extractors --> Handlers[FHIR Handlers\nCRUD / History / Bundles / Search]
    end

    subgraph Search & Models [Core & Search Engine]
        Handlers --> SearchEngine[Search Engine & Indexer]
        Handlers --> Models[FHIR R4 Models\nBundle, CapabilityStatement, OperationOutcome]
    end

    subgraph Storage [Storage Abstraction (FhirStorage Trait)]
        Handlers --> FhirStorage[FhirStorage Trait]
        FhirStorage --> MemoryStorage[MemoryStorage\nConcurrent In-Memory Maps]
        FhirStorage --> SqlStorage[SqlStorage\nSQLx Query & Transaction Engine]
    end

    subgraph Persistence [Database Backends]
        SqlStorage --> SQLite[(SQLite fhir.db / WAL)]
        SqlStorage --> Postgres[(PostgreSQL DB)]
    end
```

---

## 🚀 Quick Start

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (1.85+ recommended for Rust 2024 edition)
- `cargo` package manager

### 1. Clone & Build

```bash
git clone https://github.com/pradhyumansinghrao/rust-fhir.git
cd rust-fhir
cargo build --release
```

### 2. Run the Server

By default, the server runs on `http://127.0.0.1:3000` using a local SQLite database (`fhir.db`):

```bash
cargo run
```

You should see startup logs similar to:

```text
====================================================
 Rust FHIR R4 Server
 Running at: http://127.0.0.1:3000
 Database: Local SQLite (sqlite://fhir.db)
 Conformance metadata: http://127.0.0.1:3000/metadata
 Health check: http://127.0.0.1:3000/health
====================================================
```

---

## ⚙️ Configuration

The server is configured using environment variables or a `.env` file:

| Variable | Description | Default Value | Options / Format |
|---|---|---|---|
| `HOST` | Bind network host address | `127.0.0.1` | `0.0.0.0`, `127.0.0.1` |
| `PORT` | Listening TCP port | `3000` | Any valid port (e.g. `8080`) |
| `BASE_URL` | Canonical public base URL | `http://{HOST}:{PORT}` | `https://fhir.example.com` |
| `DATABASE_URL` | Database connection URL | `sqlite://fhir.db` | `sqlite://...` or `postgres://...` |
| `STORAGE_BACKEND` | Explicit storage backend override | Auto-detected from `DATABASE_URL` | `sqlite`, `postgres`, `memory` |

### Storage Backend Examples

#### Local SQLite (Default)
```bash
cargo run
# Or custom file path:
DATABASE_URL="sqlite://my_records.db" cargo run
```

#### Fast In-Memory (Zero Disk Footprint)
```bash
STORAGE_BACKEND="memory" cargo run
```

#### Remote PostgreSQL
```bash
DATABASE_URL="postgres://fhir_user:secret@localhost:5432/fhir_db" cargo run
```

---

## 📡 API Reference & Examples

### Health Check

```bash
curl -X GET http://127.0.0.1:3000/health
```
```json
{
  "status": "ok",
  "service": "rust-fhir-server"
}
```

### Conformance Metadata (`CapabilityStatement`)

```bash
curl -X GET http://127.0.0.1:3000/metadata \
  -H "Accept: application/fhir+json"
```

---

### CRUD Operations

#### 1. Create Resource (`POST /{resource_type}`)

Creates a new resource. Returns `201 Created` with an assigned `id`, `ETag: W/"1"`, and a `Location` header.

```bash
curl -i -X POST http://127.0.0.1:3000/Patient \
  -H "Content-Type: application/fhir+json" \
  -d '{
    "resourceType": "Patient",
    "name": [
      {
        "use": "official",
        "family": "Curie",
        "given": ["Marie"]
      }
    ],
    "gender": "female",
    "birthDate": "1867-11-07"
  }'
```

#### 2. Read Resource (`GET /{resource_type}/{id}`)

```bash
curl -i -X GET http://127.0.0.1:3000/Patient/<id>
```

Supports conditional read:
```bash
curl -i -X GET http://127.0.0.1:3000/Patient/<id> \
  -H 'If-None-Match: W/"1"'
# Returns 304 Not Modified if unchanged
```

#### 3. Version Read (`GET /{resource_type}/{id}/_history/{version_id}`)

Retrieves a specific historical revision of the resource:

```bash
curl -X GET http://127.0.0.1:3000/Patient/<id>/_history/1
```

#### 4. Update Resource with Optimistic Locking (`PUT /{resource_type}/{id}`)

Updates the resource and increments its version ID. Use the `If-Match` header to guard against concurrent overwrites:

```bash
curl -i -X PUT http://127.0.0.1:3000/Patient/<id> \
  -H "Content-Type: application/fhir+json" \
  -H 'If-Match: W/"1"' \
  -d '{
    "resourceType": "Patient",
    "id": "<id>",
    "name": [
      {
        "use": "official",
        "family": "Curie-Sklodowska",
        "given": ["Marie"]
      }
    ],
    "gender": "female",
    "birthDate": "1867-11-07"
  }'
```

*(If the current version does not match `W/"1"`, the server rejects the request with `412 Precondition Failed`.)*

#### 5. Delete Resource (`DELETE /{resource_type}/{id}`)

Performs a soft delete. The version is incremented, and subsequent `GET` requests return `410 Gone`.

```bash
curl -i -X DELETE http://127.0.0.1:3000/Patient/<id>
```

---

### Resource History

Query historical versions formatted as a FHIR `Bundle` of type `history`:

- **Instance History**: `GET /{resource_type}/{id}/_history`
- **Type History**: `GET /{resource_type}/_history`
- **System History**: `GET /_history`

```bash
curl -X GET http://127.0.0.1:3000/Patient/<id>/_history
```

---

### Search

Supports standard search via `GET /{resource_type}?param=val` or `POST /{resource_type}/_search`.

#### Search by String or Token
```bash
# Search patients by family name
curl -X GET "http://127.0.0.1:3000/Patient?family=Curie"

# Search active patients with pagination
curl -X GET "http://127.0.0.1:3000/Patient?active=true&_count=10&_offset=0"
```

#### Search with Date Prefixes
Supported prefixes: `eq`, `ne`, `gt`, `lt`, `ge`, `le`, `sa` (starts after), `eb` (ends before).

```bash
# Patients born on or after 1980-01-01
curl -X GET "http://127.0.0.1:3000/Patient?birthdate=ge1980-01-01"

# Resources updated recently
curl -X GET "http://127.0.0.1:3000/Observation?_lastUpdated=gt2026-01-01T00:00:00Z"
```

---

### Batch & Transaction Bundles (`POST /`)

Execute multiple operations in a single request. 

- `type: "transaction"`: Executes all operations atomically within a database transaction. If any operation fails, the entire bundle is rolled back.
- `type: "batch"`: Executes operations independently and returns individual results.

```bash
curl -X POST http://127.0.0.1:3000/ \
  -H "Content-Type: application/fhir+json" \
  -d '{
    "resourceType": "Bundle",
    "type": "transaction",
    "entry": [
      {
        "request": {
          "method": "POST",
          "url": "Patient"
        },
        "resource": {
          "resourceType": "Patient",
          "name": [{ "family": "Fleming", "given": ["Alexander"] }]
        }
      },
      {
        "request": {
          "method": "POST",
          "url": "Observation"
        },
        "resource": {
          "resourceType": "Observation",
          "status": "final",
          "code": {
            "coding": [{
              "system": "http://loinc.org",
              "code": "15074-8",
              "display": "Glucose"
            }]
          }
        }
      }
    ]
  }'
```

---

### Validation (`POST /{resource_type}/$validate`)

Validate a resource against basic FHIR structural constraints:

```bash
curl -X POST http://127.0.0.1:3000/Patient/\$validate \
  -H "Content-Type: application/fhir+json" \
  -d '{
    "resourceType": "Patient",
    "gender": "male"
  }'
```

---

## 📦 Using as a Rust Library

You can use `rust-fhir` as a library in your own Rust applications:

```rust
use rust_fhir::{create_router, AppState, ServerConfig, SqlStorage};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::default();
    
    // Connect to SQLite or Postgres
    let storage = SqlStorage::connect("sqlite://fhir.db").await?;
    
    let state = AppState {
        config: config.clone(),
        storage: Arc::new(storage),
    };

    let router = create_router(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, router).await?;
    
    Ok(())
}
```

---

## 🧪 Testing

The repository includes comprehensive unit and integration test suites covering the FHIR API, transaction rollbacks, pagination, date filtering, and SQLite/PostgreSQL persistence.

Run all tests:

```bash
cargo test
```

Run tests with verbose output:

```bash
RUST_LOG=debug cargo test -- --nocapture
```

---

## 📂 Project Structure

```text
rust-fhir/
├── src/
│   ├── config.rs         # Server and database configuration
│   ├── lib.rs            # Library entrypoint and public exports
│   ├── main.rs           # Server binary entrypoint
│   ├── model/            # FHIR R4 domain models
│   │   ├── bundle.rs     # Bundle, entries, links, transactions
│   │   ├── capability.rs # CapabilityStatement conformance generator
│   │   ├── outcome.rs    # OperationOutcome error modeling
│   │   └── resource.rs   # StoredResource, UUID, and timestamp helpers
│   ├── search/           # FHIR search engine
│   │   ├── engine.rs     # Multi-parameter filtering & sorting
│   │   └── params.rs     # Query string parsing & date prefix logic
│   ├── server/           # Axum HTTP server layer
│   │   ├── extractors.rs # ETag and header extractors
│   │   ├── handlers.rs   # REST endpoint handlers
│   │   ├── responses.rs  # FHIR JSON response builders
│   │   └── router.rs     # Axum routing & CORS configuration
│   └── storage/          # Storage abstraction
│       ├── mem.rs        # Thread-safe in-memory storage engine
│       ├── mod.rs        # FhirStorage trait & index extraction
│       └── sql.rs        # SQLx storage engine (SQLite & PostgreSQL)
└── tests/
    ├── fhir_api_test.rs  # End-to-end HTTP API tests
    └── sql_storage_test.rs # SQL database persistence & rollback tests
```

---

## 🤝 Contributing

Contributions, issues, and feature requests are welcome!

1. Fork the project
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Run tests to ensure all pass (`cargo test`)
5. Push to the branch (`git push origin feature/amazing-feature`)
6. Open a Pull Request

---

## 📄 License

This project is dual-licensed under either:

- **Apache License, Version 2.0** ([LICENSE-APACHE](http://www.apache.org/licenses/LICENSE-2.0))
- **MIT License** ([LICENSE-MIT](http://opensource.org/licenses/MIT))

at your option.

---

<p align="center">
  Built with ❤️ for modern healthcare technology interoperability.
</p>
