use crate::model::outcome::OperationOutcome;
use crate::model::resource::StoredResource;
use crate::storage::StorageError;
use axum::{
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;

pub const FHIR_CONTENT_TYPE: &str = "application/fhir+json; charset=utf-8";

pub struct FhirOutcomeResponse {
    pub status: StatusCode,
    pub outcome: OperationOutcome,
}

impl IntoResponse for FhirOutcomeResponse {
    fn into_response(self) -> Response {
        let mut res = (self.status, Json(self.outcome.to_value())).into_response();
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(FHIR_CONTENT_TYPE),
        );
        res
    }
}

pub struct FhirResourceResponse {
    pub status: StatusCode,
    pub resource: StoredResource,
    pub location: Option<String>,
}

impl IntoResponse for FhirResourceResponse {
    fn into_response(self) -> Response {
        let mut res = (self.status, Json(self.resource.to_json())).into_response();
        let headers = res.headers_mut();

        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(FHIR_CONTENT_TYPE),
        );

        if let Ok(etag_val) = HeaderValue::from_str(&self.resource.etag()) {
            headers.insert(header::ETAG, etag_val);
        }

        if let Some(loc) = self.location {
            if let Ok(loc_val) = HeaderValue::from_str(&loc) {
                headers.insert(header::LOCATION, loc_val);
            }
        }

        res
    }
}

pub struct FhirJsonResponse {
    pub status: StatusCode,
    pub body: Value,
}

impl IntoResponse for FhirJsonResponse {
    fn into_response(self) -> Response {
        let mut res = (self.status, Json(self.body)).into_response();
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(FHIR_CONTENT_TYPE),
        );
        res
    }
}

pub fn outcome_error(err: StorageError) -> Response {
    FhirOutcomeResponse {
        status: err.status_code(),
        outcome: err.to_outcome(),
    }
    .into_response()
}

pub fn outcome_bad_request(msg: &str) -> Response {
    FhirOutcomeResponse {
        status: StatusCode::BAD_REQUEST,
        outcome: OperationOutcome::bad_request(msg),
    }
    .into_response()
}
