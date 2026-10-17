use crate::config::ServerConfig;
use crate::model::bundle::{Bundle, BundleEntry, BundleLink, BundleSearch};
use crate::model::capability::make_capability_statement;
use crate::model::outcome::OperationOutcome;
use crate::model::resource::{is_valid_id, is_valid_resource_type, StoredResource};
use crate::search::params::SearchParams;
use crate::server::extractors::{get_if_match, get_if_none_match};
use crate::server::responses::{
    outcome_bad_request, outcome_error, FhirJsonResponse, FhirOutcomeResponse,
    FhirResourceResponse,
};
use crate::storage::FhirStorage;
use axum::{
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: ServerConfig,
    pub storage: Arc<dyn FhirStorage>,
}

pub async fn health() -> Response {
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "service": "rust-fhir-server"
        })),
    )
        .into_response()
}

pub async fn metadata(State(state): State<AppState>) -> Response {
    let cap = make_capability_statement(&state.config.base_url);
    FhirJsonResponse {
        status: StatusCode::OK,
        body: cap,
    }
    .into_response()
}

pub async fn create_resource(
    Path(resource_type): Path<String>,
    State(state): State<AppState>,
    Json(mut payload): Json<Value>,
) -> Response {
    if !is_valid_resource_type(&resource_type) {
        return outcome_bad_request(&format!("Invalid resource type '{resource_type}'"));
    }

    if let Some(body_type) = payload.get("resourceType").and_then(|t| t.as_str()) {
        if body_type != resource_type {
            return outcome_bad_request(&format!(
                "Path resource type '{resource_type}' does not match body resource type '{body_type}'"
            ));
        }
    } else if let Value::Object(ref mut map) = payload {
        map.insert(
            "resourceType".to_string(),
            Value::String(resource_type.clone()),
        );
    }

    match state.storage.create(&resource_type, payload).await {
        Ok(stored) => {
            let loc = format!("{}/{}/_history/{}", resource_type, stored.id, stored.version_id);
            FhirResourceResponse {
                status: StatusCode::CREATED,
                resource: stored,
                location: Some(loc),
            }
            .into_response()
        }
        Err(err) => outcome_error(err),
    }
}

pub async fn read_resource(
    Path((resource_type, id)): Path<(String, String)>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if !is_valid_resource_type(&resource_type) {
        return outcome_bad_request(&format!("Invalid resource type '{resource_type}'"));
    }
    if !is_valid_id(&id) {
        return outcome_bad_request(&format!("Invalid resource ID '{id}'"));
    }

    match state.storage.read(&resource_type, &id).await {
        Ok(stored) => {
            if let Some(if_none_match) = get_if_none_match(&headers) {
                let clean_match = if_none_match.trim().trim_start_matches("W/").trim_matches('"');
                if clean_match == stored.version_id {
                    return StatusCode::NOT_MODIFIED.into_response();
                }
            }

            FhirResourceResponse {
                status: StatusCode::OK,
                resource: stored,
                location: None,
            }
            .into_response()
        }
        Err(err) => outcome_error(err),
    }
}

pub async fn vread_resource(
    Path((resource_type, id, version_id)): Path<(String, String, String)>,
    State(state): State<AppState>,
) -> Response {
    if !is_valid_resource_type(&resource_type) {
        return outcome_bad_request(&format!("Invalid resource type '{resource_type}'"));
    }
    if !is_valid_id(&id) {
        return outcome_bad_request(&format!("Invalid resource ID '{id}'"));
    }

    match state.storage.vread(&resource_type, &id, &version_id).await {
        Ok(stored) => FhirResourceResponse {
            status: StatusCode::OK,
            resource: stored,
            location: None,
        }
        .into_response(),
        Err(err) => outcome_error(err),
    }
}

