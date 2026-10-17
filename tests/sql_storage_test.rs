use rust_fhir::search::params::SearchParams;
use rust_fhir::storage::sql::SqlStorage;
use rust_fhir::storage::FhirStorage;
use serde_json::json;

#[tokio::test]
async fn test_sqlite_crud_and_history() {
    let storage = SqlStorage::connect("sqlite://:memory:")
        .await
        .expect("Failed to connect to in-memory SQLite");

    // 1. Create Patient
    let patient_payload = json!({
        "resourceType": "Patient",
        "name": [{ "family": "Curie", "given": ["Marie"] }],
        "gender": "female",
        "birthDate": "1867-11-07"
    });

    let created = storage
        .create("Patient", patient_payload)
        .await
        .expect("Failed to create Patient");

    assert_eq!(created.version_id, "1");
    let patient_id = created.id.clone();

    // 2. Read Patient
    let read = storage
        .read("Patient", &patient_id)
        .await
        .expect("Failed to read Patient");
    assert_eq!(read.id, patient_id);
    assert_eq!(read.data["name"][0]["family"], "Curie");

    // 3. Update Patient
    let update_payload = json!({
        "resourceType": "Patient",
        "id": patient_id,
        "name": [{ "family": "Curie-Sklodowska", "given": ["Marie"] }],
        "gender": "female"
    });

    let updated = storage
        .update("Patient", &patient_id, update_payload, Some("W/\"1\""))
        .await
        .expect("Failed to update Patient");
    assert_eq!(updated.version_id, "2");
    assert_eq!(updated.data["name"][0]["family"], "Curie-Sklodowska");

    // 4. VRead Version 1 and 2
    let v1 = storage
        .vread("Patient", &patient_id, "1")
        .await
        .expect("Failed to vread v1");
    assert_eq!(v1.data["name"][0]["family"], "Curie");

    let v2 = storage
        .vread("Patient", &patient_id, "2")
        .await
        .expect("Failed to vread v2");
    assert_eq!(v2.data["name"][0]["family"], "Curie-Sklodowska");

    // 5. History
    let history = storage
        .history_instance("Patient", &patient_id)
        .await
        .expect("Failed to get history");
    assert_eq!(history.len(), 2);

    // 6. Delete
    storage
        .delete("Patient", &patient_id)
        .await
        .expect("Failed to delete Patient");

    // 7. Read returns Deleted (410)
    let read_after_del = storage.read("Patient", &patient_id).await;
    assert!(read_after_del.is_err());
    match read_after_del.unwrap_err() {
        rust_fhir::storage::StorageError::Deleted(_) => (),
        other => panic!("Expected Deleted error, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_sqlite_search() {
    let storage = SqlStorage::connect("sqlite://:memory:")
        .await
        .expect("Failed to connect to SQLite");

    // Insert 2 patients
    let p1 = json!({
        "resourceType": "Patient",
        "name": [{ "family": "Einstein", "given": ["Albert"] }],
        "gender": "male"
    });
    storage.create("Patient", p1).await.unwrap();

    let p2 = json!({
        "resourceType": "Patient",
        "name": [{ "family": "Franklin", "given": ["Rosalind"] }],
        "gender": "female"
    });
    storage.create("Patient", p2).await.unwrap();

    // Search by name
    let params_name = SearchParams::parse_query("name=Albert", 20, 100);
    let res_name = storage.search("Patient", &params_name).await.unwrap();
    assert_eq!(res_name.total, 1);
    assert_eq!(res_name.items[0].data["name"][0]["family"], "Einstein");

    // Search by gender
    let params_gender = SearchParams::parse_query("gender=female", 20, 100);
    let res_gender = storage.search("Patient", &params_gender).await.unwrap();
    assert_eq!(res_gender.total, 1);
    assert_eq!(res_gender.items[0].data["name"][0]["family"], "Franklin");
}

#[tokio::test]
async fn test_sqlite_disk_persistence_across_reconnect() {
    let db_path = "test_persistence.db";
    let db_url = format!("sqlite://{db_path}");

    // Clean up any stale file from prior runs
    let _ = std::fs::remove_file(db_path);

    let patient_id = {
        // Scope 1: Open database, insert record, then close/drop connection pool
        let storage = SqlStorage::connect(&db_url).await.expect("Failed to open SQLite db");

        let patient = json!({
            "resourceType": "Patient",
            "name": [{ "family": "Newton", "given": ["Isaac"] }],
            "gender": "male"
        });

        let created = storage.create("Patient", patient).await.expect("Failed to create Patient");
        assert_eq!(created.version_id, "1");
        created.id
    };

    // Scope 2: Reconnect to the same database file and verify persistence
    {
        let storage2 = SqlStorage::connect(&db_url).await.expect("Failed to re-open SQLite db");

        let read = storage2
            .read("Patient", &patient_id)
            .await
            .expect("Failed to read persisted Patient after reconnect");

        assert_eq!(read.id, patient_id);
        assert_eq!(read.data["name"][0]["family"], "Newton");
        assert_eq!(read.version_id, "1");

        // Verify search index also persisted
        let params = SearchParams::parse_query("name=Newton", 20, 100);
        let search_res = storage2.search("Patient", &params).await.unwrap();
        assert_eq!(search_res.total, 1);
    }

    // Clean up test file
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn test_sqlite_transaction_bundle() {
    let storage = SqlStorage::connect("sqlite://:memory:")
        .await
        .expect("Failed to connect to SQLite");

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "request": { "method": "POST", "url": "Patient" },
                "resource": {
                    "resourceType": "Patient",
                    "id": "tx-sql-p1",
                    "name": [{ "family": "Turing", "given": ["Alan"] }]
                }
            },
            {
                "request": { "method": "POST", "url": "Observation" },
                "resource": {
                    "resourceType": "Observation",
                    "id": "tx-sql-o1",
                    "status": "final",
                    "code": { "coding": [{ "code": "1234-5" }] },
                    "subject": { "reference": "Patient/tx-sql-p1" }
                }
            }
        ]
    });

    let resp = storage.process_bundle(bundle).await.expect("Transaction failed");
    assert_eq!(resp["type"], "transaction-response");
    assert_eq!(resp["entry"].as_array().unwrap().len(), 2);

    // Verify records exist in SQLite
    let p = storage.read("Patient", "tx-sql-p1").await.unwrap();
    assert_eq!(p.data["name"][0]["family"], "Turing");

    let o = storage.read("Observation", "tx-sql-o1").await.unwrap();
    assert_eq!(o.data["status"], "final");
}
