use std::env;

#[derive(Clone, Debug, PartialEq)]
pub enum DbBackend {
    Sqlite,
    Postgres,
    Memory,
}

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub base_url: String,
    pub default_page_size: usize,
    pub max_page_size: usize,
    pub database_url: String,
    pub storage_backend: DbBackend,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3000,
            base_url: "http://127.0.0.1:3000".to_string(),
            default_page_size: 20,
            max_page_size: 100,
            database_url: "sqlite://fhir.db".to_string(),
            storage_backend: DbBackend::Sqlite,
        }
    }
}

impl ServerConfig {
    pub fn from_env() -> Self {
        let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(3000);
        let base_url = env::var("BASE_URL")
            .unwrap_or_else(|_| format!("http://{host}:{port}"));

        let explicit_backend = env::var("STORAGE_BACKEND")
            .ok()
            .map(|s| s.to_lowercase());

        let raw_db_url = env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://fhir.db".to_string());
        let database_url = if !raw_db_url.contains("://") && !raw_db_url.starts_with(':') {
            format!("sqlite://{raw_db_url}")
        } else {
            raw_db_url
        };

        let storage_backend = match explicit_backend.as_deref() {
            Some("memory") => DbBackend::Memory,
            _ => {
                if database_url.starts_with("postgres://") || database_url.starts_with("postgresql://") {
                    DbBackend::Postgres
                } else {
                    DbBackend::Sqlite
                }
            }
        };

        Self {
            host,
            port,
            base_url,
            default_page_size: 20,
            max_page_size: 100,
            database_url,
            storage_backend,
        }
    }
}
