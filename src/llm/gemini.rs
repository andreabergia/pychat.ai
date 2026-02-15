use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::provider::{
    AssistantCandidate, AssistantInput, AssistantMessage, AssistantOutput, AssistantPart,
    AssistantRole, FunctionDeclaration, LlmError, LlmProvider, LlmResult, ToolCallingMode,
};
use crate::http::client::HttpClient;

#[derive(Debug, Clone)]
pub struct GeminiProvider {
    client: HttpClient,
    api_key: String,
    model: String,
    base_url: String,
}

impl GeminiProvider {
    pub fn new(
        client: HttpClient,
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
            contents: input
                .messages
                .iter()
                .map(Self::to_content)
                .collect::<Vec<_>>(),
            system_instruction: input.system_instruction.as_ref().map(|text| {
                GeminiSystemInstruction {
                    parts: vec![GeminiPartRequest {
                        text: Some(text.clone()),
                        function_call: None,
                        function_response: None,
                    }],
                }
            }),
            tools: if input.tools.is_empty() {
                Vec::new()
            } else {
                vec![GeminiTool {
                    function_declarations: input
                        .tools
                        .iter()
                        .map(Self::to_function_declaration)
                        .collect(),
                }]
            },
            tool_config: Some(GeminiToolConfig {
                function_calling_config: GeminiFunctionCallingConfig {
                    mode: match input.tool_calling_mode {
                        ToolCallingMode::Auto => "AUTO".to_string(),
                    },
                },
            }),
        }
    }

    fn to_content(message: &AssistantMessage) -> GeminiContentRequest {
        GeminiContentRequest {
            role: match message.role {
                AssistantRole::User => "user".to_string(),
                AssistantRole::Model => "model".to_string(),
                AssistantRole::Tool => "tool".to_string(),
            },
            parts: message.parts.iter().map(Self::to_part).collect(),
        }
    }

    fn to_part(part: &AssistantPart) -> GeminiPartRequest {
        match part {
            AssistantPart::Text(text) => GeminiPartRequest {
                text: Some(text.clone()),
                function_call: None,
                function_response: None,
            },
            AssistantPart::FunctionCall {
                id,
                name,
                args_json,
            } => GeminiPartRequest {
                text: None,
                function_call: Some(GeminiFunctionCall {
                    id: id.clone(),
                    name: name.clone(),
                    args: args_json.clone(),
                }),
                function_response: None,
            },
            AssistantPart::FunctionResponse {
                id,
                name,
                response_json,
            } => GeminiPartRequest {
                text: None,
                function_call: None,
                function_response: Some(GeminiFunctionResponse {
                    id: id.clone(),
                    name: name.clone(),
                    response: response_json.clone(),
                }),
            },
        }
    }

    fn to_function_declaration(decl: &FunctionDeclaration) -> GeminiFunctionDeclaration {
        GeminiFunctionDeclaration {
            name: decl.name.clone(),
            description: decl.description.clone(),
            parameters: decl.parameters_json_schema.clone(),
        }
    }

    fn extract_output(resp: GeminiGenerateResponse) -> LlmResult<AssistantOutput> {
        let candidates = resp
            .candidates
            .into_iter()
            .map(|candidate| {
                let finish_reason = candidate.finish_reason;
                AssistantCandidate {
                    message: AssistantMessage {
                        role: AssistantRole::Model,
                        parts: candidate
                            .content
                            .parts
                            .into_iter()
                            .filter_map(Self::to_output_part)
                            .collect(),
                    },
                    safety_blocked: matches!(finish_reason.as_deref(), Some("SAFETY")),
                    finish_reason,
                }
            })
            .collect::<Vec<_>>();

        if candidates.is_empty() {
            return Err(LlmError::EmptyCandidates);
        }

        Ok(AssistantOutput { candidates })
    }

    fn to_output_part(part: GeminiPartResponse) -> Option<AssistantPart> {
        if let Some(function_call) = part.function_call {
            return Some(AssistantPart::FunctionCall {
                id: function_call.id,
                name: function_call.name,
                args_json: function_call.args,
            });
        }

        if let Some(function_response) = part.function_response {
            return Some(AssistantPart::FunctionResponse {
                id: function_response.id,
                name: function_response.name,
                response_json: function_response.response,
            });
        }

        part.text.map(AssistantPart::Text)
    }
}

