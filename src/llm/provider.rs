use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantInput {
    pub user_message: String,
    pub system_instruction: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantOutput {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmError {
    MissingApiKey,
    HttpStatus { status: u16, body: String },
    Transport(String),
    Parse(String),
    EmptyResponse,
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
            Self::EmptyResponse => write!(f, "provider returned empty response text"),
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