pub async fn update_resource(
    Path((resource_type, id)): Path<(String, String)>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut payload): Json<Value>,
) -> Response {
    if !is_valid_resource_type(&resource_type) {
        return outcome_bad_request(&format!("Invalid resource type '{resource_type}'"));
    }
    if !is_valid_id(&id) {
        return outcome_bad_request(&format!("Invalid resource ID '{id}'"));
    }

    // Verify ID matches if present in body
    if let Some(body_id) = payload.get("id").and_then(|i| i.as_str()) {
        if body_id != id {
            return outcome_bad_request(&format!(
                "Path id '{id}' does not match body id '{body_id}'"
            ));
        }
    } else if let Value::Object(ref mut map) = payload {
        map.insert("id".to_string(), Value::String(id.clone()));
    }

    // Verify resourceType matches if present
    if let Some(body_type) = payload.get("resourceType").and_then(|t| t.as_str()) {
        if body_type != resource_type {
            return outcome_bad_request(&format!(
                "Path resource type '{resource_type}' does not match body resource type '{body_type}'"
            ));
        }
    } else if let Value::Object(ref mut map) = payload {
        map.insert(
            "resourceType".to_string(),
            Value::String(resource_type.clone()),
        );
    }

    let if_match = get_if_match(&headers);

    match state.storage.update(&resource_type, &id, payload, if_match).await {
        Ok(stored) => {
            let loc = format!("{}/{}/_history/{}", resource_type, stored.id, stored.version_id);
            FhirResourceResponse {
                status: StatusCode::OK,
                resource: stored,
                location: Some(loc),
            }
            .into_response()
        }
        Err(err) => outcome_error(err),
    }
}

