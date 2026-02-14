use super::debug::{
    HttpDebugConfig, redact_header_value, redact_text_body, redact_url, truncate_for_log,
};
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
        }
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

        let response = self.inner.execute(request).await?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.text().await?;

        self.log_response(status, &headers, &body);

        Ok(HttpResponseData { status, body })
    }

    fn log_request(&self, request: &reqwest::Request, body_json: &str) {
        if !self.debug.enabled {
            return;
        }

        let url = redact_url(request.url(), self.debug.redact_secrets);
        let body = redact_text_body(body_json, self.debug.redact_secrets);
        let body = truncate_for_log(&body, self.debug.max_body_chars);

        self.log_line(format!("[http-debug] > {} {}", request.method(), url));
        for (name, value) in request.headers() {
            self.log_line(format!(
                "[http-debug] > {}: {}",
                name.as_str(),
                redact_header_value(name.as_str(), value, self.debug.redact_secrets)
            ));
        }
        self.log_line("[http-debug] >".to_string());
        self.log_body('>', &body);
    }

    fn log_response(&self, status: u16, headers: &reqwest::header::HeaderMap, body: &str) {
        if !self.debug.enabled {
            return;
        }

        let body = redact_text_body(body, self.debug.redact_secrets);
        let body = truncate_for_log(&body, self.debug.max_body_chars);

        self.log_line(format!("[http-debug] < HTTP {status}"));
        for (name, value) in headers {
            self.log_line(format!(
                "[http-debug] < {}: {}",
                name.as_str(),
                redact_header_value(name.as_str(), value, self.debug.redact_secrets)
            ));
        }
        self.log_line("[http-debug] <".to_string());
        self.log_body('<', &body);
    }

    fn log_body(&self, direction: char, body: &str) {
        if body.is_empty() {
            self.log_line(format!("[http-debug] {direction} <empty body>"));
            return;
        }

        for line in body.lines() {
            self.log_line(format!("[http-debug] {direction} {line}"));
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
        };
        (client, buffer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponseData {
    pub status: u16,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::{HttpClient, HttpResponseData};
    use crate::http::debug::HttpDebugConfig;
    use reqwest::Client;
    use serde_json::json;
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
}
