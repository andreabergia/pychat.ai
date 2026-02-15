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
    Text(String),
    FunctionCall {
        id: Option<String>,
        name: String,
        args_json: Value,
    },
    FunctionResponse {
        id: Option<String>,
        name: String,
        response_json: Value,
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
pub struct AssistantOutput {
    pub candidates: Vec<AssistantCandidate>,
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
