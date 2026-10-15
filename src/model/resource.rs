use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::File;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(1);

/// Generates a valid RFC 4122 v4 UUID string.
pub fn generate_uuid() -> String {
    let mut bytes = [0u8; 16];
    let mut filled = false;

    // Try reading from /dev/urandom on Unix systems
    if let Ok(mut f) = File::open("/dev/urandom") {
        if f.read_exact(&mut bytes).is_ok() {
            filled = true;
        }
    }

    if !filled {
        // Fallback using timestamp and atomic counter
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id() as u128;
        let mixed = now ^ (count as u128).rotate_left(32) ^ pid.rotate_left(64);
        bytes[..8].copy_from_slice(&(mixed as u64).to_be_bytes());
        bytes[8..].copy_from_slice(&((mixed >> 64) as u64).to_be_bytes());
    }

    // Set version to 4 (0100 in high 4 bits of byte 6)
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    // Set variant to RFC 4122 (10 in high 2 bits of byte 8)
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

/// Returns current UTC timestamp in ISO 8601 format: YYYY-MM-DDTHH:MM:SS.sssZ
pub fn now_iso8601() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let total_secs = now.as_secs();
    let millis = now.subsec_millis();

    let days = (total_secs / 86400) as i64;
    let rem_secs = (total_secs % 86400) as u32;

    let hours = rem_secs / 3600;
    let minutes = (rem_secs % 3600) / 60;
    let seconds = rem_secs % 60;

    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, hours, minutes, seconds, millis
    )
}

/// Converts days since 1970-01-01 to year, month, day (Gregorian calendar).
fn days_to_ymd(mut days: i64) -> (i32, u32, u32) {
    // 719468 days between 0000-03-01 and 1970-01-01
    days += 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = (days - era * 146097) as u32; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // year of era [0, 399]
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // month in year starting from March [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Validates FHIR resource ID according to specification: [A-Za-z0-9\-\.]{1,64}
pub fn is_valid_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 64 {
        return false;
    }
    id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

/// Validates FHIR Resource Type name.
pub fn is_valid_resource_type(res_type: &str) -> bool {
    if res_type.is_empty() || res_type.len() > 64 {
        return false;
    }
    let mut chars = res_type.chars();
    match chars.next() {
        Some(first) if first.is_ascii_uppercase() => chars.all(|c| c.is_ascii_alphanumeric()),
        _ => false,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceMeta {
    #[serde(rename = "versionId")]
    pub version_id: String,
    #[serde(rename = "lastUpdated")]
    pub last_updated: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredResource {
    pub resource_type: String,
    pub id: String,
    pub version_id: String,
    pub last_updated: String,
    pub deleted: bool,
    pub data: Value,
}

impl StoredResource {
    pub fn new(resource_type: String, id: String, version_id: String, mut data: Value) -> Self {
        let last_updated = now_iso8601();

        // Ensure resourceType and id match
        if let Value::Object(ref mut map) = data {
            map.insert("resourceType".to_string(), Value::String(resource_type.clone()));
            map.insert("id".to_string(), Value::String(id.clone()));

            // Update meta object
            let mut meta_obj = map
                .get("meta")
                .and_then(|m| m.as_object().cloned())
                .unwrap_or_default();

            meta_obj.insert("versionId".to_string(), Value::String(version_id.clone()));
            meta_obj.insert("lastUpdated".to_string(), Value::String(last_updated.clone()));
            map.insert("meta".to_string(), Value::Object(meta_obj));
        }

        Self {
            resource_type,
            id,
            version_id,
            last_updated,
            deleted: false,
            data,
        }
    }

    pub fn new_deleted(resource_type: String, id: String, version_id: String) -> Self {
        let last_updated = now_iso8601();
        Self {
            resource_type: resource_type.clone(),
            id: id.clone(),
            version_id: version_id.clone(),
            last_updated: last_updated.clone(),
            deleted: true,
            data: json!({
                "resourceType": resource_type,
                "id": id,
                "meta": {
                    "versionId": version_id,
                    "lastUpdated": last_updated
                }
            }),
        }
    }

    pub fn etag(&self) -> String {
        format!("W/\"{}\"", self.version_id)
    }

    pub fn to_json(&self) -> Value {
        self.data.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_generation() {
        let u1 = generate_uuid();
        let u2 = generate_uuid();
        assert_ne!(u1, u2);
        assert_eq!(u1.len(), 36);
        // Verify version 4 bit
        assert_eq!(&u1[14..15], "4");
        // Verify variant bit (8, 9, a, or b)
        let var_char = u1.chars().nth(19).unwrap();
        assert!(matches!(var_char, '8' | '9' | 'a' | 'b'));
    }

    #[test]
    fn test_iso8601_format() {
        let ts = now_iso8601();
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.chars().nth(4), Some('-'));
        assert_eq!(ts.chars().nth(7), Some('-'));
        assert_eq!(ts.chars().nth(10), Some('T'));
    }

    #[test]
    fn test_id_validation() {
        assert!(is_valid_id("patient-123"));
        assert!(is_valid_id("123.456"));
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("invalid/id"));
        assert!(!is_valid_id("invalid?id"));
    }

    #[test]
    fn test_resource_type_validation() {
        assert!(is_valid_resource_type("Patient"));
        assert!(is_valid_resource_type("Observation"));
        assert!(!is_valid_resource_type("patient")); // Must start uppercase
        assert!(!is_valid_resource_type("123"));
    }
}
