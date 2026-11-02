use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Outcome of one step attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrialStatus {
    Succeeded,
    Failed,
    TimedOut,
    Skipped,
}

impl TrialStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Skipped => "skipped",
        }
    }
}

/// Append-only trial log entry. One row per attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trial {
    pub id: i64,
    pub run_id: String,
    pub node_index: i64,
    pub step: String,
    pub round: u32,
    pub attempt: u32,
    pub status: TrialStatus,
    pub score: f64,
    pub duration_ms: u64,
    pub input_hash: String,
    pub output_hash: String,
    pub input: Value,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub created_at: String,
}

/// Data required to append a trial.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewTrial {
    pub run_id: String,
    pub node_index: i64,
    pub step: String,
    pub round: u32,
    pub attempt: u32,
    pub status: TrialStatus,
    pub score: f64,
    pub duration_ms: u64,
    pub input_hash: String,
    pub output_hash: String,
    pub input: Value,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub created_at: String,
}

/// A persisted recursive-improvement proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Improvement {
    pub id: i64,
    pub run_id: String,
    pub parent_id: Option<i64>,
    pub round: u32,
    pub diagnosis: String,
    pub proposal: Value,
    pub created_at: String,
}

/// Data required to persist an improvement proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewImprovement {
    pub run_id: String,
    pub parent_id: Option<i64>,
    pub round: u32,
    pub diagnosis: String,
    pub proposal: Value,
    pub created_at: String,
}
