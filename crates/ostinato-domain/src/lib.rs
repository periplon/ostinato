//! Pure domain model for Ostinato.
//!
//! This crate contains no I/O, no database, no process execution and no
//! framework dependencies. It is the "inside" of the hexagonal architecture:
//! workflow specifications, run state, execution plans, trials and the
//! tamper-evident audit chain.

pub mod audit;
pub mod error;
pub mod plan;
pub mod run;
pub mod trial;
pub mod workflow;

pub use audit::{AuditEvent, NewEvent, lifecycle_status, state_digest};
pub use error::DomainError;
pub use plan::{Plan, PlanNode, build_plan};
pub use run::{RunRecord, RunReport, RunStatus, StepRecord, StepStatus};
pub use trial::{Improvement, NewImprovement, NewTrial, Trial, TrialStatus};
pub use workflow::{
    AgentSpec, CycleSpec, ExecSpec, ImproveSpec, RetryPolicy, StepKind, StepSpec, WorkflowSpec,
};
