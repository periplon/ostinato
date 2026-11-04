use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::agent::{AgentProvider, IsolationSpec, PromptDelivery};

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
    /// Route tokens through a split or join gateway. Performs no I/O.
    Gateway,
}

impl StepKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exec => "exec",
            Self::Agent => "agent",
            Self::Improve => "improve",
            Self::Noop => "noop",
            Self::Gateway => "gateway",
        }
    }
}

/// How a split gateway fans one token out to its branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitMode {
    /// Activate every branch.
    And,
    /// Activate every branch whose condition is true (or the default).
    Or,
    /// Activate exactly one branch.
    Xor,
}

impl SplitMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
            Self::Xor => "xor",
        }
    }
}

/// How a join gateway synchronises its incoming branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinMode {
    /// Wait for every incoming branch; any dead input makes the join dead.
    And,
    /// Activate when at least one incoming branch is active.
    Or,
    /// Activate when exactly one incoming branch is active.
    Xor,
    /// Activate when at least `GatewaySpec::n` incoming branches are active.
    NOf,
    /// Activate when the configured boolean edge expression is true.
    Expression,
}

impl JoinMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
            Self::Xor => "xor",
            Self::NOf => "n_of",
            Self::Expression => "expression",
        }
    }
}

/// A single outgoing branch of a split gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BranchSpec {
    /// `None` means the branch is unconditional (`true` in CUE).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

/// Gateway routing definition. Exactly one of `split` or `join` is set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GatewaySpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split: Option<SplitMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<JoinMode>,
    /// Outgoing branches, keyed by target step name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub branches: BTreeMap<String, BranchSpec>,
    /// Fallback branch for XOR/OR splits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Quorum size for `join: "n_of"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    /// Boolean edge expression for `join: "expression"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    /// Optional edge labels for `join: "expression"`, keyed by label.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

impl GatewaySpec {
    #[must_use]
    pub fn is_split(&self) -> bool {
        self.split.is_some()
    }

    #[must_use]
    pub fn is_join(&self) -> bool {
        self.join.is_some()
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
///
/// Every field added after the original inline schema is optional with a
/// serialization skip, so existing workflow specs keep the same JSON bytes and
/// therefore the same persisted state digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentSpec {
    pub prompt: String,
    pub system: Option<String>,
    pub model: Option<String>,
    /// Per-step override of the agent command (argv form).
    pub command: Option<Vec<String>>,
    /// Name of the top-level `agents` definition this step uses, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
    #[serde(default, skip_serializing_if = "is_default_provider")]
    pub provider: AgentProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_delivery: Option<PromptDelivery>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<IsolationSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<String>,
}

fn is_default_provider(provider: &AgentProvider) -> bool {
    *provider == AgentProvider::Command
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
    /// Gateway routing definition. Must be absent for non-gateway steps.
    ///
    /// `skip_serializing_if` is mandatory: adding a `null` field would change
    /// the persisted `spec` bytes and therefore every historical state digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<GatewaySpec>,
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
            gateway: None,
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
