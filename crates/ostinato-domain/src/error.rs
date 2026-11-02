use thiserror::Error;

/// Errors raised by the pure domain layer.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("unknown step referenced in `needs`: {0}")]
    UnknownDependency(String),
    #[error("step `{0}` must not depend on itself without a cycle policy")]
    SelfDependency(String),
    #[error(
        "cycle containing {steps:?} has conflicting `until` conditions; use one condition per cycle"
    )]
    ConflictingCycleConditions { steps: Vec<String> },
    #[error(
        "cycle containing {steps:?} has no `cycle` policy; add `cycle: {{max: N, until: \"...\"}}` to at least one member"
    )]
    CycleWithoutPolicy { steps: Vec<String> },
    #[error("cycle `{0}` must have max >= 1")]
    InvalidCycleLimit(String),
    #[error("workflow must contain at least one step")]
    EmptyWorkflow,
    #[error("cycle policy on step `{0}` has no graph cycle; remove it or add a back-edge")]
    CycleOnAcyclicStep(String),
    #[error("materialised plan exceeds the limit of {max} nodes")]
    PlanTooLarge { max: usize },
    #[error("maxParallel must be >= 1")]
    InvalidParallelism,
    #[error("invalid audit chain at sequence {seq}: {reason}")]
    InvalidAuditChain { seq: u64, reason: String },
    #[error("audit event sequence is not contiguous at {0}")]
    AuditSequence(u64),
    #[error("serialization failure: {0}")]
    Serialization(String),
}
