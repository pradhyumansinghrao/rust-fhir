use axum::{
    body::{to_bytes, Body},
    http::{header, Request},
    Router,
};
use rust_fhir::{create_router, AppState, MemoryStorage, ServerConfig};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;

struct TestClient {
    router: Router,
}

impl TestClient {
    fn new() -> Self {
        let config = ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 3000,
            base_url: "http://127.0.0.1:3000".to_string(),
            default_page_size: 20,
            max_page_size: 100,
        };

        let storage = Arc::new(MemoryStorage::new());
        let state = AppState { config, storage };
        let router = create_router(state);

        Self { router }
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        headers: &[(&str, &str)],
    ) -> (u16, HashMap<String, String>, Value) {
        let mut builder = Request::builder().method(method).uri(path);

        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/fhir+json");
        }

        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }

        let body_content = body.map(|b| b.to_string()).unwrap_or_default();
        let req = builder.body(Body::from(body_content)).unwrap();

        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status().as_u16();

        let mut resp_headers = HashMap::new();
        for (k, v) in response.headers() {
            if let Ok(val) = v.to_str() {
                resp_headers.insert(k.as_str().to_lowercase(), val.to_string());
            }
        }

        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json_body: Value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };

        (status, resp_headers, json_body)
    }
}

#[tokio::test]
async fn test_health_check() {
    let client = TestClient::new();
    let (status, _headers, body) = client.request("GET", "/health", None, &[]).await;

    assert_eq!(status, 200);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "rust-fhir-server");
}

#[tokio::test]
async fn test_capability_statement_metadata() {
    let client = TestClient::new();
    let (status, _headers, body) = client.request("GET", "/metadata", None, &[]).await;

    assert_eq!(status, 200);
    assert_eq!(body["resourceType"], "CapabilityStatement");
    assert_eq!(body["fhirVersion"], "4.0.1");
    assert_eq!(body["status"], "active");
    assert_eq!(body["software"]["name"], "rust-fhir-server");

    // Verify REST resource declarations
    let rest = &body["rest"][0];
    assert_eq!(rest["mode"], "server");

    let resources = rest["resource"].as_array().expect("resources array");
    let has_patient = resources.iter().any(|r| r["type"] == "Patient");
    let has_observation = resources.iter().any(|r| r["type"] == "Observation");
    assert!(has_patient);
    assert!(has_observation);
}

