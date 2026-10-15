use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum DatePrefix {
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
    Sa, // Starts after
    Eb, // Ends before
}

impl DatePrefix {
    pub fn parse(s: &str) -> (Self, &str) {
        if s.len() >= 2 {
            match &s[..2] {
                "eq" => (DatePrefix::Eq, &s[2..]),
                "ne" => (DatePrefix::Ne, &s[2..]),
                "gt" => (DatePrefix::Gt, &s[2..]),
                "lt" => (DatePrefix::Lt, &s[2..]),
                "ge" => (DatePrefix::Ge, &s[2..]),
                "le" => (DatePrefix::Le, &s[2..]),
                "sa" => (DatePrefix::Sa, &s[2..]),
                "eb" => (DatePrefix::Eb, &s[2..]),
                _ => (DatePrefix::Eq, s),
            }
        } else {
            (DatePrefix::Eq, s)
        }
    }
}

#[derive(Clone, Debug)]
pub struct DateFilter {
    pub prefix: DatePrefix,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct SearchParams {
    pub id: Option<Vec<String>>,
    pub last_updated: Option<Vec<DateFilter>>,
    pub count: usize,
    pub offset: usize,
    pub sort: Option<String>,
    pub custom: HashMap<String, Vec<String>>,
}

impl SearchParams {
    pub fn parse_query(query_str: &str, default_count: usize, max_count: usize) -> Self {
        let mut id = None;
        let mut last_updated = Vec::new();
        let mut count = default_count;
        let mut offset = 0;
        let mut sort = None;
        let mut custom: HashMap<String, Vec<String>> = HashMap::new();

        for pair in query_str.split('&') {
            if pair.is_empty() {
                continue;
            }
            let mut parts = pair.splitn(2, '=');
            let key = parts.next().unwrap_or("").trim();
            let raw_val = parts.next().unwrap_or("").trim();
            if key.is_empty() {
                continue;
            }

            // URL decode basic characters
            let val = percent_decode(raw_val);

            match key {
                "_id" => {
                    let ids: Vec<String> = val.split(',').map(|s| s.trim().to_string()).collect();
                    id = Some(ids);
                }
                "_lastUpdated" => {
                    for v in val.split(',') {
                        let (prefix, date_val) = DatePrefix::parse(v.trim());
                        last_updated.push(DateFilter {
                            prefix,
                            value: date_val.to_string(),
                        });
                    }
                }
                "_count" => {
                    if let Ok(c) = val.parse::<usize>() {
                        count = c.min(max_count);
                    }
                }
                "_offset" | "_page" => {
                    if let Ok(o) = val.parse::<usize>() {
                        offset = o;
                    }
                }
                "_sort" => {
                    sort = Some(val);
                }
                _ => {
                    // Resource-specific search parameters
                    let values: Vec<String> = val.split(',').map(|s| s.trim().to_string()).collect();
                    custom.entry(key.to_string()).or_default().extend(values);
                }
            }
        }

        Self {
            id,
            last_updated: if last_updated.is_empty() { None } else { Some(last_updated) },
            count,
            offset,
            sort,
            custom,
        }
    }
}

fn percent_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut bytes = input.bytes();

    while let Some(b) = bytes.next() {
        if b == b'%' {
            if let (Some(h1), Some(h2)) = (bytes.next(), bytes.next()) {
                if let (Some(v1), Some(v2)) = (hex_val(h1), hex_val(h2)) {
                    result.push((v1 << 4 | v2) as char);
                    continue;
                }
            }
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }

    result
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_prefix_parsing() {
        let (prefix, val) = DatePrefix::parse("gt2026-01-01");
        assert_eq!(prefix, DatePrefix::Gt);
        assert_eq!(val, "2026-01-01");

        let (prefix_eq, val_eq) = DatePrefix::parse("2026-01-01");
        assert_eq!(prefix_eq, DatePrefix::Eq);
        assert_eq!(val_eq, "2026-01-01");
    }

    #[test]
    fn test_search_params_parsing() {
        let query = "_id=123,456&_count=15&_offset=30&gender=female&name=Jane+Doe";
        let params = SearchParams::parse_query(query, 20, 100);

        assert_eq!(params.id, Some(vec!["123".to_string(), "456".to_string()]));
        assert_eq!(params.count, 15);
        assert_eq!(params.offset, 30);
        assert_eq!(params.custom.get("gender"), Some(&vec!["female".to_string()]));
        assert_eq!(params.custom.get("name"), Some(&vec!["Jane Doe".to_string()]));
    }
}
