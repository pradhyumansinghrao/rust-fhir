use crate::model::bundle::{Bundle, BundleEntry, BundleResponse, BundleType};
use crate::model::outcome::OperationOutcome;
use crate::model::resource::{generate_uuid, is_valid_id, StoredResource};
use crate::search::engine::{SearchEngine, SearchResult};
use crate::search::params::SearchParams;
use crate::storage::{FhirStorage, StorageError};
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Default)]
struct MemoryState {
    resources: HashMap<(String, String), StoredResource>,
    history: HashMap<(String, String), Vec<StoredResource>>,
}

pub struct MemoryStorage {
    state: Arc<RwLock<MemoryState>>,
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(MemoryState::default())),
        }
    }
}

#[async_trait]
impl FhirStorage for MemoryStorage {
    async fn create(&self, resource_type: &str, resource: Value) -> Result<StoredResource, StorageError> {
        let mut state = self.state.write();

        // Extract or generate resource ID
        let id = match resource.get("id").and_then(|i| i.as_str()) {
            Some(existing_id) => {
                if !is_valid_id(existing_id) {
                    return Err(StorageError::BadRequest(format!(
                        "Invalid resource ID '{existing_id}'. ID must match [A-Za-z0-9\\-\\.]{{1,64}}"
                    )));
                }
                existing_id.to_string()
            }
            None => generate_uuid(),
        };

        let key = (resource_type.to_string(), id.clone());

        // Ensure active resource does not already exist
        if let Some(existing) = state.resources.get(&key) {
            if !existing.deleted {
                return Err(StorageError::Conflict(format!(
                    "Resource {resource_type}/{id} already exists"
                )));
            }
        }

        // Determine version ID
        let version_id = match state.history.get(&key) {
            Some(hist) => (hist.len() + 1).to_string(),
            None => "1".to_string(),
        };

        // Create stored resource
        let stored = StoredResource::new(resource_type.to_string(), id.clone(), version_id, resource);

        state.resources.insert(key.clone(), stored.clone());
        state.history.entry(key).or_default().push(stored.clone());

        Ok(stored)
    }

    async fn read(&self, resource_type: &str, id: &str) -> Result<StoredResource, StorageError> {
        let state = self.state.read();
        let key = (resource_type.to_string(), id.to_string());

        match state.resources.get(&key) {
            Some(res) => {
                if res.deleted {
                    Err(StorageError::Deleted(format!(
                        "Resource {resource_type}/{id} has been deleted"
                    )))
                } else {
                    Ok(res.clone())
                }
            }
            None => {
                // Check if it exists in history as deleted
                if let Some(hist) = state.history.get(&key) {
                    if let Some(last) = hist.last() {
                        if last.deleted {
                            return Err(StorageError::Deleted(format!(
                                "Resource {resource_type}/{id} has been deleted"
                            )));
                        }
                    }
                }
                Err(StorageError::NotFound(format!(
                    "Resource {resource_type}/{id} not found"
                )))
            }
        }
    }

    async fn vread(&self, resource_type: &str, id: &str, version_id: &str) -> Result<StoredResource, StorageError> {
        let state = self.state.read();
        let key = (resource_type.to_string(), id.to_string());

        let history = state.history.get(&key).ok_or_else(|| {
            StorageError::NotFound(format!("Resource {resource_type}/{id} not found"))
        })?;

        for item in history {
            if item.version_id == version_id {
                if item.deleted {
                    return Err(StorageError::Deleted(format!(
                        "Resource {resource_type}/{id}/_history/{version_id} has been deleted"
                    )));
                } else {
                    return Ok(item.clone());
                }
            }
        }

        Err(StorageError::NotFound(format!(
            "Resource {resource_type}/{id} with version {version_id} not found"
        )))
    }