impl LlmProvider for GeminiProvider {
    async fn generate(&self, input: AssistantInput) -> LlmResult<AssistantOutput> {
        let payload = Self::build_request(&input);
        let resp = self
            .client
            .post_json(
                self.endpoint().as_str(),
                &[("key", self.api_key.as_str())],
                &payload,
            )
            .await
            .map_err(|err| LlmError::Transport(err.to_string()))?;

        if !(200..300).contains(&resp.status) {
            let status = resp.status;
            let body = resp.body;
            let body = body.chars().take(400).collect::<String>();
            return Err(LlmError::HttpStatus { status, body });
        }

        let parsed = serde_json::from_str::<GeminiGenerateResponse>(&resp.body)
            .map_err(|err| LlmError::Parse(err.to_string()))?;
        Self::extract_output(parsed)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerateRequest {
    contents: Vec<GeminiContentRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<GeminiTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_config: Option<GeminiToolConfig>,
}

#[derive(Debug, Serialize)]
struct GeminiContentRequest {
    role: String,
    parts: Vec<GeminiPartRequest>,
}

#[derive(Debug, Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPartRequest>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiPartRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    function_call: Option<GeminiFunctionCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    function_response: Option<GeminiFunctionResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiFunctionCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    name: String,
    #[serde(default)]
    args: Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiFunctionResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    name: String,
    response: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiTool {
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Serialize)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiToolConfig {
    function_calling_config: GeminiFunctionCallingConfig,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiFunctionCallingConfig {
    mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerateResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiCandidate {
    content: GeminiResponseContent,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponseContent {
    parts: Vec<GeminiPartResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiPartResponse {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    function_call: Option<GeminiFunctionCall>,
    #[serde(default)]
    function_response: Option<GeminiFunctionResponse>,
}

#[cfg(test)]
mod tests {
    use super::GeminiProvider;
    use crate::http::{client::HttpClient, debug::HttpDebugConfig};
    use crate::llm::provider::{
        AssistantInput, AssistantMessage, AssistantPart, AssistantRole, FunctionDeclaration,
        LlmError, LlmProvider, ToolCallingMode,
    };
    use serde_json::json;
    use wiremock::matchers::{body_string_contains, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn basic_input() -> AssistantInput {
        AssistantInput {
            system_instruction: Some("system".to_string()),
            messages: vec![AssistantMessage {
                role: AssistantRole::User,
                parts: vec![AssistantPart::Text("hello".to_string())],
            }],
            tools: vec![FunctionDeclaration {
                name: "list_globals".to_string(),
                description: "List global names in scope".to_string(),
                parameters_json_schema: json!({"type":"object","properties":{}}),
            }],
            tool_calling_mode: ToolCallingMode::Auto,
        }
    }

    #[tokio::test]
    async fn generate_parses_function_call_candidate() {
        let server = MockServer::start().await;
        let body = r#"{
            "candidates": [
                {"finishReason":"STOP","content":{"parts":[{"functionCall":{"id":"c1","name":"list_globals","args":{}}}]}}
            ]
        }"#;

        Mock::given(method("POST"))
            .and(path("/v1beta/models/test-model:generateContent"))
            .and(query_param("key", "test-key"))
            .and(body_string_contains("functionDeclarations"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .mount(&server)
            .await;

        let provider = GeminiProvider::new(
            HttpClient::new(reqwest::Client::new(), HttpDebugConfig::disabled()),
            Some("test-key".to_string()),
            "test-model".to_string(),
            server.uri(),
        )
        .expect("provider");

        let out = provider
            .generate(basic_input())
            .await
            .expect("success response");
        assert_eq!(out.candidates.len(), 1);
        assert_eq!(out.candidates[0].finish_reason.as_deref(), Some("STOP"));
        assert!(matches!(
            out.candidates[0].message.parts.first(),
            Some(AssistantPart::FunctionCall { name, .. }) if name == "list_globals"
        ));
    }

    #[tokio::test]
    async fn generate_maps_http_error_status() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid key"))
            .mount(&server)
            .await;

        let provider = GeminiProvider::new(
            HttpClient::new(reqwest::Client::new(), HttpDebugConfig::disabled()),
            Some("bad-key".to_string()),
            "test-model".to_string(),
            server.uri(),
        )
        .expect("provider");

        let err = provider
            .generate(basic_input())
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
    async fn generate_returns_empty_candidates_error_when_missing() {
        let server = MockServer::start().await;
        let body = r#"{"candidates": []}"#;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .mount(&server)
            .await;

        let provider = GeminiProvider::new(
            HttpClient::new(reqwest::Client::new(), HttpDebugConfig::disabled()),
            Some("test-key".to_string()),
            "test-model".to_string(),
            server.uri(),
        )
        .expect("provider");

        let err = provider
            .generate(basic_input())
            .await
            .expect_err("expected empty candidates error");

        assert_eq!(err, LlmError::EmptyCandidates);
    }

    #[test]
    fn new_requires_api_key() {
        let err = GeminiProvider::new(
            HttpClient::new(reqwest::Client::new(), HttpDebugConfig::disabled()),
            None,
            "test-model".to_string(),
            "https://example.com".to_string(),
        )
        .expect_err("missing key should fail");

        assert_eq!(err, LlmError::MissingApiKey);
    }
}
