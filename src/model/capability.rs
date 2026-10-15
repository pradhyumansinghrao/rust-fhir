use crate::model::resource::now_iso8601;
use serde_json::{json, Value};

/// Generates a complete FHIR R4 CapabilityStatement
pub fn make_capability_statement(base_url: &str) -> Value {
    let now = now_iso8601();

    let standard_search_params = vec![
        json!({
            "name": "_id",
            "type": "token",
            "documentation": "Logical id of the resource"
        }),
        json!({
            "name": "_lastUpdated",
            "type": "date",
            "documentation": "Only match resources which were last updated as specified by the given range"
        }),
        json!({
            "name": "_count",
            "type": "number",
            "documentation": "Number of results per page"
        }),
        json!({
            "name": "_sort",
            "type": "string",
            "documentation": "Order results by specified field"
        }),
    ];

    let standard_interactions = vec![
        json!({ "code": "read" }),
        json!({ "code": "vread" }),
        json!({ "code": "update" }),
        json!({ "code": "delete" }),
        json!({ "code": "history-instance" }),
        json!({ "code": "history-type" }),
        json!({ "code": "create" }),
        json!({ "code": "search-type" }),
    ];

    let resource_configs = vec![
        ("Patient", vec![
            json!({ "name": "identifier", "type": "token", "documentation": "A patient identifier" }),
            json!({ "name": "name", "type": "string", "documentation": "A server defined search that matches any part of the patient name" }),
            json!({ "name": "family", "type": "string", "documentation": "A portion of the family name of the patient" }),
            json!({ "name": "given", "type": "string", "documentation": "A portion of the given name of the patient" }),
            json!({ "name": "gender", "type": "token", "documentation": "Gender of the patient" }),
            json!({ "name": "birthdate", "type": "date", "documentation": "The patient's date of birth" }),
            json!({ "name": "active", "type": "token", "documentation": "Whether the patient record is active" }),
        ]),
        ("Observation", vec![
            json!({ "name": "code", "type": "token", "documentation": "The code of the observation type" }),
            json!({ "name": "status", "type": "token", "documentation": "The status of the observation" }),
            json!({ "name": "subject", "type": "reference", "documentation": "The subject that the observation is about" }),
            json!({ "name": "patient", "type": "reference", "documentation": "The subject that the observation is about (if patient)" }),
            json!({ "name": "date", "type": "date", "documentation": "Obtained date/time" }),
        ]),
        ("Encounter", vec![
            json!({ "name": "status", "type": "token", "documentation": "Status of the encounter" }),
            json!({ "name": "subject", "type": "reference", "documentation": "The patient present at the encounter" }),
            json!({ "name": "patient", "type": "reference", "documentation": "The patient present at the encounter" }),
            json!({ "name": "date", "type": "date", "documentation": "A date within the period the Encounter lasted" }),
        ]),
        ("Condition", vec![
            json!({ "name": "code", "type": "token", "documentation": "Code for the condition" }),
            json!({ "name": "clinical-status", "type": "token", "documentation": "The clinical status of the condition" }),
            json!({ "name": "subject", "type": "reference", "documentation": "Who has the condition" }),
            json!({ "name": "patient", "type": "reference", "documentation": "Who has the condition (if patient)" }),
        ]),
        ("DiagnosticReport", vec![
            json!({ "name": "code", "type": "token", "documentation": "The code for the report" }),
            json!({ "name": "status", "type": "token", "documentation": "The status of the diagnostic report" }),
            json!({ "name": "subject", "type": "reference", "documentation": "The subject of the report" }),
            json!({ "name": "patient", "type": "reference", "documentation": "The subject of the report (if patient)" }),
        ]),
        ("Practitioner", vec![
            json!({ "name": "identifier", "type": "token", "documentation": "A practitioner's identifier" }),
            json!({ "name": "name", "type": "string", "documentation": "A portion of either family or given name" }),
            json!({ "name": "family", "type": "string", "documentation": "A portion of the family name" }),
            json!({ "name": "given", "type": "string", "documentation": "A portion of the given name" }),
            json!({ "name": "active", "type": "token", "documentation": "Whether this practitioner's record is in active use" }),
        ]),
        ("Organization", vec![
            json!({ "name": "identifier", "type": "token", "documentation": "Any identifier for the organization" }),
            json!({ "name": "name", "type": "string", "documentation": "A portion of the organization's name" }),
            json!({ "name": "active", "type": "token", "documentation": "Whether the organization's record is active" }),
        ]),
    ];

    let resources: Vec<Value> = resource_configs
        .into_iter()
        .map(|(res_type, extra_params)| {
            let mut search_params = standard_search_params.clone();
            search_params.extend(extra_params);

            json!({
                "type": res_type,
                "profile": format!("http://hl7.org/fhir/StructureDefinition/{res_type}"),
                "interaction": standard_interactions.clone(),
                "versioning": "versioned",
                "readHistory": true,
                "updateCreate": true,
                "conditionalCreate": false,
                "conditionalRead": "not-supported",
                "conditionalUpdate": false,
                "conditionalDelete": "not-supported",
                "searchParam": search_params,
            })
        })
        .collect();

    json!({
        "resourceType": "CapabilityStatement",
        "id": "rust-fhir-capabilities",
        "status": "active",
        "date": now,
        "publisher": "Rust FHIR Server Project",
        "kind": "instance",
        "software": {
            "name": "rust-fhir-server",
            "version": "0.1.0"
        },
        "implementation": {
            "description": "High performance Rust HL7 FHIR R4 RESTful Server",
            "url": base_url
        },
        "fhirVersion": "4.0.1",
        "format": [
            "application/fhir+json",
            "application/json"
        ],
        "rest": [
            {
                "mode": "server",
                "resource": resources,
                "interaction": [
                    { "code": "transaction" },
                    { "code": "batch" },
                    { "code": "history-system" },
                    { "code": "search-system" }
                ]
            }
        ]
    })
}
