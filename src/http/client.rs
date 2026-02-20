use super::debug::{
    HttpDebugConfig, redact_header_value, redact_text_body, redact_url, truncate_for_log,
};
use crate::trace::SessionTrace;
use reqwest::Client;
use serde::Serialize;
use std::fmt;
use std::io::{self, Write};
#[cfg(test)]
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct HttpClient {
    inner: Client,
    debug: HttpDebugConfig,
    sink: LogSink,
    trace: Option<SessionTrace>,
}

#[derive(Clone)]
enum LogSink {
    Stderr,
    #[cfg(test)]
    Buffer(Arc<Mutex<Vec<String>>>),
}

impl fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpClient")
            .field("debug", &self.debug)
            .finish()
    }
}

impl HttpClient {
    pub fn new(inner: Client, debug: HttpDebugConfig) -> Self {
        Self {
            inner,
            debug,
            sink: LogSink::Stderr,
            trace: None,
        }
    }

    pub fn with_trace(mut self, trace: SessionTrace) -> Self {
        self.trace = Some(trace);
        self
    }

    pub async fn post_json<T: Serialize + ?Sized>(
        &self,
        url: &str,
        query: &[(&str, &str)],
        payload: &T,
    ) -> Result<HttpResponseData, reqwest::Error> {
        let body_json = serde_json::to_string(payload)
            .unwrap_or_else(|err| format!("{{\"_serialization_error\":\"{err}\"}}"));

        let request = self.inner.post(url).query(query).json(payload).build()?;
        self.log_request(&request, &body_json);
        if let Some(trace) = &self.trace {
            trace.log_http_request(
                request.method().as_str(),
                request.url().as_str(),
                request.headers(),
                &body_json,
            );
        }

        let response = match self.inner.execute(request).await {
            Ok(response) => response,
            Err(err) => {
                if let Some(trace) = &self.trace {
                    trace.log_http_error(&err.to_string());
                }
                return Err(err);
            }
        };
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.text().await?;

        self.log_response(status, &headers, &body);
        if let Some(trace) = &self.trace {
            trace.log_http_response(status, &headers, &body);
        }

        Ok(HttpResponseData { status, body })
    }

    fn log_request(&self, request: &reqwest::Request, body_json: &str) {
        if !self.debug.enabled {
            return;
        }

        for line in request_log_lines(self.debug, request, body_json) {
            self.log_line(line);
        }
    }

    fn log_response(&self, status: u16, headers: &reqwest::header::HeaderMap, body: &str) {
        if !self.debug.enabled {
            return;
        }

        for line in response_log_lines(self.debug, status, headers, body) {
            self.log_line(line);
        }
    }

    fn log_line(&self, line: String) {
        match &self.sink {
            LogSink::Stderr => {
                let mut stderr = io::stderr().lock();
                let _ = writeln!(stderr, "{line}");
            }
            #[cfg(test)]
            LogSink::Buffer(buffer) => {
                if let Ok(mut b) = buffer.lock() {
                    b.push(line);
                }
            }
        }
    }

    #[cfg(test)]
    pub fn with_buffer_sink(
        inner: Client,
        debug: HttpDebugConfig,
    ) -> (Self, Arc<Mutex<Vec<String>>>) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let client = Self {
            inner,
            debug,
            sink: LogSink::Buffer(Arc::clone(&buffer)),
            trace: None,
        };
        (client, buffer)
    }
}

fn request_log_lines(
    debug: HttpDebugConfig,
    request: &reqwest::Request,
    body_json: &str,
) -> Vec<String> {
    let url = redact_url(request.url(), debug.redact_secrets);
    let body = redact_text_body(body_json, debug.redact_secrets);
    let body = truncate_for_log(&body, debug.max_body_chars);

    let mut lines = Vec::new();
    lines.push(format!("[http-debug] > {} {}", request.method(), url));
    for (name, value) in request.headers() {
        lines.push(format!(
            "[http-debug] > {}: {}",
            name.as_str(),
            redact_header_value(name.as_str(), value, debug.redact_secrets)
        ));
    }
    lines.push("[http-debug] >".to_string());
    append_body_lines(&mut lines, '>', &body);
    lines
}

