use crate::model::resource::StoredResource;
use crate::search::params::{DateFilter, DatePrefix, SearchParams};
use serde_json::Value;

pub struct SearchResult {
    pub total: usize,
    pub items: Vec<StoredResource>,
    pub offset: usize,
    pub limit: usize,
}

pub struct SearchEngine;

impl SearchEngine {
    pub fn evaluate(resources: &[StoredResource], params: &SearchParams) -> SearchResult {
        // Filter active (non-deleted) resources matching all parameters
        let mut matched: Vec<StoredResource> = resources
            .iter()
            .filter(|r| !r.deleted)
            .filter(|r| Self::matches_resource(r, params))
            .cloned()
            .collect();

        // Sort results
        if let Some(ref sort_field) = params.sort {
            Self::sort_resources(&mut matched, sort_field);
        } else {
            // Default sort: _lastUpdated descending
            matched.sort_by(|a, b| b.last_updated.cmp(&a.last_updated));
        }

        let total = matched.len();
        let offset = params.offset;
        let limit = params.count;

        let items = if offset >= total {
            Vec::new()
        } else {
            matched.into_iter().skip(offset).take(limit).collect()
        };

        SearchResult {
            total,
            items,
            offset,
            limit,
        }
    }

    fn matches_resource(resource: &StoredResource, params: &SearchParams) -> bool {
        // 1. _id filter
        if let Some(ref ids) = params.id {
            if !ids.iter().any(|id| id == &resource.id) {
                return false;
            }
        }

        // 2. _lastUpdated filter
        if let Some(ref date_filters) = params.last_updated {
            for filter in date_filters {
                if !Self::matches_date(&resource.last_updated, filter) {
                    return false;
                }
            }
        }

        // 3. Custom / Resource-specific parameters
        for (param_name, values) in &params.custom {
            if values.is_empty() {
                continue;
            }
            // An entry matches if AT LEAST ONE of the comma-separated values matches (OR condition)
            let matched_any = values.iter().any(|val| Self::matches_param(resource, param_name, val));
            if !matched_any {
                return false;
            }
        }

        true
    }

    fn matches_date(resource_date: &str, filter: &DateFilter) -> bool {
        let res = resource_date.trim();
        let target = filter.value.trim();

        // Compare standard ISO timestamps
        match filter.prefix {
            DatePrefix::Eq => res.starts_with(target) || res == target,
            DatePrefix::Ne => !res.starts_with(target) && res != target,
            DatePrefix::Gt | DatePrefix::Sa => res > target,
            DatePrefix::Lt | DatePrefix::Eb => res < target,
            DatePrefix::Ge => res >= target,
            DatePrefix::Le => res <= target,
        }
    }

    fn matches_param(resource: &StoredResource, param: &str, target: &str) -> bool {
        let data = &resource.data;

        match param {
            "name" => Self::matches_name(data, target),
            "family" => Self::matches_nested_str(data, &["name", "family"], target),
            "given" => Self::matches_nested_array_str(data, &["name", "given"], target),
            "gender" => Self::matches_exact_str(data, "gender", target),
            "birthdate" => Self::matches_date_field(data, "birthDate", target),
            "active" => Self::matches_bool_or_str(data, "active", target),
            "status" => Self::matches_exact_str(data, "status", target),
            "code" => Self::matches_code(data, target),
            "identifier" => Self::matches_identifier(data, target),
            "subject" | "patient" => Self::matches_reference(data, param, target),
            _ => {
                // Fallback: search key directly in top-level JSON or recursive search
                Self::matches_generic_json(data, param, target)
            }
        }
    }

