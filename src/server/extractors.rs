use axum::http::HeaderMap;

pub fn get_if_match(headers: &HeaderMap) -> Option<&str> {
    headers.get("if-match").and_then(|v| v.to_str().ok())
}

pub fn get_if_none_match(headers: &HeaderMap) -> Option<&str> {
    headers.get("if-none-match").and_then(|v| v.to_str().ok())
}
