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
    #[error("join gateway `{0}` with `n_of` requires `n`")]
    GatewayQuorumMissingN(String),
    #[error(
        "join gateway `{gateway}` has duplicate need `{need}`; `n_of` requires distinct inputs"
    )]
    GatewayDuplicateNeeds { gateway: String, need: String },
    #[error("join gateway `{gateway}` has invalid quorum `{n}`; expected 1..={max}")]
    GatewayQuorumInvalidN { gateway: String, n: u32, max: usize },
    #[error("join gateway `{gateway}` requires {required} active inputs but only {active} arrived")]
    GatewayQuorumNotMet {
        gateway: String,
        required: u32,
        active: usize,
    },
    #[error("gateway `{0}` declares `n` but is not an `n_of` join")]
    GatewayQuorumOnWrongMode(String),
    #[error("join gateway `{0}` with `expression` requires `expression`")]
    GatewayExpressionMissing(String),
    #[error("gateway `{gateway}` label `{label}` does not name a join input")]
    GatewayLabelUnknownStep { gateway: String, label: String },
    #[error("gateway `{gateway}` maps multiple labels to the same input `{target}`")]
    GatewayDuplicateLabelTarget { gateway: String, target: String },
    #[error("gateway `{gateway}` has no label for input `{target}`")]
    GatewayLabelMissing { gateway: String, target: String },
    #[error("gateway `{gateway}` has an invalid edge label `{label}`")]
    GatewayInvalidLabel { gateway: String, label: String },
    #[error(
        "workflow.steps.{0}.gateway mixes cycle carry and same-round inputs; split it into separate gateways"
    )]
    GatewayCycleMixedScopeJoin(String),
    #[error(
        "workflow.steps.{gateway}.gateway.branches.{target} leaves its cycle; branch targets must stay inside the cycle"
    )]
    GatewayBranchLeavesCycle { gateway: String, target: String },
    #[error(
        "workflow.steps.{gateway}.gateway.branches.{target} enters a cycle from outside; add the gateway to the cycle"
    )]
    GatewayBranchIntoCycle { gateway: String, target: String },
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
    #[error("audit chain has no state anchor for run `{0}`")]
    MissingStateAnchor(String),
    #[error("execution state is not anchored in the audit chain for run `{0}`")]
    StateAnchorMismatch(String),
    #[error("audit event sequence is not contiguous at {0}")]
    AuditSequence(u64),
    #[error("serialization failure: {0}")]
    Serialization(String),
}