    async fn update(
        &self,
        resource_type: &str,
        id: &str,
        resource: Value,
        if_match: Option<&str>,
    ) -> Result<StoredResource, StorageError> {
        if !is_valid_id(id) {
            return Err(StorageError::BadRequest(format!(
                "Invalid resource ID '{id}'"
            )));
        }

        let mut state = self.state.write();
        let key = (resource_type.to_string(), id.to_string());

        // Check optimistic locking If-Match header
        if let Some(match_header) = if_match {
            let clean_match = match_header.trim().trim_start_matches("W/").trim_matches('"');
            if let Some(existing) = state.resources.get(&key) {
                if !existing.deleted && existing.version_id != clean_match {
                    return Err(StorageError::PreconditionFailed(format!(
                        "Precondition failed: current version is '{}', but If-Match was '{}'",
                        existing.version_id, clean_match
                    )));
                }
            }
        }

        let next_version = match state.history.get(&key) {
            Some(hist) => (hist.len() + 1).to_string(),
            None => "1".to_string(),
        };

        let stored = StoredResource::new(resource_type.to_string(), id.to_string(), next_version, resource);

        state.resources.insert(key.clone(), stored.clone());
        state.history.entry(key).or_default().push(stored.clone());

        Ok(stored)
    }

    async fn delete(&self, resource_type: &str, id: &str) -> Result<(), StorageError> {
        let mut state = self.state.write();
        let key = (resource_type.to_string(), id.to_string());

        let next_version = match state.resources.get(&key) {
            Some(res) if !res.deleted => {
                let hist_len = state.history.get(&key).map(|h| h.len()).unwrap_or(1);
                (hist_len + 1).to_string()
            }
            _ => {
                return Err(StorageError::NotFound(format!(
                    "Resource {resource_type}/{id} not found"
                )));
            }
        };

        let tombstone = StoredResource::new_deleted(resource_type.to_string(), id.to_string(), next_version);
        state.resources.insert(key.clone(), tombstone.clone());
        state.history.entry(key).or_default().push(tombstone);

        Ok(())
    }

    async fn history_instance(&self, resource_type: &str, id: &str) -> Result<Vec<StoredResource>, StorageError> {
        let state = self.state.read();
        let key = (resource_type.to_string(), id.to_string());

        let hist = state.history.get(&key).ok_or_else(|| {
            StorageError::NotFound(format!("Resource {resource_type}/{id} not found"))
        })?;

        let mut sorted = hist.clone();
        sorted.sort_by(|a, b| b.last_updated.cmp(&a.last_updated));
        Ok(sorted)
    }

    async fn history_type(&self, resource_type: &str) -> Result<Vec<StoredResource>, StorageError> {
        let state = self.state.read();
        let mut all_type = Vec::new();

        for ((r_type, _), versions) in &state.history {
            if r_type == resource_type {
                all_type.extend(versions.clone());
            }
        }

        all_type.sort_by(|a, b| b.last_updated.cmp(&a.last_updated));
        Ok(all_type)
    }

    async fn history_system(&self) -> Result<Vec<StoredResource>, StorageError> {
        let state = self.state.read();
        let mut all = Vec::new();

        for versions in state.history.values() {
            all.extend(versions.clone());
        }

        all.sort_by(|a, b| b.last_updated.cmp(&a.last_updated));
        Ok(all)
    }

    async fn search(&self, resource_type: &str, params: &SearchParams) -> Result<SearchResult, StorageError> {
        let state = self.state.read();
        let matching_resources: Vec<StoredResource> = state
            .resources
            .iter()
            .filter(|((r_type, _), res)| r_type == resource_type && !res.deleted)
            .map(|(_, res)| res.clone())
            .collect();

        Ok(SearchEngine::evaluate(&matching_resources, params))
    }

    async fn process_bundle(&self, bundle: Value) -> Result<Value, StorageError> {
        let bundle_type = bundle
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("");

        let entries = bundle
            .get("entry")
            .and_then(|e| e.as_array())
            .cloned()
            .unwrap_or_default();

        match bundle_type {
            "transaction" => self.execute_transaction(&entries).await,
            "batch" => self.execute_batch(&entries).await,
            _ => Err(StorageError::BadRequest(format!(
                "Unsupported bundle type '{bundle_type}'. Server accepts 'transaction' or 'batch'."
            ))),
        }
    }
}

