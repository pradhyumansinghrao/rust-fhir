use std::env;

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub base_url: String,
    pub default_page_size: usize,
    pub max_page_size: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3000,
            base_url: "http://127.0.0.1:3000".to_string(),
            default_page_size: 20,
            max_page_size: 100,
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

        Self {
            host,
            port,
            base_url,
            default_page_size: 20,
            max_page_size: 100,
        }
    }
}