    fn matches_name(data: &Value, target: &str) -> bool {
        let target_lower = target.to_lowercase();

        if let Some(names) = data.get("name").and_then(|n| n.as_array()) {
            for name in names {
                // Check text
                if let Some(text) = name.get("text").and_then(|t| t.as_str()) {
                    if text.to_lowercase().contains(&target_lower) {
                        return true;
                    }
                }
                // Check family
                if let Some(fam) = name.get("family").and_then(|f| f.as_str()) {
                    if fam.to_lowercase().contains(&target_lower) {
                        return true;
                    }
                }
                // Check given
                if let Some(givens) = name.get("given").and_then(|g| g.as_array()) {
                    for given in givens {
                        if let Some(g) = given.as_str() {
                            if g.to_lowercase().contains(&target_lower) {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    fn matches_nested_str(data: &Value, path: &[&str], target: &str) -> bool {
        let target_lower = target.to_lowercase();
        if path.len() == 2 && path[0] == "name" {
            if let Some(names) = data.get("name").and_then(|n| n.as_array()) {
                for name in names {
                    if let Some(val) = name.get(path[1]).and_then(|v| v.as_str()) {
                        if val.to_lowercase().contains(&target_lower) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn matches_nested_array_str(data: &Value, path: &[&str], target: &str) -> bool {
        let target_lower = target.to_lowercase();
        if path.len() == 2 && path[0] == "name" {
            if let Some(names) = data.get("name").and_then(|n| n.as_array()) {
                for name in names {
                    if let Some(arr) = name.get(path[1]).and_then(|v| v.as_array()) {
                        for item in arr {
                            if let Some(s) = item.as_str() {
                                if s.to_lowercase().contains(&target_lower) {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }

    fn matches_exact_str(data: &Value, field: &str, target: &str) -> bool {
        if let Some(val) = data.get(field).and_then(|v| v.as_str()) {
            val.eq_ignore_ascii_case(target)
        } else {
            false
        }
    }

    fn matches_bool_or_str(data: &Value, field: &str, target: &str) -> bool {
        if let Some(b) = data.get(field).and_then(|v| v.as_bool()) {
            return b.to_string().eq_ignore_ascii_case(target);
        }
        if let Some(s) = data.get(field).and_then(|v| v.as_str()) {
            return s.eq_ignore_ascii_case(target);
        }
        false
    }

    fn matches_date_field(data: &Value, field: &str, target: &str) -> bool {
        if let Some(val) = data.get(field).and_then(|v| v.as_str()) {
            let (prefix, date_val) = DatePrefix::parse(target);
            let filter = DateFilter { prefix, value: date_val.to_string() };
            Self::matches_date(val, &filter)
        } else {
            false
        }
    }

    fn matches_code(data: &Value, target: &str) -> bool {
        // target can be "code" or "system|code"
        let parts: Vec<&str> = target.split('|').collect();
        let (req_system, req_code) = if parts.len() == 2 {
            (Some(parts[0]), parts[1])
        } else {
            (None, target)
        };

        // Check code element (CodeableConcept or Coding)
        if let Some(code_obj) = data.get("code") {
            if let Some(codings) = code_obj.get("coding").and_then(|c| c.as_array()) {
                for c in codings {
                    let code = c.get("code").and_then(|v| v.as_str()).unwrap_or("");
                    let system = c.get("system").and_then(|v| v.as_str()).unwrap_or("");

                    let code_match = code.eq_ignore_ascii_case(req_code);
                    let system_match = match req_system {
                        Some(s) if !s.is_empty() => system.eq_ignore_ascii_case(s),
                        _ => true,
                    };

                    if code_match && system_match {
                        return true;
                    }
                }
            }
            if let Some(text) = code_obj.get("text").and_then(|t| t.as_str()) {
                if text.to_lowercase().contains(&req_code.to_lowercase()) {
                    return true;
                }
            }
        }
        false
    }

    fn matches_identifier(data: &Value, target: &str) -> bool {
        let parts: Vec<&str> = target.split('|').collect();
        let (req_system, req_value) = if parts.len() == 2 {
            (Some(parts[0]), parts[1])
        } else {
            (None, target)
        };

        if let Some(identifiers) = data.get("identifier").and_then(|i| i.as_array()) {
            for id in identifiers {
                let val = id.get("value").and_then(|v| v.as_str()).unwrap_or("");
                let system = id.get("system").and_then(|v| v.as_str()).unwrap_or("");

                let val_match = val.eq_ignore_ascii_case(req_value);
                let sys_match = match req_system {
                    Some(s) if !s.is_empty() => system.eq_ignore_ascii_case(s),
                    _ => true,
                };

                if val_match && sys_match {
                    return true;
                }
            }
        }
        false
    }

    fn matches_reference(data: &Value, field: &str, target: &str) -> bool {
        if let Some(ref_obj) = data.get(field) {
            if let Some(reference) = ref_obj.get("reference").and_then(|r| r.as_str()) {
                // target can be "Patient/123" or just "123"
                if reference.eq_ignore_ascii_case(target)
                    || reference.ends_with(&format!("/{target}"))
                {
                    return true;
                }
            }
        }
        false
    }

    fn matches_generic_json(data: &Value, key: &str, target: &str) -> bool {
        let target_lower = target.to_lowercase();
        if let Some(val) = data.get(key) {
            match val {
                Value::String(s) => s.to_lowercase().contains(&target_lower),
                Value::Bool(b) => b.to_string().eq_ignore_ascii_case(target),
                Value::Number(n) => n.to_string() == target,
                _ => false,
            }
        } else {
            false
        }
    }

    fn sort_resources(resources: &mut [StoredResource], sort_field: &str) {
        let (descending, field) = if sort_field.starts_with('-') {
            (true, &sort_field[1..])
        } else {
            (false, sort_field)
        };

        resources.sort_by(|a, b| {
            let order = match field {
                "_id" => a.id.cmp(&b.id),
                "_lastUpdated" => a.last_updated.cmp(&b.last_updated),
                _ => std::cmp::Ordering::Equal,
            };
            if descending {
                order.reverse()
            } else {
                order
            }
        });
    }
}
