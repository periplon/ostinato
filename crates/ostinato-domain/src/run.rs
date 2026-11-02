use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::workflow::WorkflowSpec;

/// Lifecycle of a workflow run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Interrupted,
    Cancelled,
}

impl RunStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Cancelled => "cancelled",
        }
    }

    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// Lifecycle of a single step for the latest visit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
}

impl StepStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

/// Durable record for a workflow run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub workflow_name: String,
    pub source_file: String,
    /// The fully evaluated specification, persisted so `resume` never needs the
    /// original CUE file to still be present.
    pub spec: WorkflowSpec,
    pub params: Value,
    pub status: RunStatus,
    /// Index into the compiled plan at which execution should resume.
    pub resume_index: i64,
    pub parent_run_id: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Latest audit hash, persisted so tail truncation is detectable.
    pub audit_head: String,
}

/// Durable record for a step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRecord {
    pub run_id: String,
    pub name: String,
    pub status: StepStatus,
    /// Number of completed visits (cycle rounds included).
    pub visits: u32,
    pub attempts: u32,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub updated_at: String,
}

/// Result of driving a run to completion (or to the next failure).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    pub run_id: String,
    pub workflow_name: String,
    pub status: RunStatus,
    pub resume_index: i64,
    pub error: Option<String>,
    pub steps: Vec<StepRecord>,
    pub duration_ms: u64,
}