pub async fn delete_resource(
    Path((resource_type, id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Response {
    if !is_valid_resource_type(&resource_type) {
        return outcome_bad_request(&format!("Invalid resource type '{resource_type}'"));
    }
    if !is_valid_id(&id) {
        return outcome_bad_request(&format!("Invalid resource ID '{id}'"));
    }

    match state.storage.delete(&resource_type, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => outcome_error(err),
    }
}

pub async fn search_type(
    Path(resource_type): Path<String>,
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
) -> Response {
    if !is_valid_resource_type(&resource_type) {
        return outcome_bad_request(&format!("Invalid resource type '{resource_type}'"));
    }

    let query_str = raw_query.unwrap_or_default();
    let params = SearchParams::parse_query(
        &query_str,
        state.config.default_page_size,
        state.config.max_page_size,
    );

    match state.storage.search(&resource_type, &params).await {
        Ok(result) => {
            let entries: Vec<BundleEntry> = result
                .items
                .into_iter()
                .map(|item| BundleEntry {
                    full_url: Some(format!("{}/{}/{}", state.config.base_url, resource_type, item.id)),
                    resource: Some(item.to_json()),
                    search: Some(BundleSearch {
                        mode: "match".to_string(),
                    }),
                    request: None,
                    response: None,
                })
                .collect();

            let mut links = vec![BundleLink {
                relation: "self".to_string(),
                url: if query_str.is_empty() {
                    format!("{}/{}", state.config.base_url, resource_type)
                } else {
                    format!("{}/{}?{}", state.config.base_url, resource_type, query_str)
                },
            }];

            if result.offset + result.limit < result.total {
                let next_offset = result.offset + result.limit;
                links.push(BundleLink {
                    relation: "next".to_string(),
                    url: format!(
                        "{}/{}?_offset={}&_count={}",
                        state.config.base_url, resource_type, next_offset, result.limit
                    ),
                });
            }

            if result.offset > 0 {
                let prev_offset = result.offset.saturating_sub(result.limit);
                links.push(BundleLink {
                    relation: "previous".to_string(),
                    url: format!(
                        "{}/{}?_offset={}&_count={}",
                        state.config.base_url, resource_type, prev_offset, result.limit
                    ),
                });
            }

            let bundle = Bundle::new_searchset(result.total, entries, links);
            FhirJsonResponse {
                status: StatusCode::OK,
                body: bundle.to_value(),
            }
            .into_response()
        }
        Err(err) => outcome_error(err),
    }
}

pub async fn history_instance(
    Path((resource_type, id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Response {
    if !is_valid_resource_type(&resource_type) {
        return outcome_bad_request(&format!("Invalid resource type '{resource_type}'"));
    }
    if !is_valid_id(&id) {
        return outcome_bad_request(&format!("Invalid resource ID '{id}'"));
    }

    match state.storage.history_instance(&resource_type, &id).await {
        Ok(history) => {
            let entries = history_to_bundle_entries(&state.config.base_url, &resource_type, history);
            let bundle = Bundle::new_history(entries);
            FhirJsonResponse {
                status: StatusCode::OK,
                body: bundle.to_value(),
            }
            .into_response()
        }
        Err(err) => outcome_error(err),
    }
}

pub async fn history_type(
    Path(resource_type): Path<String>,
    State(state): State<AppState>,
) -> Response {
    if !is_valid_resource_type(&resource_type) {
        return outcome_bad_request(&format!("Invalid resource type '{resource_type}'"));
    }

    match state.storage.history_type(&resource_type).await {
        Ok(history) => {
            let entries = history_to_bundle_entries(&state.config.base_url, &resource_type, history);
            let bundle = Bundle::new_history(entries);
            FhirJsonResponse {
                status: StatusCode::OK,
                body: bundle.to_value(),
            }
            .into_response()
        }
        Err(err) => outcome_error(err),
    }
}

pub async fn history_system(State(state): State<AppState>) -> Response {
    match state.storage.history_system().await {
        Ok(history) => {
            let entries: Vec<BundleEntry> = history
                .into_iter()
                .map(|item| BundleEntry {
                    full_url: Some(format!(
                        "{}/{}/{}/_history/{}",
                        state.config.base_url, item.resource_type, item.id, item.version_id
                    )),
                    resource: if item.deleted { None } else { Some(item.to_json()) },
                    search: None,
                    request: None,
                    response: None,
                })
                .collect();

            let bundle = Bundle::new_history(entries);
            FhirJsonResponse {
                status: StatusCode::OK,
                body: bundle.to_value(),
            }
            .into_response()
        }
        Err(err) => outcome_error(err),
    }
}

pub async fn process_bundle(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Response {
    match state.storage.process_bundle(payload).await {
        Ok(resp_bundle) => FhirJsonResponse {
            status: StatusCode::OK,
            body: resp_bundle,
        }
        .into_response(),
        Err(err) => outcome_error(err),
    }
}

pub async fn validate_resource(
    Path(resource_type): Path<String>,
    Json(payload): Json<Value>,
) -> Response {
    if !is_valid_resource_type(&resource_type) {
        return outcome_bad_request(&format!("Invalid resource type '{resource_type}'"));
    }

    if let Some(body_type) = payload.get("resourceType").and_then(|t| t.as_str()) {
        if body_type != resource_type {
            return FhirOutcomeResponse {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                outcome: OperationOutcome::bad_request(&format!(
                    "Resource type '{body_type}' does not match endpoint type '{resource_type}'"
                )),
            }
            .into_response();
        }
    } else {
        return FhirOutcomeResponse {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            outcome: OperationOutcome::bad_request("Resource is missing 'resourceType' property"),
        }
        .into_response();
    }

    FhirOutcomeResponse {
        status: StatusCode::OK,
        outcome: OperationOutcome::success(&format!("{resource_type} validation successful")),
    }
    .into_response()
}

fn history_to_bundle_entries(
    base_url: &str,
    resource_type: &str,
    history: Vec<StoredResource>,
) -> Vec<BundleEntry> {
    history
        .into_iter()
        .map(|item| BundleEntry {
            full_url: Some(format!(
                "{}/{}/{}/_history/{}",
                base_url, resource_type, item.id, item.version_id
            )),
            resource: if item.deleted { None } else { Some(item.to_json()) },
            search: None,
            request: None,
            response: None,
        })
        .collect()
}
