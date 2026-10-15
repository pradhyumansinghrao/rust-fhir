use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationOutcome {
    #[serde(rename = "resourceType")]
    pub resource_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub issue: Vec<Issue>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    pub severity: IssueSeverity,
    pub code: IssueCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueSeverity {
    Fatal,
    Error,
    Warning,
    Information,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IssueCode {
    Invalid,
    NotFound,
    Conflict,
    Deleted,
    Processing,
    Required,
    Value,
    NotSupported,
    Informational,
    BusinessRule,
}

impl OperationOutcome {
    pub fn new(severity: IssueSeverity, code: IssueCode, diagnostics: &str) -> Self {
        Self {
            resource_type: "OperationOutcome".to_string(),
            id: None,
            issue: vec![Issue {
                severity,
                code,
                diagnostics: Some(diagnostics.to_string()),
                expression: None,
            }],
        }
    }

    pub fn not_found(diagnostics: &str) -> Self {
        Self::new(IssueSeverity::Error, IssueCode::NotFound, diagnostics)
    }

    pub fn deleted(diagnostics: &str) -> Self {
        Self::new(IssueSeverity::Error, IssueCode::Deleted, diagnostics)
    }

    pub fn bad_request(diagnostics: &str) -> Self {
        Self::new(IssueSeverity::Error, IssueCode::Invalid, diagnostics)
    }

    pub fn precondition_failed(diagnostics: &str) -> Self {
        Self::new(IssueSeverity::Error, IssueCode::Conflict, diagnostics)
    }

    pub fn conflict(diagnostics: &str) -> Self {
        Self::new(IssueSeverity::Error, IssueCode::Conflict, diagnostics)
    }

    pub fn internal_error(diagnostics: &str) -> Self {
        Self::new(IssueSeverity::Fatal, IssueCode::Processing, diagnostics)
    }

    pub fn success(diagnostics: &str) -> Self {
        Self::new(IssueSeverity::Information, IssueCode::Informational, diagnostics)
    }

    pub fn to_value(&self) -> Value {
        json!(self)
    }
}
