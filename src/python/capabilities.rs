use std::error::Error;
use std::fmt::{Display, Formatter};

use serde_json::Value;

use super::ExceptionInfo;

pub const REPR_MAX_LEN: usize = 4096;
pub const DOC_MAX_LEN: usize = 4096;
pub const INSPECT_SAMPLE_MAX_ITEMS: usize = 16;
pub const INSPECT_MEMBER_MAX_PER_GROUP: usize = 24;
pub const INSPECT_SOURCE_PREVIEW_MAX_LEN: usize = 1200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalEntry {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InspectInfo {
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalInfo {
    pub value_repr: String,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityError {
    PythonException(ExceptionInfo),
    InvalidResultShape(String),
    Internal(String),
}

impl Display for CapabilityError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PythonException(exc) => write!(f, "{}: {}", exc.exc_type, exc.message),
            Self::InvalidResultShape(msg) => write!(f, "invalid capability result shape: {msg}"),
            Self::Internal(msg) => write!(f, "internal capability error: {msg}"),
        }
    }
}

impl Error for CapabilityError {}

pub type CapabilityResult<T> = std::result::Result<T, CapabilityError>;

pub trait CapabilityProvider {
    fn list_globals(&self) -> CapabilityResult<Vec<GlobalEntry>>;
    fn inspect(&self, expr: &str) -> CapabilityResult<InspectInfo>;
    fn eval_expr(&self, expr: &str) -> CapabilityResult<EvalInfo>;
}
