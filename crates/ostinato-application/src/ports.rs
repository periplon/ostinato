use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ostinato_domain::{
    AgentProvider, AgentTrace, AgentTraceRecord, AuditEvent, CapturedArtifact, EventSource,
    Improvement, IsolationMode, NewEvent, NewImprovement, NewTrial, NewWorkflowEvent,
    PromptDelivery, RunRecord, RunStatus, StepRecord, Trial, WorkflowEvent, WorkflowSpec,
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
    #[error("corrupt durable data: {0}")]
    Corrupt(String),
    #[error("run was cancelled: {0}")]
    Cancelled(String),
}

pub type PortResult<T> = Result<T, PortError>;

/// Request to evaluate a workflow source file.
#[derive(Debug, Clone)]
pub struct LoadRequest {
    /// CUE package file or directory to evaluate.
    pub source: PathBuf,
    /// Scalar `-p key=value` parameter overrides.
    pub params: BTreeMap<String, String>,
    /// Repeatable `--input` JSON/YAML files unified into the CUE root.
    pub inputs: Vec<DataInput>,
}

impl LoadRequest {
    /// Build a request with no extra inputs.
    #[must_use]
    pub fn new(source: PathBuf, params: BTreeMap<String, String>) -> Self {
        Self {
            source,
            params,
            inputs: Vec::new(),
        }
    }
}

/// A JSON/YAML file unified into a CUE root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataInput {
    pub path: PathBuf,
}

impl DataInput {
    /// Wrap a path as a data input.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

/// Data encoding supported by the CLI and codecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentFormat {
    Json,
    Yaml,
}

impl DocumentFormat {
    /// Human-readable name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Yaml => "yaml",
        }
    }
}

/// Decode and encode JSON/YAML documents.
pub trait DocumentCodec: Send + Sync {
    /// Read every document in a data file.
    ///
    /// # Errors
    /// Returns a typed error for missing files, malformed documents, size or
    /// nesting limits, duplicate keys or unsupported YAML tags.
    fn read(&self, input: &DataInput) -> PortResult<Vec<Value>>;

    /// Encode a value in the requested format.
    ///
    /// # Errors
    /// Returns a typed error when the value cannot be represented.
    fn encode(&self, format: DocumentFormat, value: &Value) -> PortResult<String>;

    /// Encode one or more values as a stream.
    ///
    /// JSON values are separated by newlines; YAML values are separated by
    /// `---` document markers.
    ///
    /// # Errors
    /// Propagates [`DocumentCodec::encode`] errors.
    fn encode_stream(&self, format: DocumentFormat, values: &[Value]) -> PortResult<String> {
        let mut encoded = Vec::with_capacity(values.len());
        for value in values {
            encoded.push(self.encode(format, value)?);
        }
        Ok(match format {
            DocumentFormat::Json => encoded.concat(),
            DocumentFormat::Yaml => encoded.join("---\n"),
        })
    }
}

/// Options for writing an output document.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutputOptions {
    /// Replace an existing destination.
    pub force: bool,
    /// Skip an existing destination instead of failing.
    pub skip_existing: bool,
}

/// Result of an output write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    Written,
    Skipped,
}

/// Write encoded output to stdout or a safe file destination.
pub trait OutputSink: Send + Sync {
    /// Write `content` to `target` (`None` means stdout).
    ///
    /// Implementations must refuse to overwrite a symlink, a hard-linked file
    /// or any path in `protected` (the input files), and must write atomically.
    ///
    /// # Errors
    /// Returns an I/O or safety error.
    fn write(
        &self,
        target: Option<&Path>,
        content: &str,
        options: OutputOptions,
        protected: &[PathBuf],
    ) -> PortResult<WriteOutcome>;
}

/// Evaluated workflow plus the concrete parameters used.
#[derive(Debug, Clone)]
pub struct LoadedWorkflow {
    pub spec: WorkflowSpec,
    pub params: Value,
    pub source_file: String,
    pub evaluator: String,
}

/// Port for evaluating raw CUE/data roots for export.
pub trait DocumentEvaluator: Send + Sync {
    /// Evaluator name (`rust`, `cue`).
    fn evaluator(&self) -> &'static str;

    /// Evaluate the source and return one JSON value per expression.
    ///
    /// An empty `expressions` slice returns the root value. `list` requests
    /// CUE's `--list` handling for data inputs.
    ///
    /// # Errors
    /// Returns a loader error for invalid sources, conflicting data, incomplete
    /// values or unsupported expressions.
    fn evaluate(
        &self,
        request: &LoadRequest,
        expressions: &[String],
        list: bool,
    ) -> PortResult<Vec<Value>>;
}