#[tokio::test]
async fn test_patient_crud_lifecycle_and_versioning() {
    let client = TestClient::new();

    // 1. Create Patient
    let patient_payload = json!({
        "resourceType": "Patient",
        "name": [{
            "family": "Smith",
            "given": ["John", "Alexander"]
        }],
        "gender": "male",
        "birthDate": "1980-05-15"
    });

    let (create_status, create_headers, create_body) = client
        .request("POST", "/Patient", Some(&patient_payload), &[])
        .await;

    assert_eq!(create_status, 201);
    let patient_id = create_body["id"].as_str().expect("assigned ID").to_string();
    assert_eq!(create_body["meta"]["versionId"], "1");
    assert!(create_body["meta"]["lastUpdated"].is_string());
    assert_eq!(create_headers.get("etag").unwrap(), "W/\"1\"");
    assert!(create_headers.get("location").unwrap().contains(&patient_id));

    // 2. Read Patient
    let (read_status, read_headers, read_body) = client
        .request("GET", &format!("/Patient/{patient_id}"), None, &[])
        .await;

    assert_eq!(read_status, 200);
    assert_eq!(read_body["id"], patient_id);
    assert_eq!(read_body["name"][0]["family"], "Smith");
    assert_eq!(read_headers.get("etag").unwrap(), "W/\"1\"");

    // 3. Update Patient (Version 2)
    let updated_payload = json!({
        "resourceType": "Patient",
        "id": patient_id,
        "name": [{
            "family": "Smith-Jones",
            "given": ["John"]
        }],
        "gender": "male",
        "birthDate": "1980-05-15"
    });

    let (update_status, update_headers, update_body) = client
        .request(
            "PUT",
            &format!("/Patient/{patient_id}"),
            Some(&updated_payload),
            &[("if-match", "W/\"1\"")],
        )
        .await;

    assert_eq!(update_status, 200);
    assert_eq!(update_body["meta"]["versionId"], "2");
    assert_eq!(update_body["name"][0]["family"], "Smith-Jones");
    assert_eq!(update_headers.get("etag").unwrap(), "W/\"2\"");

    // 4. VRead Version 1 and Version 2
    let (vread1_status, _, vread1_body) = client
        .request("GET", &format!("/Patient/{patient_id}/_history/1"), None, &[])
        .await;
    assert_eq!(vread1_status, 200);
    assert_eq!(vread1_body["meta"]["versionId"], "1");
    assert_eq!(vread1_body["name"][0]["family"], "Smith");

    let (vread2_status, _, vread2_body) = client
        .request("GET", &format!("/Patient/{patient_id}/_history/2"), None, &[])
        .await;
    assert_eq!(vread2_status, 200);
    assert_eq!(vread2_body["meta"]["versionId"], "2");
    assert_eq!(vread2_body["name"][0]["family"], "Smith-Jones");

    // 5. History
    let (hist_status, _, hist_body) = client
        .request("GET", &format!("/Patient/{patient_id}/_history"), None, &[])
        .await;
    assert_eq!(hist_status, 200);
    assert_eq!(hist_body["resourceType"], "Bundle");
    assert_eq!(hist_body["type"], "history");
    assert_eq!(hist_body["total"], 2);

    // 6. Delete Patient
    let (del_status, _, _) = client
        .request("DELETE", &format!("/Patient/{patient_id}"), None, &[])
        .await;
    assert_eq!(del_status, 204);

    // 7. Read after deletion returns 410 Gone with OperationOutcome
    let (gone_status, _, gone_body) = client
        .request("GET", &format!("/Patient/{patient_id}"), None, &[])
        .await;
    assert_eq!(gone_status, 410);
    assert_eq!(gone_body["resourceType"], "OperationOutcome");
    assert_eq!(gone_body["issue"][0]["code"], "deleted");
}

#[tokio::test]
async fn test_optimistic_locking_if_match() {
    let client = TestClient::new();

    let payload = json!({
        "resourceType": "Patient",
        "name": [{"family": "Taylor"}]
    });

    let (_, _, create_body) = client.request("POST", "/Patient", Some(&payload), &[]).await;
    let patient_id = create_body["id"].as_str().unwrap();

    // Try update with wrong version in If-Match header
    let update_payload = json!({
        "resourceType": "Patient",
        "id": patient_id,
        "name": [{"family": "Taylor-Modified"}]
    });

    let (status, _, err_body) = client
        .request(
            "PUT",
            &format!("/Patient/{patient_id}"),
            Some(&update_payload),
            &[("if-match", "W/\"999\"")],
        )
        .await;

    assert_eq!(status, 412);
    assert_eq!(err_body["resourceType"], "OperationOutcome");
    assert_eq!(err_body["issue"][0]["code"], "conflict");
}

#[tokio::test]
async fn test_search_parameters() {
    let client = TestClient::new();

    // Create Patient 1: Alice Wonderland (female)
    let p1 = json!({
        "resourceType": "Patient",
        "name": [{"family": "Wonderland", "given": ["Alice"]}],
        "gender": "female",
        "active": true
    });
    client.request("POST", "/Patient", Some(&p1), &[]).await;

    // Create Patient 2: Bob Builder (male)
    let p2 = json!({
        "resourceType": "Patient",
        "name": [{"family": "Builder", "given": ["Bob"]}],
        "gender": "male",
        "active": true
    });
    client.request("POST", "/Patient", Some(&p2), &[]).await;

    // Create Patient 3: Charlie Brown (male)
    let p3 = json!({
        "resourceType": "Patient",
        "name": [{"family": "Brown", "given": ["Charlie"]}],
        "gender": "male",
        "active": false
    });
    client.request("POST", "/Patient", Some(&p3), &[]).await;

    // Search by name "Alice"
    let (s1, _, b1) = client.request("GET", "/Patient?name=Alice", None, &[]).await;
    assert_eq!(s1, 200);
    assert_eq!(b1["resourceType"], "Bundle");
    assert_eq!(b1["type"], "searchset");
    assert_eq!(b1["total"], 1);
    assert_eq!(b1["entry"][0]["resource"]["name"][0]["given"][0], "Alice");

    // Search by gender "male"
    let (s2, _, b2) = client.request("GET", "/Patient?gender=male", None, &[]).await;
    assert_eq!(s2, 200);
    assert_eq!(b2["total"], 2);

    // Search by gender with multiple comma-separated values (OR condition)
    let (s3, _, b3) = client.request("GET", "/Patient?gender=female,male", None, &[]).await;
    assert_eq!(s3, 200);
    assert_eq!(b3["total"], 3);

    // Search with pagination _count=1
    let (s4, _, b4) = client.request("GET", "/Patient?gender=male&_count=1", None, &[]).await;
    assert_eq!(s4, 200);
    assert_eq!(b4["total"], 2);
    assert_eq!(b4["entry"].as_array().unwrap().len(), 1);
    assert!(b4["link"].is_array());
}

