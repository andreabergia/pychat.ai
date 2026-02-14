use reqwest::Url;
#[cfg(test)]
use reqwest::header::HeaderMap;
use reqwest::header::HeaderValue;
use serde_json::Value;

const REDACTION: &str = "***REDACTED***";
const SENSITIVE_KEYS: [&str; 10] = [
    "key",
    "api_key",
    "apikey",
    "token",
    "access_token",
    "authorization",
    "secret",
    "password",
    "x-api-key",
    "x-goog-api-key",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpDebugConfig {
    pub enabled: bool,
    pub redact_secrets: bool,
    pub max_body_chars: usize,
}

impl HttpDebugConfig {
    pub fn from_verbose(verbose: bool) -> Self {
        Self {
            enabled: verbose,
            redact_secrets: true,
            max_body_chars: 4_000,
        }
    }

    #[cfg(test)]
    pub fn disabled() -> Self {
        Self::from_verbose(false)
    }
}

pub fn redact_url(url: &Url, enable_redaction: bool) -> String {
    if !enable_redaction {
        return url.as_str().to_string();
    }

    let mut redacted = url.clone();
    let pairs: Vec<(String, String)> = redacted
        .query_pairs()
        .map(|(k, v)| {
            if is_sensitive_key(k.as_ref()) {
                (k.into_owned(), REDACTION.to_string())
            } else {
                (k.into_owned(), v.into_owned())
            }
        })
        .collect();

    redacted.set_query(None);
    if !pairs.is_empty() {
        let mut qp = redacted.query_pairs_mut();
        for (k, v) in pairs {
            qp.append_pair(&k, &v);
        }
    }

    redacted.as_str().to_string()
}

#[cfg(test)]
pub fn redact_headers(headers: &HeaderMap, enable_redaction: bool) -> String {
    let mut items = Vec::new();

    for (name, value) in headers {
        let header_name = name.as_str();
        let shown_value = redact_header_value(header_name, value, enable_redaction);

        items.push(format!("{header_name}: {shown_value}"));
    }

    items.join(", ")
}

pub fn redact_header_value(name: &str, value: &HeaderValue, enable_redaction: bool) -> String {
    if enable_redaction && is_sensitive_key(name) {
        REDACTION.to_string()
    } else {
        value
            .to_str()
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|_| "<non-utf8>".to_string())
    }
}

pub fn redact_text_body(raw: &str, enable_redaction: bool) -> String {
    if !enable_redaction {
        return raw.to_string();
    }

    match serde_json::from_str::<Value>(raw) {
        Ok(mut json) => {
            redact_json_value(&mut json);
            serde_json::to_string(&json).unwrap_or_else(|_| raw.to_string())
        }
        Err(_) => raw.to_string(),
    }
}

pub fn truncate_for_log(input: &str, max_chars: usize) -> String {
    let count = input.chars().count();
    if count <= max_chars {
        return input.to_string();
    }

    let truncated = input.chars().take(max_chars).collect::<String>();
    format!("{truncated}... <truncated {} chars>", count - max_chars)
}

fn redact_json_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                if is_sensitive_key(key) {
                    *item = Value::String(REDACTION.to_string());
                } else {
                    redact_json_value(item);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_json_value(item);
            }
        }
        _ => {}
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    SENSITIVE_KEYS.iter().any(|s| *s == key)
}

#[cfg(test)]
mod tests {
    use super::{HttpDebugConfig, redact_headers, redact_text_body, redact_url, truncate_for_log};
    use reqwest::Url;
    use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};

    #[test]
    fn from_verbose_defaults_to_redaction() {
        let cfg = HttpDebugConfig::from_verbose(true);
        assert!(cfg.enabled);
        assert!(cfg.redact_secrets);
        assert_eq!(cfg.max_body_chars, 4_000);
    }

    #[test]
    fn redact_url_masks_sensitive_query_params() {
        let url = Url::parse("https://example.com/path?key=super-secret&view=full").expect("url");
        let redacted = redact_url(&url, true);
        assert!(
            redacted.contains("key=%2A%2A%2AREDACTED%2A%2A%2A")
                || redacted.contains("key=***REDACTED***")
        );
        assert!(redacted.contains("view=full"));
        assert!(!redacted.contains("super-secret"));
    }

    #[test]
    fn redact_headers_masks_authorization_values() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer secret"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let formatted = redact_headers(&headers, true);
        assert!(formatted.contains("authorization: ***REDACTED***"));
        assert!(formatted.contains("content-type: application/json"));
        assert!(!formatted.contains("Bearer secret"));
    }

    #[test]
    fn redact_text_body_masks_nested_json_keys() {
        let raw = r#"{"token":"secret","nested":{"api_key":"123"},"ok":true}"#;
        let redacted = redact_text_body(raw, true);
        assert!(redacted.contains("\"token\":\"***REDACTED***\""));
        assert!(redacted.contains("\"api_key\":\"***REDACTED***\""));
        assert!(redacted.contains("\"ok\":true"));
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("123"));
    }

    #[test]
    fn truncate_for_log_appends_marker() {
        let out = truncate_for_log("abcdefghijklmnopqrstuvwxyz", 5);
        assert!(out.starts_with("abcde"));
        assert!(out.contains("<truncated 21 chars>"));
    }
}
