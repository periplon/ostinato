use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// External agentic CLI used to execute an agent step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentProvider {
    /// A user-supplied command (global `--agent-cmd` or `agent.command`).
    #[default]
    Command,
    /// Claude Code (`claude`).
    Claude,
    /// OpenAI Codex CLI (`codex`).
    Codex,
    /// OpenCode (`opencode`).
    Opencode,
}

impl AgentProvider {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
        }
    }

    /// Parse the CUE spelling of a provider. Returns `None` when unknown.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "command" => Some(Self::Command),
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::Opencode),
            _ => None,
        }
    }
}

/// How the rendered prompt reaches the agent process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptDelivery {
    Stdin,
    Arg,
}

impl PromptDelivery {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdin => "stdin",
            Self::Arg => "arg",
        }
    }

    /// Parse the CUE spelling of a prompt delivery mode.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "stdin" => Some(Self::Stdin),
            "arg" => Some(Self::Arg),
            _ => None,
        }
    }
}

/// Filesystem isolation mode for an agent attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IsolationMode {
    /// Run directly in the workflow directory.
    #[default]
    None,
    /// Run in a fresh git worktree for this attempt.
    Worktree,
}

impl IsolationMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Worktree => "worktree",
        }
    }

    /// Parse the CUE spelling of an isolation mode.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "worktree" => Some(Self::Worktree),
            _ => None,
        }
    }
}

/// Isolation policy for an agent attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct IsolationSpec {
    #[serde(default)]
    pub mode: IsolationMode,
    /// Git ref the worktree is created from. Defaults to `HEAD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// Keep the worktree after the attempt. Defaults to false.
    #[serde(default)]
    pub keep: bool,
}

impl IsolationSpec {
    #[must_use]
    pub fn is_worktree(&self) -> bool {
        self.mode == IsolationMode::Worktree
    }
}

/// A reusable agent definition declared under the top-level `agents` field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentDefinition {
    pub name: String,
    #[serde(default)]
    pub provider: AgentProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
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

/// One captured output file or patch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedArtifact {
    /// Repository/directory-relative path. Never absolute, never contains `..`.
    pub name: String,
    /// `declared`, `changed`, `created`, `deleted`, `renamed` or `patch`.
    pub kind: String,
    pub bytes: Vec<u8>,
    /// True when only a prefix of the file was stored.
    pub truncated: bool,
    /// Original size on disk before truncation.
    pub original_size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
}

impl CapturedArtifact {
    /// SHA-256 of the bytes that are actually stored.
    #[must_use]
    pub fn sha256(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(&self.bytes);
        hex::encode(hasher.finalize())
    }

    #[must_use]
    pub fn meta(&self) -> ArtifactMeta {
        ArtifactMeta {
            name: self.name.clone(),
            kind: self.kind.clone(),
            sha256: self.sha256(),
            size: self.original_size,
            stored_size: u64::try_from(self.bytes.len()).unwrap_or(u64::MAX),
            truncated: self.truncated,
            mode: self.mode,
        }
    }
}

/// SHA-256 helper for trace fields that must not store the original value.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Artifact metadata recorded inside an [`AgentTrace`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactMeta {
    pub name: String,
    pub kind: String,
    pub sha256: String,
    /// Original file size.
    pub size: u64,
    /// Number of bytes stored.
    pub stored_size: u64,
    pub truncated: bool,
    /// Unix permission bits, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
}

/// Complete, tamper-evident record of one agent attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTrace {
    pub run_id: String,
    pub step: String,
    pub round: u32,
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub prompt: String,
    /// SHA-256 of the exact prompt bytes written to the process.
    pub prompt_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub argv: Vec<String>,
    pub env_keys: Vec<String>,
    /// SHA-256 of each environment value, keyed by name. Values are never stored.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env_hashes: BTreeMap<String, String>,
    pub cwd: String,
    pub isolation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation_path: Option<String>,
    /// Resolved git commit the worktree was created from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation_base: Option<String>,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u64,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    pub ostinato_version: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub stdout: String,
    pub stderr: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactMeta>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capture_warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_error: Option<String>,
}

impl AgentTrace {
    /// Canonical SHA-256 over the trace (artifact content is hashed separately).
    #[must_use]
    pub fn trace_hash(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }
}

/// A persisted trace row plus its verification hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTraceRecord {
    pub id: i64,
    pub trace_hash: String,
    pub created_at: String,
    pub trace: AgentTrace,
}

/// A persisted artifact blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredArtifact {
    pub trace_id: i64,
    pub name: String,
    pub kind: String,
    pub sha256: String,
    pub size: u64,
    pub stored_size: u64,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
    pub content: Vec<u8>,
}

/// Result of re-hashing one trace and its artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceVerification {
    pub trace_id: i64,
    pub step: String,
    pub attempt: u32,
    pub ok: bool,
    pub problems: Vec<String>,
}
