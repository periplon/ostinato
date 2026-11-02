use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The kind of work a step performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// Run an external command deterministically.
    Exec,
    /// Ask an agent (through the configured agent adapter) to do something.
    Agent,
    /// Summarise trials and ask an agent to propose an improvement.
    Improve,
    /// Do nothing. Useful as a join or as a cycle marker.
    Noop,
}

impl StepKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exec => "exec",
            Self::Agent => "agent",
            Self::Improve => "improve",
            Self::Noop => "noop",
        }
    }
}

/// Retry policy for a single step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Number of retries *after* the first attempt.
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub factor: f64,
    pub max_delay_ms: u64,
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 0,
            base_delay_ms: 100,
            factor: 2.0,
            max_delay_ms: 30_000,
            jitter: true,
        }
    }
}

impl RetryPolicy {
    /// Delay before `attempt` (0-based, where 0 is the first retry).
    #[must_use]
    pub fn delay_for(&self, attempt: u32, seed: u64) -> Duration {
        let exponent = i32::try_from(attempt).unwrap_or(i32::MAX);
        let factor = self.factor.max(1.0);
        let raw = (self.base_delay_ms as f64) * factor.powi(exponent);
        let capped = if raw.is_finite() {
            raw.min(self.max_delay_ms as f64)
        } else {
            self.max_delay_ms as f64
        };
        let jittered = if self.jitter {
            // Deterministic jitter in [0.5, 1.5) derived from attempt + seed.
            let mixed = splitmix64(
                seed.wrapping_add(u64::from(attempt))
                    .wrapping_add(0x9E37_79B9_7F4A_7C15),
            );
            let jitter_factor = 0.5 + (mixed >> 11) as f64 / (1u64 << 53) as f64;
            capped * jitter_factor
        } else {
            capped
        };
        Duration::from_millis(jittered.round().max(0.0) as u64)
    }
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Bounded repetition for a step that participates in a cycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CycleSpec {
    /// Maximum number of rounds this cycle may execute.
    pub max_rounds: u32,
    /// Optional runtime condition. When it evaluates to true the remaining
    /// rounds of the cycle are skipped. Example: `steps.review.score >= 0.9`.
    pub until: Option<String>,
}

/// Deterministic command execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExecSpec {
    /// argv form. The first element is the program.
    pub cmd: Vec<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub timeout_ms: Option<u64>,
}

/// Agent invocation. The adapter decides how to reach the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentSpec {
    pub prompt: String,
    pub system: Option<String>,
    pub model: Option<String>,
    /// Per-step override of the agent command (argv form).
    pub command: Option<Vec<String>>,
}

/// Recursive-improvement primitive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ImproveSpec {
    pub prompt: Option<String>,
    /// How many recent trials to include in the improvement prompt.
    pub max_samples: usize,
}

/// A single workflow step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepSpec {
    pub name: String,
    pub kind: StepKind,
    pub needs: Vec<String>,
    pub retry: RetryPolicy,
    pub cycle: Option<CycleSpec>,
    pub exec: Option<ExecSpec>,
    pub agent: Option<AgentSpec>,
    pub improve: Option<ImproveSpec>,
    pub continue_on_error: bool,
    pub timeout_ms: Option<u64>,
}

impl StepSpec {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: StepKind::Noop,
            needs: Vec::new(),
            retry: RetryPolicy::default(),
            cycle: None,
            exec: None,
            agent: None,
            improve: None,
            continue_on_error: false,
            timeout_ms: None,
        }
    }
}

/// A complete, validated workflow specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowSpec {
    pub name: String,
    pub max_parallel: usize,
    pub steps: BTreeMap<String, StepSpec>,
}

impl WorkflowSpec {
    #[must_use]
    pub fn step(&self, name: &str) -> Option<&StepSpec> {
        self.steps.get(name)
    }
}