impl MemoryStorage {
    async fn execute_transaction(&self, entries: &[Value]) -> Result<Value, StorageError> {
        // Clone state for atomic rollback
        let backup_state = self.state.read().clone();
        let mut response_entries = Vec::new();

        for entry in entries {
            let request = entry.get("request").ok_or_else(|| {
                StorageError::BadRequest("Transaction entry must contain a 'request' element".to_string())
            })?;

            let method = request
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_uppercase();

            let url = request
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("");

            let res_result = self.execute_entry_operation(
                &method,
                url,
                entry.get("resource").cloned(),
                request.get("ifMatch").and_then(|m| m.as_str()),
            ).await;

            match res_result {
                Ok(resp_entry) => response_entries.push(resp_entry),
                Err(err) => {
                    // Rollback
                    *self.state.write() = backup_state;
                    return Err(err);
                }
            }
        }

        let resp_bundle = Bundle {
            resource_type: "Bundle".to_string(),
            id: Some(generate_uuid()),
            bundle_type: BundleType::TransactionResponse,
            total: None,
            link: None,
            entry: response_entries,
        };

        Ok(resp_bundle.to_value())
    }

    async fn execute_batch(&self, entries: &[Value]) -> Result<Value, StorageError> {
        let mut response_entries = Vec::new();

        for entry in entries {
            let request = match entry.get("request") {
                Some(r) => r,
                None => {
                    response_entries.push(BundleEntry {
                        full_url: None,
                        resource: Some(OperationOutcome::bad_request("Missing request in entry").to_value()),
                        search: None,
                        request: None,
                        response: Some(BundleResponse {
                            status: "400 Bad Request".to_string(),
                            location: None,
                            etag: None,
                            last_modified: None,
                            outcome: None,
                        }),
                    });
                    continue;
                }
            };

            let method = request
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_uppercase();

            let url = request
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("");

            match self.execute_entry_operation(
                &method,
                url,
                entry.get("resource").cloned(),
                request.get("ifMatch").and_then(|m| m.as_str()),
            ).await {
                Ok(resp_entry) => response_entries.push(resp_entry),
                Err(err) => {
                    let status = format!("{} {}", err.status_code().as_u16(), err.status_code().canonical_reason().unwrap_or(""));
                    response_entries.push(BundleEntry {
                        full_url: None,
                        resource: Some(err.to_outcome().to_value()),
                        search: None,
                        request: None,
                        response: Some(BundleResponse {
                            status,
                            location: None,
                            etag: None,
                            last_modified: None,
                            outcome: Some(err.to_outcome().to_value()),
                        }),
                    });
                }
            }
        }

        let resp_bundle = Bundle {
            resource_type: "Bundle".to_string(),
            id: Some(generate_uuid()),
            bundle_type: BundleType::BatchResponse,
            total: None,
            link: None,
            entry: response_entries,
        };

        Ok(resp_bundle.to_value())
    }

