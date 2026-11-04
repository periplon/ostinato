use ostinato_domain::DomainError;
use ostinato_lang::LangError;
use thiserror::Error;

use crate::ports::PortError;

/// Errors raised by application use cases.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("port failure: {0}")]
    Port(#[from] PortError),
    #[error("domain error: {0}")]
    Domain(#[from] DomainError),
    #[error("language error: {0}")]
    Language(#[from] LangError),
    #[error("run not found: {0}")]
    RunNotFound(String),
    #[error("run {0} is cancelled and cannot be resumed")]
    Cancelled(String),
    #[error("invalid run state: {0}")]
    InvalidState(String),
    #[error("agent failed: {0}")]
    Agent(String),
    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;
