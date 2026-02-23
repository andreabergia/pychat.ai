use std::error::Error;
use std::fmt::{Display, Formatter};

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantRole {
    User,
    Model,
    #[allow(dead_code)]
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantPart {
    Text {
        text: String,
        thought_signature: Option<String>,
    },
    FunctionCall {
        id: Option<String>,
        name: String,
        args_json: Value,
        thought_signature: Option<String>,
    },
    FunctionResponse {
        id: Option<String>,
        name: String,
        response_json: Value,
        thought_signature: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantMessage {
    pub role: AssistantRole,
    pub parts: Vec<AssistantPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters_json_schema: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallingMode {
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantInput {
    pub system_instruction: Option<String>,
    pub messages: Vec<AssistantMessage>,
    pub tools: Vec<FunctionDeclaration>,
    pub tool_calling_mode: ToolCallingMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantCandidate {
    pub message: AssistantMessage,
    pub finish_reason: Option<String>,
    pub safety_blocked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmTokenUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LlmTokenUsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

impl LlmTokenUsageTotals {
    pub fn add_usage(&mut self, usage: Option<&LlmTokenUsage>) {
        let Some(usage) = usage else {
            return;
        };

        if let Some(value) = usage.input_tokens {
            self.input_tokens = self.input_tokens.saturating_add(u64::from(value));
        }
        if let Some(value) = usage.output_tokens {
            self.output_tokens = self.output_tokens.saturating_add(u64::from(value));
        }
        if let Some(value) = usage.total_tokens {
            self.total_tokens = self.total_tokens.saturating_add(u64::from(value));
        }
    }

    pub fn is_zero(&self) -> bool {
        self.input_tokens == 0 && self.output_tokens == 0 && self.total_tokens == 0
    }

    pub fn add_totals(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.total_tokens);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantOutput {
    pub candidates: Vec<AssistantCandidate>,
    pub usage: Option<LlmTokenUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmError {
    MissingApiKey,
    HttpStatus { status: u16, body: String },
    Transport(String),
    Parse(String),
    EmptyCandidates,
}

impl Display for LlmError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingApiKey => write!(f, "missing GEMINI_API_KEY"),
            Self::HttpStatus { status, body } => {
                write!(f, "provider request failed with status {status}: {body}")
            }
            Self::Transport(msg) => write!(f, "provider transport error: {msg}"),
            Self::Parse(msg) => write!(f, "provider parse error: {msg}"),
            Self::EmptyCandidates => write!(f, "provider returned no candidates"),
        }
    }
}

impl Error for LlmError {}

pub type LlmResult<T> = std::result::Result<T, LlmError>;

pub trait LlmProvider {
    fn generate(
        &self,
        input: AssistantInput,
    ) -> impl std::future::Future<Output = LlmResult<AssistantOutput>> + Send;
}
