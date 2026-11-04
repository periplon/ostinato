use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use ostinato_domain::{
    AuditEvent, Improvement, NewEvent, NewImprovement, NewTrial, RunRecord, RunStatus, StepRecord,
    Trial, WorkflowSpec,
};
use serde_json::Value;
use thiserror::Error;

/// Infrastructure failures. Domain/rule failures use different error types.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PortError {
    #[error("store error: {0}")]
    Store(String),
    #[error("executor error: {0}")]
    Executor(String),
    #[error("agent error: {0}")]
    Agent(String),
    #[error("loader error: {0}")]
    Loader(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("run was cancelled: {0}")]
    Cancelled(String),
}

pub type PortResult<T> = Result<T, PortError>;

/// Request to evaluate a workflow source file.
#[derive(Debug, Clone)]
pub struct LoadRequest {
    pub source: PathBuf,
    pub params: BTreeMap<String, String>,
}

/// Evaluated workflow plus the concrete parameters used.
#[derive(Debug, Clone)]
pub struct LoadedWorkflow {
    pub spec: WorkflowSpec,
    pub params: Value,
    pub source_file: String,
    pub evaluator: String,
}

/// Port for the language front-end. The default adapter shells out to the
/// official CUE implementation for full compliance.
pub trait SpecLoader: Send + Sync {
    fn evaluator(&self) -> &'static str;
    fn load(&self, request: &LoadRequest) -> PortResult<LoadedWorkflow>;
}

/// Deterministic command execution request.
#[derive(Debug, Clone)]
pub struct ExecRequest {
    pub step: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub timeout: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct ExecOutcome {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl ExecOutcome {
    #[must_use]
    pub fn succeeded(&self) -> bool {
        !self.timed_out && self.exit_code == 0
    }
}

pub trait CommandExecutor: Send + Sync {
    fn execute(&self, request: &ExecRequest) -> PortResult<ExecOutcome>;
}

/// Agent invocation request. The runner decides which provider to call.
#[derive(Debug, Clone)]
pub struct AgentRequest {
    pub step: String,
    pub prompt: String,
    pub system: Option<String>,
    pub model: Option<String>,
    pub command: Option<Vec<String>>,
    pub timeout: Option<Duration>,
    /// Default working directory (the workflow file's directory).
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct AgentOutcome {
    pub text: String,
    pub provider: String,
}

pub trait AgentRunner: Send + Sync {
    fn run(&self, request: &AgentRequest) -> PortResult<AgentOutcome>;
}

/// Time and identity ports keep the core deterministic in tests.
pub trait Clock: Send + Sync {
    fn now(&self) -> String;
    fn sleep(&self, duration: Duration);
}

pub trait IdGenerator: Send + Sync {
    fn new_run_id(&self) -> String;
}

/// Data committed atomically after a parallel group completes.
#[derive(Debug, Clone)]
pub struct Completion {
    pub run_id: String,
    pub resume_index: i64,
    pub status: RunStatus,
    pub error: Option<String>,
    pub updated_at: String,
    pub steps: Vec<StepRecord>,
    pub events: Vec<NewEvent>,
}

/// Consistent read of a run, its steps and its events in one transaction.
#[derive(Debug, Clone)]
pub struct RunSnapshot {
    pub run: RunRecord,
    pub steps: Vec<StepRecord>,
    pub events: Vec<AuditEvent>,
}

/// Aggregate metrics used by the observability CLI.
#[derive(Debug, Clone, Default)]
pub struct MetricsSummary {
    pub runs_total: u64,
    pub runs_succeeded: u64,
    pub runs_failed: u64,
    pub runs_running: u64,
    pub steps_total: u64,
    pub steps_failed: u64,
    pub steps_skipped: u64,
    pub gateways_evaluated: u64,
    pub gateway_failures: u64,
    pub gateway_quorum_short: u64,
    pub attempts_total: u64,
    pub retries_total: u64,
    pub avg_attempt_ms: f64,
}

/// Durable state port. Adapters must make `complete` and `append_event`
/// transactional and must preserve the audit hash chain.
pub trait Store: Send + Sync {
    fn create_run(&self, run: &RunRecord) -> PortResult<()>;
    /// Insert a run and its first audit event in one transaction.
    fn create_run_with_event(
        &self,
        run: &RunRecord,
        event: NewEvent,
        ts: &str,
    ) -> PortResult<AuditEvent>;
    fn load_run(&self, run_id: &str) -> PortResult<Option<RunRecord>>;
    fn snapshot(&self, run_id: &str) -> PortResult<Option<RunSnapshot>>;
    fn list_runs(&self, limit: usize) -> PortResult<Vec<RunRecord>>;
    fn update_run_status(
        &self,
        run_id: &str,
        status: RunStatus,
        resume_index: i64,
        error: Option<&str>,
        updated_at: &str,
    ) -> PortResult<()>;
    fn steps(&self, run_id: &str) -> PortResult<Vec<StepRecord>>;
    /// Atomically mark a run running and append its `run.resumed` event.
    fn resume_run(
        &self,
        run_id: &str,
        resume_index: i64,
        updated_at: &str,
        event: NewEvent,
    ) -> PortResult<AuditEvent>;
    fn complete(&self, completion: Completion) -> PortResult<Vec<AuditEvent>>;
    fn advance(
        &self,
        run_id: &str,
        resume_index: i64,
        updated_at: &str,
        event: NewEvent,
    ) -> PortResult<AuditEvent>;
    fn append_event(&self, run_id: &str, event: NewEvent, ts: &str) -> PortResult<AuditEvent>;
    fn events(&self, run_id: &str) -> PortResult<Vec<AuditEvent>>;
    fn record_trial(&self, trial: &NewTrial) -> PortResult<i64>;
    fn trials(&self, run_id: &str) -> PortResult<Vec<Trial>>;
    fn record_improvement(&self, improvement: &NewImprovement) -> PortResult<i64>;
    fn improvements(&self, run_id: &str) -> PortResult<Vec<Improvement>>;
    fn cancel(&self, run_id: &str, updated_at: &str) -> PortResult<AuditEvent>;
    fn metrics(&self, run_id: Option<&str>) -> PortResult<MetricsSummary>;
}

/// Observability port. The core never depends on a logging framework.
pub trait Telemetry: Send + Sync {
    fn event(&self, event: &AuditEvent);
    fn metric(&self, name: &str, value: f64, labels: &[(&str, &str)]);
}

/// A no-op telemetry implementation for tests and library users.
#[derive(Debug, Default)]
pub struct NoTelemetry;

impl Telemetry for NoTelemetry {
    fn event(&self, _event: &AuditEvent) {}
    fn metric(&self, _name: &str, _value: f64, _labels: &[(&str, &str)]) {}
}
