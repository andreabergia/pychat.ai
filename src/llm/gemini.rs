use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::provider::{AssistantInput, AssistantOutput, LlmError, LlmProvider, LlmResult};

#[derive(Debug, Clone)]
pub struct GeminiProvider {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl GeminiProvider {
    pub fn new(
        client: Client,
        api_key: Option<String>,
        model: String,
        base_url: String,
    ) -> LlmResult<Self> {
        let api_key = api_key
            .filter(|v| !v.trim().is_empty())
            .ok_or(LlmError::MissingApiKey)?;

        Ok(Self {
            client,
            api_key,
            model,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/v1beta/models/{}:generateContent",
            self.base_url, self.model
        )
    }

    fn build_request(input: &AssistantInput) -> GeminiGenerateRequest {
        GeminiGenerateRequest {
            contents: vec![GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart {
                    text: input.user_message.clone(),
                }],
            }],
            system_instruction: input.system_instruction.as_ref().map(|text| GeminiSystemInstruction {
                parts: vec![GeminiPart { text: text.clone() }],
            }),
        }
    }

    fn extract_text(resp: GeminiGenerateResponse) -> LlmResult<String> {
        for candidate in resp.candidates {
            for part in candidate.content.parts {
                let text = part.text.trim();
                if !text.is_empty() {
                    return Ok(text.to_string());
                }
            }
        }

        Err(LlmError::EmptyResponse)
    }
}

impl LlmProvider for GeminiProvider {
    async fn generate(&self, input: AssistantInput) -> LlmResult<AssistantOutput> {
        let payload = Self::build_request(&input);
        let resp = self
            .client
            .post(self.endpoint())
            .query(&[("key", self.api_key.as_str())])
            .json(&payload)
            .send()
            .await
            .map_err(|err| LlmError::Transport(err.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read body>".to_string());
            let body = body.chars().take(400).collect::<String>();
            return Err(LlmError::HttpStatus { status, body });
        }

        let parsed = resp
            .json::<GeminiGenerateResponse>()
            .await
            .map_err(|err| LlmError::Parse(err.to_string()))?;
        let text = Self::extract_text(parsed)?;
        Ok(AssistantOutput { text })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerateRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
}

#[derive(Debug, Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerateResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiResponseContent,
}

#[derive(Debug, Deserialize)]
struct GeminiResponseContent {
    parts: Vec<GeminiPart>,
}

#[cfg(test)]
mod tests {
    use super::GeminiProvider;
    use crate::llm::provider::{AssistantInput, LlmError, LlmProvider};
    use reqwest::Client;
    use wiremock::matchers::{body_string_contains, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn generate_returns_first_text_part() {
        let server = MockServer::start().await;
        let body = r#"{
            "candidates": [
                {"content": {"parts": [{"text": ""}, {"text": "hello from gemini"}]}}
            ]
        }"#;

        Mock::given(method("POST"))
            .and(path("/v1beta/models/test-model:generateContent"))
            .and(query_param("key", "test-key"))
            .and(body_string_contains("systemInstruction"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .mount(&server)
            .await;

        let provider = GeminiProvider::new(
            Client::new(),
            Some("test-key".to_string()),
            "test-model".to_string(),
            server.uri(),
        )
        .expect("provider");

        let out = provider
            .generate(AssistantInput {
                user_message: "hello".to_string(),
                system_instruction: Some("system".to_string()),
            })
            .await
            .expect("success response");

        assert_eq!(out.text, "hello from gemini");
    }

    #[tokio::test]
    async fn generate_maps_http_error_status() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid key"))
            .mount(&server)
            .await;

        let provider = GeminiProvider::new(
            Client::new(),
            Some("bad-key".to_string()),
            "test-model".to_string(),
            server.uri(),
        )
        .expect("provider");

        let err = provider
            .generate(AssistantInput {
                user_message: "hello".to_string(),
                system_instruction: None,
            })
            .await
            .expect_err("expected auth error");

        match err {
            LlmError::HttpStatus { status, body } => {
                assert_eq!(status, 401);
                assert!(body.contains("invalid key"));
            }
            other => panic!("expected HttpStatus, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn generate_returns_empty_response_error_when_no_text() {
        let server = MockServer::start().await;
        let body = r#"{"candidates": [{"content": {"parts": [{"text": ""}]}}]}"#;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .mount(&server)
            .await;

        let provider = GeminiProvider::new(
            Client::new(),
            Some("test-key".to_string()),
            "test-model".to_string(),
            server.uri(),
        )
        .expect("provider");

        let err = provider
            .generate(AssistantInput {
                user_message: "hello".to_string(),
                system_instruction: None,
            })
            .await
            .expect_err("expected empty response error");

        assert_eq!(err, LlmError::EmptyResponse);
    }

    #[test]
    fn new_requires_api_key() {
        let err = GeminiProvider::new(
            Client::new(),
            None,
            "test-model".to_string(),
            "https://example.com".to_string(),
        )
        .expect_err("missing key should fail");

        assert_eq!(err, LlmError::MissingApiKey);
    }
}
