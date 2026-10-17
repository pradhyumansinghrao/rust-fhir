pub mod mem;
pub mod sql;

use crate::model::outcome::OperationOutcome;
use crate::model::resource::StoredResource;
use crate::search::engine::SearchResult;
use crate::search::params::SearchParams;
use async_trait::async_trait;
use serde_json::Value;

pub use mem::MemoryStorage;
pub use sql::SqlStorage;

#[derive(Debug, Clone)]
pub enum StorageError {
    NotFound(String),
    Deleted(String),
    BadRequest(String),
    Conflict(String),
    PreconditionFailed(String),
    Internal(String),
}

impl StorageError {
    pub fn to_outcome(&self) -> OperationOutcome {
        match self {
            StorageError::NotFound(msg) => OperationOutcome::not_found(msg),
            StorageError::Deleted(msg) => OperationOutcome::deleted(msg),
            StorageError::BadRequest(msg) => OperationOutcome::bad_request(msg),
            StorageError::Conflict(msg) => OperationOutcome::conflict(msg),
            StorageError::PreconditionFailed(msg) => OperationOutcome::precondition_failed(msg),
            StorageError::Internal(msg) => OperationOutcome::internal_error(msg),
        }
    }

    pub fn status_code(&self) -> axum::http::StatusCode {
        match self {
            StorageError::NotFound(_) => axum::http::StatusCode::NOT_FOUND,
            StorageError::Deleted(_) => axum::http::StatusCode::GONE,
            StorageError::BadRequest(_) => axum::http::StatusCode::BAD_REQUEST,
            StorageError::Conflict(_) => axum::http::StatusCode::CONFLICT,
            StorageError::PreconditionFailed(_) => axum::http::StatusCode::PRECONDITION_FAILED,
            StorageError::Internal(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[async_trait]
pub trait FhirStorage: Send + Sync {
    async fn create(&self, resource_type: &str, resource: Value) -> Result<StoredResource, StorageError>;
    async fn read(&self, resource_type: &str, id: &str) -> Result<StoredResource, StorageError>;
    async fn vread(&self, resource_type: &str, id: &str, version_id: &str) -> Result<StoredResource, StorageError>;
    async fn update(
        &self,
        resource_type: &str,
        id: &str,
        resource: Value,
        if_match: Option<&str>,
    ) -> Result<StoredResource, StorageError>;
    async fn delete(&self, resource_type: &str, id: &str) -> Result<(), StorageError>;
    async fn history_instance(&self, resource_type: &str, id: &str) -> Result<Vec<StoredResource>, StorageError>;
    async fn history_type(&self, resource_type: &str) -> Result<Vec<StoredResource>, StorageError>;
    async fn history_system(&self) -> Result<Vec<StoredResource>, StorageError>;
    async fn search(&self, resource_type: &str, params: &SearchParams) -> Result<SearchResult, StorageError>;
    async fn process_bundle(&self, bundle: Value) -> Result<Value, StorageError>;
}

/// Helper function to extract search parameter index entries from a resource JSON
pub fn extract_search_indices(data: &Value) -> Vec<(String, String)> {
    let mut indices = Vec::new();

    // 1. name (Patient, Practitioner, Organization, etc.)
    if let Some(names) = data.get("name").and_then(|n| n.as_array()) {
        for n in names {
            if let Some(text) = n.get("text").and_then(|t| t.as_str()) {
                indices.push(("name".to_string(), text.to_lowercase()));
            }
            if let Some(family) = n.get("family").and_then(|f| f.as_str()) {
                indices.push(("family".to_string(), family.to_lowercase()));
                indices.push(("name".to_string(), family.to_lowercase()));
            }
            if let Some(givens) = n.get("given").and_then(|g| g.as_array()) {
                for g in givens {
                    if let Some(given) = g.as_str() {
                        indices.push(("given".to_string(), given.to_lowercase()));
                        indices.push(("name".to_string(), given.to_lowercase()));
                    }
                }
            }
        }
    } else if let Some(name_str) = data.get("name").and_then(|n| n.as_str()) {
        indices.push(("name".to_string(), name_str.to_lowercase()));
    }

    // 2. gender
    if let Some(gender) = data.get("gender").and_then(|g| g.as_str()) {
        indices.push(("gender".to_string(), gender.to_lowercase()));
    }

    // 3. birthDate
    if let Some(birth_date) = data.get("birthDate").and_then(|b| b.as_str()) {
        indices.push(("birthdate".to_string(), birth_date.to_string()));
    }

    // 4. active
    if let Some(active) = data.get("active").and_then(|a| a.as_bool()) {
        indices.push(("active".to_string(), active.to_string()));
    }

    // 5. status
    if let Some(status) = data.get("status").and_then(|s| s.as_str()) {
        indices.push(("status".to_string(), status.to_lowercase()));
    }

    // 6. code
    if let Some(code_obj) = data.get("code") {
        if let Some(codings) = code_obj.get("coding").and_then(|c| c.as_array()) {
            for c in codings {
                let code = c.get("code").and_then(|v| v.as_str()).unwrap_or("");
                let system = c.get("system").and_then(|v| v.as_str()).unwrap_or("");
                if !code.is_empty() {
                    indices.push(("code".to_string(), code.to_lowercase()));
                    if !system.is_empty() {
                        indices.push(("code".to_string(), format!("{}|{}", system.to_lowercase(), code.to_lowercase())));
                    }
                }
            }
        }
        if let Some(text) = code_obj.get("text").and_then(|t| t.as_str()) {
            indices.push(("code".to_string(), text.to_lowercase()));
        }
    }

    // 7. identifier
    if let Some(identifiers) = data.get("identifier").and_then(|i| i.as_array()) {
        for id in identifiers {
            let val = id.get("value").and_then(|v| v.as_str()).unwrap_or("");
            let system = id.get("system").and_then(|v| v.as_str()).unwrap_or("");
            if !val.is_empty() {
                indices.push(("identifier".to_string(), val.to_lowercase()));
                if !system.is_empty() {
                    indices.push(("identifier".to_string(), format!("{}|{}", system.to_lowercase(), val.to_lowercase())));
                }
            }
        }
    }

    // 8. subject & patient references
    for ref_field in &["subject", "patient"] {
        if let Some(reference) = data.get(*ref_field).and_then(|r| r.get("reference")).and_then(|v| v.as_str()) {
            indices.push((ref_field.to_string(), reference.to_lowercase()));
            if let Some((_, raw_id)) = reference.split_once('/') {
                indices.push((ref_field.to_string(), raw_id.to_lowercase()));
            }
        }
    }

    indices
}
