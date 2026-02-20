use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::Value;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentProgressEvent {
    StepStarted {
        step: usize,
    },
    ModelResponse {
        step: usize,
        thought_signatures: usize,
        tool_calls: usize,
        has_text: bool,
    },
    ToolRequest {
        step: usize,
        id: Option<String>,
        name: String,
        args_json: Value,
    },
    ToolResult {
        step: usize,
        id: Option<String>,
        name: String,
        response_json: Value,
    },
}

pub async fn run_question_with_events<
    P: LlmProvider,
    C: CapabilityProvider,
    F: FnMut(AgentProgressEvent),
>(
    provider: &P,
    capabilities: &C,
    question: &str,
    config: &AgentConfig,
    on_event: &mut F,
) -> Result<AgentAnswer> {
    let mut messages = vec![AssistantMessage {
        role: AssistantRole::User,
        parts: vec![AssistantPart::Text {
            text: question.to_string(),
            thought_signature: None,
        }],
    }];
    let tools = tool_declarations();
    let total_deadline = Instant::now() + Duration::from_millis(config.total_timeout_ms);
    let mut invalid_response_attempts = 0usize;

    for step in 1..=config.max_steps {
        on_event(AgentProgressEvent::StepStarted { step });

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

        let calls = extract_function_calls(&candidate.message.parts);
        let text = extract_text(&candidate.message.parts);
        on_event(AgentProgressEvent::ModelResponse {
            step,
            thought_signatures: count_thought_signatures(&candidate.message.parts),
            tool_calls: calls.len(),
            has_text: !text.is_empty(),
        });

        messages.push(candidate.message.clone());

        if calls.is_empty() {
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

        for call in &calls {
            on_event(AgentProgressEvent::ToolRequest {
                step,
                id: call.id.clone(),
                name: call.name.clone(),
                args_json: call.args_json.clone(),
            });
        }

        let responses = dispatch_calls(capabilities, &calls);
        for response in &responses {
            if let AssistantPart::FunctionResponse {
                id,
                name,
                response_json,
                ..
            } = response
            {
                on_event(AgentProgressEvent::ToolResult {
                    step,
                    id: id.clone(),
                    name: name.clone(),
                    response_json: response_json.clone(),
                });
            }
        }
        messages.push(AssistantMessage {
            role: AssistantRole::User,
            parts: responses,
        });
    }

    let now = Instant::now();
    if now < total_deadline {
        let remaining = total_deadline.duration_since(now);
        let per_step = Duration::from_millis(config.per_step_timeout_ms);
        let timeout_budget = per_step.min(remaining);
        if !timeout_budget.is_zero()
            && let Some(text) = finalize_without_tools(provider, &messages, timeout_budget).await
        {
            return Ok(AgentAnswer {
                text,
                degraded: true,
            });
        }
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
        parts: vec![AssistantPart::Text {
            text: "Your previous response was invalid for this tool loop. Either call a declared function or provide a non-empty plain-text final answer."
                .to_string(),
            thought_signature: None,
        }],
    }
}

fn select_candidate(candidates: &[AssistantCandidate]) -> Option<&AssistantCandidate> {
    candidates
        .iter()
        .find(|candidate| {
            is_usable_candidate(candidate)
                && !has_function_calls(&candidate.message.parts)
                && has_non_empty_text(&candidate.message.parts)
        })
        .or_else(|| {
            candidates.iter().find(|candidate| {
                is_usable_candidate(candidate) && has_function_calls(&candidate.message.parts)
            })
        })
        .or_else(|| {
            candidates
                .iter()
                .find(|candidate| is_usable_candidate(candidate))
        })
}

fn is_acceptable_finish_reason(reason: Option<&str>) -> bool {
    match reason {
        None => true,
        Some("STOP") | Some("MAX_TOKENS") => true,
        Some("SAFETY") | Some("RECITATION") | Some("BLOCKLIST") | Some("PROHIBITED_CONTENT") => {
            false
        }
        Some(_) => true,
    }
}

fn is_usable_candidate(candidate: &AssistantCandidate) -> bool {
    !candidate.safety_blocked
        && !candidate.message.parts.is_empty()
        && is_acceptable_finish_reason(candidate.finish_reason.as_deref())
}

fn has_function_calls(parts: &[AssistantPart]) -> bool {
    parts
        .iter()
        .any(|part| matches!(part, AssistantPart::FunctionCall { .. }))
}

fn has_non_empty_text(parts: &[AssistantPart]) -> bool {
    !extract_text(parts).is_empty()
}

fn count_thought_signatures(parts: &[AssistantPart]) -> usize {
    parts
        .iter()
        .filter(|part| match part {
            AssistantPart::Text {
                thought_signature, ..
            }
            | AssistantPart::FunctionCall {
                thought_signature, ..
            }
            | AssistantPart::FunctionResponse {
                thought_signature, ..
            } => thought_signature.is_some(),
        })
        .count()
}

async fn finalize_without_tools<P: LlmProvider>(
    provider: &P,
    messages: &[AssistantMessage],
    timeout_budget: Duration,
) -> Option<String> {
    let llm = timeout(
        timeout_budget,
        provider.generate(AssistantInput {
            system_instruction: Some(format!(
                "{AGENT_SYSTEM_PROMPT}\n\nThe tool loop is complete. Do not call functions. Provide the best concise plain-text answer from available context."
            )),
            messages: messages.to_vec(),
            tools: vec![],
            tool_calling_mode: ToolCallingMode::Auto,
        }),
    )
    .await
    .ok()?
    .ok()?;

    let candidate = select_candidate(&llm.candidates)?;
    let text = extract_text(&candidate.message.parts);
    if text.is_empty() { None } else { Some(text) }
}

fn extract_function_calls(parts: &[AssistantPart]) -> Vec<FunctionCallSpec> {
    parts
        .iter()
        .filter_map(|part| match part {
            AssistantPart::FunctionCall {
                id,
                name,
                args_json,
                ..
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
            AssistantPart::Text { text, .. } => {
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

    use crate::agent::{AgentConfig, run_question_with_events};
    use crate::llm::provider::{
        AssistantCandidate, AssistantInput, AssistantMessage, AssistantOutput, AssistantPart,
        AssistantRole, LlmError, LlmProvider,
    };
    use crate::python::PythonSession;

    struct FakeProvider {
        responses: Arc<Mutex<VecDeque<Result<AssistantOutput, LlmError>>>>,
        seen_inputs: Arc<Mutex<Vec<AssistantInput>>>,
    }

    impl FakeProvider {
        fn new(responses: Vec<Result<AssistantOutput, LlmError>>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(VecDeque::from(responses))),
                seen_inputs: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl LlmProvider for FakeProvider {
        async fn generate(&self, input: AssistantInput) -> Result<AssistantOutput, LlmError> {
            self.seen_inputs.lock().expect("lock").push(input);
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
                            thought_signature: Some("sig".to_string()),
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
                        parts: vec![AssistantPart::Text {
                            text: "done".to_string(),
                            thought_signature: None,
                        }],
                    },
                    finish_reason: Some("STOP".to_string()),
                    safety_blocked: false,
                }],
            }),
        ]);

        let session = PythonSession::initialize().expect("python");
        let answer = run_question_with_events(
            &provider,
            &session,
            "what globals?",
            &AgentConfig::default(),
            &mut |_| {},
        )
        .await
        .expect("answer");

        assert_eq!(answer.text, "done");
        assert!(!answer.degraded);
    }

    #[tokio::test]
    async fn run_question_skips_unusable_first_candidate() {
        let provider = FakeProvider::new(vec![Ok(AssistantOutput {
            candidates: vec![
                AssistantCandidate {
                    message: AssistantMessage {
                        role: AssistantRole::Model,
                        parts: vec![AssistantPart::Text {
                            text: "blocked".to_string(),
                            thought_signature: None,
                        }],
                    },
                    finish_reason: Some("SAFETY".to_string()),
                    safety_blocked: true,
                },
                AssistantCandidate {
                    message: AssistantMessage {
                        role: AssistantRole::Model,
                        parts: vec![AssistantPart::Text {
                            text: "usable".to_string(),
                            thought_signature: None,
                        }],
                    },
                    finish_reason: Some("STOP".to_string()),
                    safety_blocked: false,
                },
            ],
        })]);

        let session = PythonSession::initialize().expect("python");
        let answer = run_question_with_events(
            &provider,
            &session,
            "say something",
            &AgentConfig::default(),
            &mut |_| {},
        )
        .await
        .expect("answer");

        assert_eq!(answer.text, "usable");
        assert!(!answer.degraded);
    }

    #[tokio::test]
    async fn run_question_retries_once_after_invalid_response() {
        let provider = FakeProvider::new(vec![
            Ok(AssistantOutput {
                candidates: vec![AssistantCandidate {
                    message: AssistantMessage {
                        role: AssistantRole::Model,
                        parts: vec![AssistantPart::Text {
                            text: " ".to_string(),
                            thought_signature: None,
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
                        parts: vec![AssistantPart::Text {
                            text: "recovered".to_string(),
                            thought_signature: None,
                        }],
                    },
                    finish_reason: Some("STOP".to_string()),
                    safety_blocked: false,
                }],
            }),
        ]);

        let session = PythonSession::initialize().expect("python");
        let answer = run_question_with_events(
            &provider,
            &session,
            "retry flow",
            &AgentConfig::default(),
            &mut |_| {},
        )
        .await
        .expect("answer");

        assert_eq!(answer.text, "recovered");
        assert!(!answer.degraded);
    }

    #[tokio::test]
    async fn run_question_handles_multiple_tool_calls_in_one_turn() {
        let provider = FakeProvider::new(vec![
            Ok(AssistantOutput {
                candidates: vec![AssistantCandidate {
                    message: AssistantMessage {
                        role: AssistantRole::Model,
                        parts: vec![
                            AssistantPart::FunctionCall {
                                id: Some("c1".to_string()),
                                name: "list_globals".to_string(),
                                args_json: json!({}),
                                thought_signature: Some("sig1".to_string()),
                            },
                            AssistantPart::FunctionCall {
                                id: Some("c2".to_string()),
                                name: "inspect".to_string(),
                                args_json: json!({"expr": "1"}),
                                thought_signature: Some("sig2".to_string()),
                            },
                        ],
                    },
                    finish_reason: Some("STOP".to_string()),
                    safety_blocked: false,
                }],
            }),
            Ok(AssistantOutput {
                candidates: vec![AssistantCandidate {
                    message: AssistantMessage {
                        role: AssistantRole::Model,
                        parts: vec![AssistantPart::Text {
                            text: "multi ok".to_string(),
                            thought_signature: None,
                        }],
                    },
                    finish_reason: Some("STOP".to_string()),
                    safety_blocked: false,
                }],
            }),
        ]);

        let session = PythonSession::initialize().expect("python");
        let answer = run_question_with_events(
            &provider,
            &session,
            "run multiple",
            &AgentConfig::default(),
            &mut |_| {},
        )
        .await
        .expect("answer");

        assert_eq!(answer.text, "multi ok");
        assert!(!answer.degraded);
    }

    #[tokio::test]
    async fn run_question_degrades_after_retry_budget_exhausted() {
        let provider = FakeProvider::new(vec![
            Ok(AssistantOutput {
                candidates: vec![AssistantCandidate {
                    message: AssistantMessage {
                        role: AssistantRole::Model,
                        parts: vec![],
                    },
                    finish_reason: Some("STOP".to_string()),
                    safety_blocked: false,
                }],
            }),
            Ok(AssistantOutput {
                candidates: vec![AssistantCandidate {
                    message: AssistantMessage {
                        role: AssistantRole::Model,
                        parts: vec![],
                    },
                    finish_reason: Some("STOP".to_string()),
                    safety_blocked: false,
                }],
            }),
        ]);

        let session = PythonSession::initialize().expect("python");
        let answer = run_question_with_events(
            &provider,
            &session,
            "retry fail",
            &AgentConfig::default(),
            &mut |_| {},
        )
        .await
        .expect("answer");

        assert!(answer.degraded);
        assert!(answer.text.contains("invalid response repeatedly"));
    }

    #[tokio::test]
    async fn run_question_uses_no_tool_fallback_after_step_limit() {
        let provider = FakeProvider::new(vec![
            Ok(AssistantOutput {
                candidates: vec![AssistantCandidate {
                    message: AssistantMessage {
                        role: AssistantRole::Model,
                        parts: vec![AssistantPart::FunctionCall {
                            id: Some("c1".to_string()),
                            name: "list_globals".to_string(),
                            args_json: json!({}),
                            thought_signature: None,
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
                        parts: vec![AssistantPart::FunctionCall {
                            id: Some("c2".to_string()),
                            name: "list_globals".to_string(),
                            args_json: json!({}),
                            thought_signature: None,
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
                        parts: vec![AssistantPart::Text {
                            text: "Redefine it: def f():\\n    return 43".to_string(),
                            thought_signature: None,
                        }],
                    },
                    finish_reason: Some("STOP".to_string()),
                    safety_blocked: false,
                }],
            }),
        ]);

        let config = AgentConfig {
            max_steps: 2,
            ..AgentConfig::default()
        };
        let session = PythonSession::initialize().expect("python");
        let answer =
            run_question_with_events(&provider, &session, "change f", &config, &mut |_| {})
                .await
                .expect("answer");

        assert_eq!(answer.text, "Redefine it: def f():\\n    return 43");
        assert!(answer.degraded);

        let inputs = provider.seen_inputs.lock().expect("lock");
        let last = inputs.last().expect("last input");
        assert!(last.tools.is_empty());
    }

    #[tokio::test]
    async fn run_question_skips_fallback_when_total_budget_exhausted() {
        let provider = FakeProvider::new(vec![]);

        let config = AgentConfig {
            max_steps: 0,
            per_step_timeout_ms: 8_000,
            total_timeout_ms: 0,
            invalid_response_retries: 1,
        };
        let session = PythonSession::initialize().expect("python");
        let answer =
            run_question_with_events(&provider, &session, "change f", &config, &mut |_| {})
                .await
                .expect("answer");

        assert!(answer.degraded);
        assert!(answer.text.contains("step limit"));
        assert!(provider.seen_inputs.lock().expect("lock").is_empty());
    }

    #[test]
    fn select_candidate_prefers_final_text_over_tool_call() {
        let candidates = vec![
            AssistantCandidate {
                message: AssistantMessage {
                    role: AssistantRole::Model,
                    parts: vec![AssistantPart::FunctionCall {
                        id: Some("c1".to_string()),
                        name: "list_globals".to_string(),
                        args_json: json!({}),
                        thought_signature: None,
                    }],
                },
                finish_reason: Some("STOP".to_string()),
                safety_blocked: false,
            },
            AssistantCandidate {
                message: AssistantMessage {
                    role: AssistantRole::Model,
                    parts: vec![AssistantPart::Text {
                        text: "final answer".to_string(),
                        thought_signature: None,
                    }],
                },
                finish_reason: Some("STOP".to_string()),
                safety_blocked: false,
            },
        ];

        let selected = super::select_candidate(&candidates).expect("selected candidate");
        assert_eq!(super::extract_text(&selected.message.parts), "final answer");
    }
}
