use thiserror::Error;

/// Errors produced by the language front-end.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LangError {
    #[error("parse error at {line}:{col}: {message}")]
    Parse {
        line: u32,
        col: u32,
        message: String,
    },
    #[error("evaluation error at {line}:{col}: {message}")]
    Eval {
        line: u32,
        col: u32,
        message: String,
    },
    /// A value conflict that behaves as bottom inside a disjunction.
    #[error("evaluation error at {line}:{col}: {message}")]
    Conflict {
        line: u32,
        col: u32,
        message: String,
    },
    /// A required expression referenced an optional field that has not been
    /// materialized by unification yet.
    #[error("evaluation error at {line}:{col}: {message}")]
    IncompleteOptional {
        line: u32,
        col: u32,
        message: String,
    },
    #[error("workflow error: {0}")]
    Workflow(String),
    #[error("invalid condition `{expr}`: {message}")]
    Condition { expr: String, message: String },
    #[error("invalid template: {0}")]
    Template(String),
    #[error("invalid parameter `{name}`: {message}")]
    Param { name: String, message: String },
}

impl LangError {
    pub(crate) fn parse(line: u32, col: u32, message: impl Into<String>) -> Self {
        Self::Parse {
            line,
            col,
            message: message.into(),
        }
    }

    pub(crate) fn eval(line: u32, col: u32, message: impl Into<String>) -> Self {
        Self::Eval {
            line,
            col,
            message: message.into(),
        }
    }

    pub(crate) fn conflict(line: u32, col: u32, message: impl Into<String>) -> Self {
        Self::Conflict {
            line,
            col,
            message: message.into(),
        }
    }

    pub(crate) fn incomplete_optional(line: u32, col: u32, message: impl Into<String>) -> Self {
        Self::IncompleteOptional {
            line,
            col,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn is_conflict(&self) -> bool {
        matches!(self, Self::Conflict { .. })
    }

    #[must_use]
    pub(crate) fn is_incomplete_optional(&self) -> bool {
        matches!(self, Self::IncompleteOptional { .. })
    }
}
