use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bundle {
    #[serde(rename = "resourceType")]
    pub resource_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub bundle_type: BundleType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<Vec<BundleLink>>,
    #[serde(default)]
    pub entry: Vec<BundleEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum BundleType {
    Document,
    Message,
    Transaction,
    TransactionResponse,
    Batch,
    BatchResponse,
    History,
    Searchset,
    Collection,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BundleLink {
    pub relation: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BundleEntry {
    #[serde(rename = "fullUrl", skip_serializing_if = "Option::is_none")]
    pub full_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<BundleSearch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<BundleRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<BundleResponse>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BundleSearch {
    pub mode: String, // "match" | "include" | "outcome"
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BundleRequest {
    pub method: String, // "GET" | "POST" | "PUT" | "DELETE" | "HEAD"
    pub url: String,
    #[serde(rename = "ifNoneMatch", skip_serializing_if = "Option::is_none")]
    pub if_none_match: Option<String>,
    #[serde(rename = "ifMatch", skip_serializing_if = "Option::is_none")]
    pub if_match: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BundleResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(rename = "lastModified", skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Value>,
}

impl Bundle {
    pub fn new(bundle_type: BundleType, id: Option<String>) -> Self {
        Self {
            resource_type: "Bundle".to_string(),
            id,
            bundle_type,
            total: None,
            link: None,
            entry: Vec::new(),
        }
    }

    pub fn new_searchset(total: usize, entries: Vec<BundleEntry>, links: Vec<BundleLink>) -> Self {
        Self {
            resource_type: "Bundle".to_string(),
            id: None,
            bundle_type: BundleType::Searchset,
            total: Some(total),
            link: if links.is_empty() { None } else { Some(links) },
            entry: entries,
        }
    }

    pub fn new_history(entries: Vec<BundleEntry>) -> Self {
        Self {
            resource_type: "Bundle".to_string(),
            id: None,
            bundle_type: BundleType::History,
            total: Some(entries.len()),
            link: None,
            entry: entries,
        }
    }

    pub fn to_value(&self) -> Value {
        json!(self)
    }
}