/// Port for the language front-end. The default adapter shells out to the
/// official CUE implementation for full compliance.
pub trait SpecLoader: DocumentEvaluator {
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
    pub run_id: String,
    pub step: String,
    /// Name of the top-level `agents` definition, when one was referenced.
    pub definition: Option<String>,
    pub provider: AgentProvider,
    pub prompt: String,
    pub system: Option<String>,
    pub model: Option<String>,
    pub command: Option<Vec<String>>,
    pub prompt_delivery: Option<PromptDelivery>,
    pub env: BTreeMap<String, String>,
    pub timeout: Option<Duration>,
    /// Working directory: the workflow directory or an isolation worktree.
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct AgentOutcome {
    /// Captured stdout (the agent's result).
    pub text: String,
    pub stdout: String,
    pub stderr: String,
    /// Provider name (`command`, `claude`, `codex`, `opencode`).
    pub provider: String,
    /// Exact argv that was executed.
    pub argv: Vec<String>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub success: bool,
    pub error: Option<String>,
    /// Unix signal that terminated the process, when any.
    pub signal: Option<i32>,
    pub pid: Option<u32>,
    /// Final environment variable names and value hashes.
    pub env_keys: Vec<String>,
    pub env_hashes: BTreeMap<String, String>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// SHA-256 of the exact stdin bytes, when the prompt was delivered on stdin.
    pub stdin_sha256: Option<String>,
}

pub trait AgentRunner: Send + Sync {
    fn run(&self, request: &AgentRequest) -> PortResult<AgentOutcome>;
}

/// Where an agent attempt runs.
#[derive(Debug, Clone)]
pub struct IsolationRequest {
    pub run_id: String,
    pub step: String,
    pub round: u32,
    pub attempt: u32,
    pub mode: IsolationMode,
    pub base: Option<String>,
    pub keep: bool,
    /// Workflow directory used for `none` isolation and git discovery.
    pub base_dir: Option<PathBuf>,
}

/// A prepared workspace for one attempt.
#[derive(Debug, Clone)]
pub struct WorkspaceHandle {
    pub dir: PathBuf,
    pub mode: IsolationMode,
    /// Worktree path when `mode == worktree`.
    pub path: Option<PathBuf>,
    /// Git top-level when `mode == worktree`.
    pub git_root: Option<PathBuf>,
    /// Resolved commit the worktree was created from.
    pub base: Option<String>,
    pub keep: bool,
}

/// Files to capture after an attempt.
#[derive(Debug, Clone)]
pub struct CaptureRequest {
    pub cwd: PathBuf,
    /// Glob patterns declared by the workflow.
    pub patterns: Vec<String>,
    /// Capture git changes relative to this commit (worktree isolation).
    pub base: Option<String>,
}

/// Captured artifacts plus non-fatal warnings (skipped symlinks/hardlinks).
#[derive(Debug, Clone, Default)]
pub struct CaptureOutcome {
    pub artifacts: Vec<CapturedArtifact>,
    pub warnings: Vec<String>,
}

/// Workspace lifecycle and artifact capture. Implemented by the git/glob
/// adapter; the engine only sees this port.
pub trait WorkspaceProvider: Send + Sync {
    fn prepare(&self, request: &IsolationRequest) -> PortResult<WorkspaceHandle>;
    fn capture(&self, request: &CaptureRequest) -> PortResult<CaptureOutcome>;
    /// Remove the worktree when `keep` is false. Errors are recorded on the
    /// trace but never invalidate captured artifacts.
    fn finish(&self, handle: &WorkspaceHandle) -> PortResult<()>;
}

/// Time and identity ports keep the core deterministic in tests.
pub trait Clock: Send + Sync {
    fn now(&self) -> String;
    fn sleep(&self, duration: Duration);
}

pub trait IdGenerator: Send + Sync {
    fn new_run_id(&self) -> String;
}

/// Forward-only per-consumer event cursor keyed by (run, step, event name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventCursor {
    pub run_id: String,
    pub consumer: String,
    pub event: String,
    pub next_seq: u64,
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
    /// Audit events to append in the same transaction.
    pub audit_events: Vec<NewEvent>,
    /// Workflow events emitted by successful steps in this group.
    pub events: Vec<NewWorkflowEvent>,
    /// Forward-only cursor advances for awaits satisfied in this group.
    pub cursor_advances: Vec<EventCursor>,
}

/// Consistent read of a run, its steps and its events in one transaction.
#[derive(Debug, Clone)]
pub struct RunSnapshot {
    pub run: RunRecord,
    pub steps: Vec<StepRecord>,
    pub events: Vec<AuditEvent>,
}

/// Filter for durable workflow event queries.
#[derive(Debug, Clone, Default)]
pub struct WorkflowEventQuery {
    /// Only events with this exact name.
    pub name: Option<String>,
    /// Source filter. `Any`/`None` match internal and external events.
    pub source: Option<EventSource>,
    /// Return only events with `seq > after_seq`.
    pub after_seq: Option<u64>,
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
    /// Append one workflow event durably and return the stored record.
    ///
    /// Implementations must assign a monotonically increasing per-run `seq`,
    /// enforce the event safety limits, and make idempotent engine emissions
    /// keyed by `emission_key` return the previously stored event without
    /// appending again.
    fn append_workflow_event(&self, event: NewWorkflowEvent) -> PortResult<WorkflowEvent>;
    /// Query durable workflow events in `seq` order.
    fn workflow_events(
        &self,
        run_id: &str,
        query: &WorkflowEventQuery,
    ) -> PortResult<Vec<WorkflowEvent>>;
    /// Current forward-only cursor for a consumer/event pair.
    fn event_cursor(
        &self,
        run_id: &str,
        consumer: &str,
        event: &str,
    ) -> PortResult<Option<EventCursor>>;
    fn record_trial(&self, trial: &NewTrial) -> PortResult<i64>;
    fn trials(&self, run_id: &str) -> PortResult<Vec<Trial>>;
    /// Persist one agent trace and its artifacts, and append the anchoring
    /// `agent.traced` audit event in a single transaction.
    fn record_agent_trace(
        &self,
        trace: &AgentTrace,
        artifacts: &[CapturedArtifact],
    ) -> PortResult<AgentTraceRecord>;
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
