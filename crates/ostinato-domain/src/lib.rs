//! Pure domain model for Ostinato.
//!
//! This crate contains no I/O, no database, no process execution and no
//! framework dependencies. It is the "inside" of the hexagonal architecture:
//! workflow specifications, run state, execution plans, trials and the
//! tamper-evident audit chain.

pub mod agent;
pub mod analysis;
pub mod audit;
pub mod error;
pub mod plan;
pub mod run;
pub mod trial;
pub mod workflow;

pub use agent::{
    AgentDefinition, AgentProvider, AgentTrace, AgentTraceRecord, ArtifactMeta, CapturedArtifact,
    IsolationMode, IsolationSpec, PromptDelivery, StoredArtifact, TraceVerification, sha256_hex,
};
pub use analysis::{BranchReport, Finding, Severity, analyze};
pub use audit::{AuditEvent, NewEvent, lifecycle_status, state_digest, verify_state_anchor};
pub use error::DomainError;
pub use plan::{
    ActivationScope, CarryEdge, CycleInfo, EdgeOrigin, EdgeScope, Plan, PlanEdge, PlanNode,
    build_plan,
};
pub use run::{RunRecord, RunReport, RunStatus, StepRecord, StepStatus};
pub use trial::{Improvement, NewImprovement, NewTrial, Trial, TrialStatus};
pub use workflow::{
    AgentSpec, AwaitSpec, BranchSpec, CycleSpec, DEFAULT_AWAIT_TIMEOUT_MS, EmitSpec, EventSource,
    ExecSpec, GatewaySpec, ImproveSpec, JoinMode, MAX_EVENT_NAME_LEN, MAX_EVENT_PAYLOAD_BYTES,
    MAX_EVENT_PAYLOAD_DEPTH, NewWorkflowEvent, RetryPolicy, SplitMode, StepKind, StepSpec,
    WorkflowEvent, WorkflowSpec, valid_event_name, validate_workflow_event,
};