    async fn execute_entry_operation(
        &self,
        method: &str,
        url: &str,
        resource: Option<Value>,
        if_match: Option<&str>,
    ) -> Result<BundleEntry, StorageError> {
        let trimmed_url = url.trim().trim_start_matches('/');
        let url_parts: Vec<&str> = trimmed_url.split('?').collect();
        let path = url_parts[0];
        let query_str = url_parts.get(1).copied().unwrap_or("");
        let path_segments: Vec<&str> = path.split('/').collect();

        match method {
            "POST" => {
                let resource_type = path_segments.first().ok_or_else(|| {
                    StorageError::BadRequest("Missing resource type in POST url".to_string())
                })?;
                let body = resource.ok_or_else(|| {
                    StorageError::BadRequest("Missing resource body for POST".to_string())
                })?;

                let stored = self.create(resource_type, body).await?;
                let location = format!("{resource_type}/{}/_history/{}", stored.id, stored.version_id);

                Ok(BundleEntry {
                    full_url: Some(location.clone()),
                    resource: Some(stored.to_json()),
                    search: None,
                    request: None,
                    response: Some(BundleResponse {
                        status: "201 Created".to_string(),
                        location: Some(location),
                        etag: Some(stored.etag()),
                        last_modified: Some(stored.last_updated.clone()),
                        outcome: None,
                    }),
                })
            }
            "GET" => {
                if path_segments.len() == 2 {
                    let (res_type, id) = (path_segments[0], path_segments[1]);
                    let stored = self.read(res_type, id).await?;
                    Ok(BundleEntry {
                        full_url: Some(format!("{res_type}/{id}")),
                        resource: Some(stored.to_json()),
                        search: None,
                        request: None,
                        response: Some(BundleResponse {
                            status: "200 OK".to_string(),
                            location: None,
                            etag: Some(stored.etag()),
                            last_modified: Some(stored.last_updated.clone()),
                            outcome: None,
                        }),
                    })
                } else if path_segments.len() == 1 {
                    let res_type = path_segments[0];
                    let params = SearchParams::parse_query(query_str, 20, 100);
                    let result = self.search(res_type, &params).await?;

                    let entries: Vec<BundleEntry> = result
                        .items
                        .into_iter()
                        .map(|item| BundleEntry {
                            full_url: Some(format!("{res_type}/{}", item.id)),
                            resource: Some(item.to_json()),
                            search: Some(crate::model::bundle::BundleSearch {
                                mode: "match".to_string(),
                            }),
                            request: None,
                            response: None,
                        })
                        .collect();

                    let bundle = Bundle::new_searchset(result.total, entries, Vec::new());
                    Ok(BundleEntry {
                        full_url: None,
                        resource: Some(bundle.to_value()),
                        search: None,
                        request: None,
                        response: Some(BundleResponse {
                            status: "200 OK".to_string(),
                            location: None,
                            etag: None,
                            last_modified: None,
                            outcome: None,
                        }),
                    })
                } else {
                    Err(StorageError::BadRequest(format!("Unsupported GET path: {path}")))
                }
            }
            "PUT" => {
                if path_segments.len() != 2 {
                    return Err(StorageError::BadRequest("PUT url must be [resourceType]/[id]".to_string()));
                }
                let (res_type, id) = (path_segments[0], path_segments[1]);
                let body = resource.ok_or_else(|| {
                    StorageError::BadRequest("Missing resource body for PUT".to_string())
                })?;

                let stored = self.update(res_type, id, body, if_match).await?;
                Ok(BundleEntry {
                    full_url: Some(format!("{res_type}/{id}")),
                    resource: Some(stored.to_json()),
                    search: None,
                    request: None,
                    response: Some(BundleResponse {
                        status: "200 OK".to_string(),
                        location: Some(format!("{res_type}/{id}/_history/{}", stored.version_id)),
                        etag: Some(stored.etag()),
                        last_modified: Some(stored.last_updated.clone()),
                        outcome: None,
                    }),
                })
            }
            "DELETE" => {
                if path_segments.len() != 2 {
                    return Err(StorageError::BadRequest("DELETE url must be [resourceType]/[id]".to_string()));
                }
                let (res_type, id) = (path_segments[0], path_segments[1]);
                self.delete(res_type, id).await?;
                Ok(BundleEntry {
                    full_url: Some(format!("{res_type}/{id}")),
                    resource: None,
                    search: None,
                    request: None,
                    response: Some(BundleResponse {
                        status: "204 No Content".to_string(),
                        location: None,
                        etag: None,
                        last_modified: None,
                        outcome: None,
                    }),
                })
            }
            _ => Err(StorageError::BadRequest(format!("Unsupported method: {method}"))),
        }
    }
}
