pub mod mem;

use crate::model::outcome::OperationOutcome;
use crate::model::resource::StoredResource;
use crate::search::engine::SearchResult;
use crate::search::params::SearchParams;
use serde_json::Value;

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

pub trait FhirStorage: Send + Sync {
    fn create(&self, resource_type: &str, resource: Value) -> Result<StoredResource, StorageError>;
    fn read(&self, resource_type: &str, id: &str) -> Result<StoredResource, StorageError>;
    fn vread(&self, resource_type: &str, id: &str, version_id: &str) -> Result<StoredResource, StorageError>;
    fn update(
        &self,
        resource_type: &str,
        id: &str,
        resource: Value,
        if_match: Option<&str>,
    ) -> Result<StoredResource, StorageError>;
    fn delete(&self, resource_type: &str, id: &str) -> Result<(), StorageError>;
    fn history_instance(&self, resource_type: &str, id: &str) -> Result<Vec<StoredResource>, StorageError>;
    fn history_type(&self, resource_type: &str) -> Result<Vec<StoredResource>, StorageError>;
    fn history_system(&self) -> Result<Vec<StoredResource>, StorageError>;
    fn search(&self, resource_type: &str, params: &SearchParams) -> Result<SearchResult, StorageError>;
    fn process_bundle(&self, bundle: Value) -> Result<Value, StorageError>;
}
