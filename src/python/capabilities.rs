use std::error::Error;
use std::fmt::{Display, Formatter};

use super::ExceptionInfo;

pub const REPR_MAX_LEN: usize = 4096;
pub const DOC_MAX_LEN: usize = 4096;
pub const DIR_MAX_MEMBERS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalEntry {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeInfo {
    pub name: String,
    pub module: String,
    pub qualified: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReprInfo {
    pub repr: String,
    pub truncated: bool,
    pub original_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirInfo {
    pub members: Vec<String>,
    pub truncated: bool,
    pub original_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocInfo {
    pub doc: Option<String>,
    pub truncated: bool,
    pub original_len: usize,
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
    fn get_type(&self, expr: &str) -> CapabilityResult<TypeInfo>;
    fn get_repr(&self, expr: &str) -> CapabilityResult<ReprInfo>;
    fn get_dir(&self, expr: &str) -> CapabilityResult<DirInfo>;
    fn get_doc(&self, expr: &str) -> CapabilityResult<DocInfo>;
    fn eval_expr(&self, expr: &str) -> CapabilityResult<EvalInfo>;
    fn get_last_exception(&self) -> CapabilityResult<Option<ExceptionInfo>>;
}

pub fn truncate_text(value: String, max_chars: usize) -> (String, bool, usize) {
    let original_len = value.chars().count();
    if original_len <= max_chars {
        return (value, false, original_len);
    }

    let truncated = value.chars().take(max_chars).collect::<String>();
    (truncated, true, original_len)
}

pub fn truncate_members(
    mut members: Vec<String>,
    max_members: usize,
) -> (Vec<String>, bool, usize) {
    let original_len = members.len();
    if original_len <= max_members {
        return (members, false, original_len);
    }

    members.truncate(max_members);
    (members, true, original_len)
}
