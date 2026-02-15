use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::time::timeout;

use crate::agent::dispatch::{FunctionCallSpec, dispatch_calls, tool_declarations};
use crate::agent::prompt::AGENT_SYSTEM_PROMPT;
use crate::llm::provider::{
    AssistantCandidate, AssistantInput, AssistantMessage, AssistantPart, AssistantRole,
    LlmProvider, ToolCallingMode,
};
use crate::python::CapabilityProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentConfig {
    pub max_steps: usize,
    pub per_step_timeout_ms: u64,
    pub total_timeout_ms: u64,
    pub invalid_response_retries: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_steps: 6,
            per_step_timeout_ms: 8_000,
            total_timeout_ms: 20_000,
            invalid_response_retries: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAnswer {
    pub text: String,
    pub degraded: bool,
}

pub async fn run_question<P: LlmProvider, C: CapabilityProvider>(
    provider: &P,
    capabilities: &C,
    question: &str,
    config: &AgentConfig,
) -> Result<AgentAnswer> {
    let mut messages = vec![AssistantMessage {
        role: AssistantRole::User,
        parts: vec![AssistantPart::Text(question.to_string())],
    }];
    let tools = tool_declarations();
    let total_deadline = Instant::now() + Duration::from_millis(config.total_timeout_ms);
    let mut invalid_response_attempts = 0usize;

    for _ in 0..config.max_steps {
        let now = Instant::now();
        if now >= total_deadline {
            return Ok(degraded(
                "Assistant hit the total time limit while reasoning about your question.",
            ));
        }

        let remaining = total_deadline.duration_since(now);
        let per_step = Duration::from_millis(config.per_step_timeout_ms);
        let timeout_budget = per_step.min(remaining);

        let llm = timeout(
            timeout_budget,
            provider.generate(AssistantInput {
                system_instruction: Some(AGENT_SYSTEM_PROMPT.to_string()),
                messages: messages.clone(),
                tools: tools.clone(),
                tool_calling_mode: ToolCallingMode::Auto,
            }),
        )
        .await;

        let output = match llm {
            Ok(Ok(output)) => output,
            Ok(Err(err)) => {
                return Ok(degraded(format!(
                    "Assistant request failed while reasoning: {err}"
                )));
            }
            Err(_) => {
                return Ok(degraded(
                    "Assistant hit a per-step timeout while reasoning about your question.",
                ));
            }
        };

        let Some(candidate) = select_candidate(&output.candidates) else {
            if invalid_response_attempts >= config.invalid_response_retries {
                return Ok(degraded(
                    "Assistant returned an invalid response repeatedly and could not complete the tool flow.",
                ));
            }
            invalid_response_attempts += 1;
            messages.push(repair_prompt_message());
            continue;
        };

        messages.push(candidate.message.clone());

        let calls = extract_function_calls(&candidate.message.parts);
        if calls.is_empty() {
            let text = extract_text(&candidate.message.parts);
            if !text.is_empty() {
                return Ok(AgentAnswer {
                    text,
                    degraded: false,
                });
            }

            if invalid_response_attempts >= config.invalid_response_retries {
                return Ok(degraded(
                    "Assistant returned an empty response repeatedly and could not complete the tool flow.",
                ));
            }
            invalid_response_attempts += 1;
            messages.push(repair_prompt_message());
            continue;
        }

        let responses = dispatch_calls(capabilities, &calls);
        messages.push(AssistantMessage {
            role: AssistantRole::User,
            parts: responses,
        });
    }

    Ok(degraded(
        "Assistant reached the step limit while reasoning about your question.",
    ))
}

fn degraded(message: impl Into<String>) -> AgentAnswer {
    AgentAnswer {
        text: message.into(),
        degraded: true,
    }
}

fn repair_prompt_message() -> AssistantMessage {
    AssistantMessage {
        role: AssistantRole::User,
        parts: vec![AssistantPart::Text(
            "Your previous response was invalid for this tool loop. Either call a declared function or provide a non-empty plain-text final answer."
                .to_string(),
        )],
    }
}

fn select_candidate(candidates: &[AssistantCandidate]) -> Option<&AssistantCandidate> {
    candidates
        .iter()
        .find(|candidate| !candidate.safety_blocked && !candidate.message.parts.is_empty())
}

fn extract_function_calls(parts: &[AssistantPart]) -> Vec<FunctionCallSpec> {
    parts
        .iter()
        .filter_map(|part| match part {
            AssistantPart::FunctionCall {
                id,
                name,
                args_json,
            } => Some(FunctionCallSpec {
                id: id.clone(),
                name: name.clone(),
                args_json: args_json.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn extract_text(parts: &[AssistantPart]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            AssistantPart::Text(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use crate::agent::{AgentConfig, run_question};
    use crate::llm::provider::{
        AssistantCandidate, AssistantInput, AssistantMessage, AssistantOutput, AssistantPart,
        AssistantRole, LlmError, LlmProvider,
    };
    use crate::python::PythonSession;

    struct FakeProvider {
        responses: Arc<Mutex<VecDeque<Result<AssistantOutput, LlmError>>>>,
    }

    impl FakeProvider {
        fn new(responses: Vec<Result<AssistantOutput, LlmError>>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(VecDeque::from(responses))),
            }
        }
    }

    impl LlmProvider for FakeProvider {
        async fn generate(&self, _input: AssistantInput) -> Result<AssistantOutput, LlmError> {
            self.responses
                .lock()
                .expect("lock")
                .pop_front()
                .expect("queued response")
        }
    }

    #[tokio::test]
    async fn run_question_handles_one_tool_call_then_final_text() {
        let provider = FakeProvider::new(vec![
            Ok(AssistantOutput {
                candidates: vec![AssistantCandidate {
                    message: AssistantMessage {
                        role: AssistantRole::Model,
                        parts: vec![AssistantPart::FunctionCall {
                            id: Some("c1".to_string()),
                            name: "list_globals".to_string(),
                            args_json: json!({}),
                        }],
                    },
                    finish_reason: Some("STOP".to_string()),
                    safety_blocked: false,
                }],
            }),
            Ok(AssistantOutput {
                candidates: vec![AssistantCandidate {
                    message: AssistantMessage {
                        role: AssistantRole::Model,
                        parts: vec![AssistantPart::Text("done".to_string())],
                    },
                    finish_reason: Some("STOP".to_string()),
                    safety_blocked: false,
                }],
            }),
        ]);

        let session = PythonSession::initialize().expect("python");
        let answer = run_question(
            &provider,
            &session,
            "what globals?",
            &AgentConfig::default(),
        )
        .await
        .expect("answer");

        assert_eq!(answer.text, "done");
        assert!(!answer.degraded);
    }
}