fn response_log_lines(
    debug: HttpDebugConfig,
    status: u16,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> Vec<String> {
    let body = redact_text_body(body, debug.redact_secrets);
    let body = truncate_for_log(&body, debug.max_body_chars);

    let mut lines = Vec::new();
    lines.push(format!("[http-debug] < HTTP {status}"));
    for (name, value) in headers {
        lines.push(format!(
            "[http-debug] < {}: {}",
            name.as_str(),
            redact_header_value(name.as_str(), value, debug.redact_secrets)
        ));
    }
    lines.push("[http-debug] <".to_string());
    append_body_lines(&mut lines, '<', &body);
    lines
}

fn append_body_lines(lines: &mut Vec<String>, direction: char, body: &str) {
    if body.is_empty() {
        lines.push(format!("[http-debug] {direction} <empty body>"));
        return;
    }

    for line in body.lines() {
        lines.push(format!("[http-debug] {direction} {line}"));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponseData {
    pub status: u16,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::{HttpClient, HttpResponseData, request_log_lines, response_log_lines};
    use crate::http::debug::HttpDebugConfig;
    use crate::trace::SessionTrace;
    use reqwest::Client;
    use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
    use reqwest::{Method, Url};
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn post_json_logs_redacted_request_and_response_when_enabled() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/test"))
            .and(query_param("key", "super-secret"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_json(json!({"api_key":"response-secret","ok":true})),
            )
            .mount(&server)
            .await;

        let (client, logs) = HttpClient::with_buffer_sink(
            Client::new(),
            HttpDebugConfig {
                enabled: true,
                redact_secrets: true,
                max_body_chars: 4_000,
            },
        );

        let response = client
            .post_json(
                &format!("{}/v1/test", server.uri()),
                &[("key", "super-secret")],
                &json!({"token":"request-secret"}),
            )
            .await
            .expect("request should succeed");

        assert_eq!(
            response,
            HttpResponseData {
                status: 200,
                body: "{\"api_key\":\"response-secret\",\"ok\":true}".to_string(),
            }
        );

        let logged = logs.lock().expect("logs lock").join("\n");
        assert!(logged.contains("[http-debug] > POST"));
        assert!(logged.contains("[http-debug] < HTTP 200"));
        assert!(logged.contains("***REDACTED***"));
        assert!(!logged.contains("super-secret"));
        assert!(!logged.contains("request-secret"));
        assert!(!logged.contains("response-secret"));
    }

    #[tokio::test]
    async fn post_json_emits_no_logs_when_disabled() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
            .mount(&server)
            .await;

        let (client, logs) =
            HttpClient::with_buffer_sink(Client::new(), HttpDebugConfig::disabled());

        let _ = client
            .post_json(
                &format!("{}/v1/test", server.uri()),
                &[],
                &json!({"ok":true}),
            )
            .await
            .expect("request should succeed");

        assert!(logs.lock().expect("logs lock").is_empty());
    }

    #[tokio::test]
    async fn post_json_writes_full_raw_http_trace_when_trace_enabled() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/test"))
            .and(query_param("key", "super-secret"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-api-key", "response-secret")
                    .set_body_json(json!({"api_key":"response-secret","ok":true})),
            )
            .mount(&server)
            .await;

        let dir = tempdir().expect("tempdir");
        let trace = SessionTrace::create_in_temp_dir("test-session", dir.path()).expect("trace");
        let trace_file = dir.path().join(trace.file_name());

        let client =
            HttpClient::new(Client::new(), HttpDebugConfig::disabled()).with_trace(trace.clone());

        let response = client
            .post_json(
                &format!("{}/v1/test", server.uri()),
                &[("key", "super-secret")],
                &json!({"token":"request-secret"}),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status, 200);
        let trace_text = fs::read_to_string(trace_file).expect("read trace file");

        assert!(trace_text.contains("key=super-secret"));
        assert!(trace_text.contains("\"token\":\"request-secret\""));
        assert!(trace_text.contains("x-api-key: response-secret"));
        assert!(trace_text.contains("\"api_key\":\"response-secret\""));
    }

    #[test]
    fn request_log_lines_match_snapshot() {
        let debug = HttpDebugConfig {
            enabled: true,
            redact_secrets: true,
            max_body_chars: 4_000,
        };
        let mut request = reqwest::Request::new(
            Method::POST,
            Url::parse("https://example.com/v1/test?key=secret&view=full").expect("valid url"),
        );
        request.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer secret-token"),
        );
        request
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let lines = request_log_lines(debug, &request, r#"{"token":"abc","message":"hello"}"#);
        insta::assert_snapshot!("http_request_verbose", lines.join("\n"));
    }

    #[test]
    fn response_log_lines_match_snapshot() {
        let debug = HttpDebugConfig {
            enabled: true,
            redact_secrets: true,
            max_body_chars: 4_000,
        };
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("x-api-key", HeaderValue::from_static("response-secret"));

        let lines = response_log_lines(
            debug,
            401,
            &headers,
            "{\"error\":\"invalid\",\"api_key\":\"response-secret\"}\n{\"hint\":\"retry\"}",
        );
        insta::assert_snapshot!("http_response_verbose", lines.join("\n"));
    }

    #[test]
    fn response_log_lines_truncated_body_snapshot() {
        let debug = HttpDebugConfig {
            enabled: true,
            redact_secrets: true,
            max_body_chars: 24,
        };
        let headers = HeaderMap::new();
        let lines = response_log_lines(
            debug,
            200,
            &headers,
            "{\"message\":\"abcdefghijklmnopqrstuvwxyz\"}",
        );
        insta::assert_snapshot!("http_response_verbose_truncated", lines.join("\n"));
    }
}
