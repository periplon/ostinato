//! Application layer: use cases, ports and the durable workflow engine.
//!
//! The core depends only on the pure [`ostinato_domain`] model and on the
//! language front-end. Infrastructure is injected through the traits in
//! [`ports`].

pub mod activation;
pub mod engine;
pub mod error;
pub mod improve;
pub mod ports;

pub use engine::{Engine, EngineDeps, EngineOptions, run_loaded};
pub use error::{EngineError, Result};
pub use improve::{ImproveRequest, ImprovementRun, params_patch, propose_improvement};
pub use ports::{
    AgentOutcome, AgentRequest, AgentRunner, CaptureOutcome, CaptureRequest, Clock,
    CommandExecutor, Completion, DataInput, DocumentCodec, DocumentEvaluator, DocumentFormat,
    EventCursor, ExecOutcome, ExecRequest, IdGenerator, IsolationRequest, LoadRequest,
    LoadedWorkflow, MetricsSummary, NoTelemetry, OutputOptions, OutputSink, PortError, PortResult,
    RunSnapshot, SpecLoader, Store, Telemetry, WorkflowEventQuery, WorkspaceHandle,
    WorkspaceProvider, WriteOutcome,
};