#[tokio::test]
async fn test_bundle_transaction_atomic_success() {
    let client = TestClient::new();

    let transaction_bundle = json!({
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
                    "id": "tx-patient-1",
                    "name": [{"family": "TxPatient"}]
                }
            },
            {
                "request": {
                    "method": "POST",
                    "url": "Observation"
                },
                "resource": {
                    "resourceType": "Observation",
                    "id": "tx-obs-1",
                    "status": "final",
                    "code": {
                        "coding": [{
                            "system": "http://loinc.org",
                            "code": "883-9"
                        }]
                    },
                    "subject": {
                        "reference": "Patient/tx-patient-1"
                    }
                }
            }
        ]
    });

    let (status, _, body) = client
        .request("POST", "/", Some(&transaction_bundle), &[])
        .await;

    assert_eq!(status, 200);
    assert_eq!(body["resourceType"], "Bundle");
    assert_eq!(body["type"], "transaction-response");
    assert_eq!(body["entry"].as_array().unwrap().len(), 2);
    assert_eq!(body["entry"][0]["response"]["status"], "201 Created");
    assert_eq!(body["entry"][1]["response"]["status"], "201 Created");

    // Verify both were committed
    let (p_status, _, p_body) = client.request("GET", "/Patient/tx-patient-1", None, &[]).await;
    assert_eq!(p_status, 200);
    assert_eq!(p_body["name"][0]["family"], "TxPatient");

    let (o_status, _, o_body) = client.request("GET", "/Observation/tx-obs-1", None, &[]).await;
    assert_eq!(o_status, 200);
    assert_eq!(o_body["status"], "final");
}

#[tokio::test]
async fn test_bundle_transaction_atomic_rollback() {
    let client = TestClient::new();

    let failing_tx = json!({
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
                    "id": "tx-rollback-patient",
                    "name": [{"family": "ShouldRollBack"}]
                }
            },
            {
                // Invalid entry: unsupported method
                "request": {
                    "method": "UNKNOWN_METHOD",
                    "url": "Patient"
                }
            }
        ]
    });

    let (status, _, body) = client.request("POST", "/", Some(&failing_tx), &[]).await;

    assert_eq!(status, 400);
    assert_eq!(body["resourceType"], "OperationOutcome");

    // Verify Patient was NOT persisted due to rollback
    let (read_status, _, _) = client
        .request("GET", "/Patient/tx-rollback-patient", None, &[])
        .await;
    assert_eq!(read_status, 404);
}

#[tokio::test]
async fn test_validate_endpoint() {
    let client = TestClient::new();

    // Valid resource
    let valid_patient = json!({
        "resourceType": "Patient",
        "name": [{"family": "Valid"}]
    });
    let (s1, _, b1) = client
        .request("POST", "/Patient/$validate", Some(&valid_patient), &[])
        .await;
    assert_eq!(s1, 200);
    assert_eq!(b1["resourceType"], "OperationOutcome");
    assert_eq!(b1["issue"][0]["severity"], "information");

    // Invalid resource: mismatched resourceType
    let invalid_patient = json!({
        "resourceType": "Observation",
        "status": "final"
    });
    let (s2, _, b2) = client
        .request("POST", "/Patient/$validate", Some(&invalid_patient), &[])
        .await;
    assert_eq!(s2, 422);
    assert_eq!(b2["resourceType"], "OperationOutcome");
    assert_eq!(b2["issue"][0]["severity"], "error");
}
