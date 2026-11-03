use thiserror::Error;

/// Errors raised by the pure domain layer.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
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
    #[error("step `{0}` is a gateway but has no `gateway` payload")]
    GatewayRequiresSpec(String),
    #[error("gateway `{step}` must not declare `{field}`")]
    GatewayPayloadNotAllowed { step: String, field: String },
    #[error("step `{0}` declares `gateway` but is not kind `gateway`")]
    GatewayOnNonGateway(String),
    #[error("gateway `{0}` declares both `split` and `join`; pick exactly one")]
    GatewaySplitAndJoin(String),
    #[error("gateway `{0}` declares neither `split` nor `join`")]
    GatewayMissingMode(String),
    #[error("split gateway `{0}` must declare at least one branch")]
    GatewayEmptyBranches(String),
    #[error("gateway `{gateway}` targets unknown step `{target}`")]
    GatewayUnknownBranchTarget { gateway: String, target: String },
    #[error("gateway `{0}` cannot branch to itself")]
    GatewaySelfBranch(String),
    #[error("branch target `{target}` of gateway `{gateway}` must not declare `needs`")]
    GatewayBranchTargetHasNeeds { gateway: String, target: String },
    #[error("branch target `{target}` of gateway `{gateway}` is a join gateway")]
    GatewayBranchTargetIsJoin { gateway: String, target: String },
    #[error(
        "step `{step}` must not list split gateway `{gateway}` in `needs`; the edge is implicit"
    )]
    GatewayNeedsRejected { gateway: String, step: String },
    #[error("gateway `{gateway}` has invalid default branch `{branch}`")]
    GatewayInvalidDefault { gateway: String, branch: String },
    #[error("gateway `{gateway}` branch `{branch}` requires a condition")]
    GatewayMissingCondition { gateway: String, branch: String },
    #[error("AND gateway `{gateway}` branch `{branch}` must be unconditional `true`")]
    GatewayAndCondition { gateway: String, branch: String },
    #[error("join gateway `{0}` requires at least one incoming `needs` edge")]
    GatewayJoinWithoutNeeds(String),
    #[error("join gateway `{0}` must not declare `branches` or `default`")]
    GatewayJoinHasBranches(String),
    #[error(
        "gateway `{gateway}` branch `{branch}` references `{reference}` outside its predecessor scope"
    )]
    GatewayConditionScope {
        gateway: String,
        branch: String,
        reference: String,
    },
    #[error(
        "workflow combines gateways and cycles (workflow.steps.{steps:?}); gateways inside cycles are not supported in v1"
    )]
    GatewayCycleUnsupported { steps: Vec<String> },
    #[error(
        "workflow.steps.{0} has more than one incoming edge in a gateway workflow; use an explicit join gateway"
    )]
    ImplicitJoinWithGateways(String),
    #[error("gateway `{0}` selected no branch and has no default")]
    GatewayNoBranch(String),
    #[error("gateway `{0}` selected more than one branch in a XOR/ambiguous join")]
    GatewayAmbiguous(String),
    #[error("gateway `{gateway}` condition failed: {message}")]
    GatewayCondition { gateway: String, message: String },
    #[error("invalid audit chain at sequence {seq}: {reason}")]
    InvalidAuditChain { seq: u64, reason: String },
    #[error("audit event sequence is not contiguous at {0}")]
    AuditSequence(u64),
    #[error("serialization failure: {0}")]
    Serialization(String),
}
