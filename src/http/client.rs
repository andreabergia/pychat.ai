use crate::trace::SessionTrace;
use reqwest::Client;
use serde::Serialize;
use std::fmt;

#[derive(Clone)]
pub struct HttpClient {
    inner: Client,
    trace: Option<SessionTrace>,
}

impl fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpClient")
            .field("trace_enabled", &self.trace.is_some())
            .finish()
    }
}

impl HttpClient {
    pub fn new(inner: Client) -> Self {
        Self { inner, trace: None }
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

        if let Some(trace) = &self.trace {
            trace.log_http_response(status, &headers, &body);
        }

        Ok(HttpResponseData { status, body })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponseData {
    pub status: u16,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::HttpClient;
    use crate::trace::SessionTrace;
    use reqwest::Client;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn post_json_returns_response_data() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true})))
            .mount(&server)
            .await;

        let client = HttpClient::new(Client::new());
        let response = client
            .post_json(
                &format!("{}/v1/test", server.uri()),
                &[],
                &json!({"ok":true}),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status, 200);
        assert_eq!(response.body, "{\"ok\":true}");
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
        let trace_file = trace.file_path().to_path_buf();

        let client = HttpClient::new(Client::new()).with_trace(trace.clone());

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
}
