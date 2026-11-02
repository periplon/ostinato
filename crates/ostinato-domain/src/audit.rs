use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::DomainError;
use crate::run::StepRecord;
use crate::workflow as wf;

/// The hash-chained audit event. Every run has its own chain starting at
/// [`GENESIS_HASH`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditEvent {
    pub run_id: String,
    pub seq: u64,
    pub ts: String,
    pub kind: String,
    pub step: Option<String>,
    pub data: Value,
    pub prev_hash: String,
    pub hash: String,
}

/// New event payload before the chain is sealed by the store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewEvent {
    pub kind: String,
    pub step: Option<String>,
    pub data: Value,
}

impl NewEvent {
    #[must_use]
    pub fn new(kind: impl Into<String>, step: Option<String>, data: Value) -> Self {
        Self {
            kind: kind.into(),
            step,
            data,
        }
    }
}

/// Genesis hash for a run's audit chain (64 zeroes).
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Seal an event into the hash chain.
///
/// # Errors
/// Fails if the payload cannot be serialised as canonical JSON.
pub fn seal(
    run_id: &str,
    seq: u64,
    ts: &str,
    kind: &str,
    step: Option<&str>,
    data: &Value,
    prev_hash: &str,
) -> Result<AuditEvent, DomainError> {
    let payload =
        serde_json::to_vec(data).map_err(|err| DomainError::Serialization(err.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(run_id.as_bytes());
    hasher.update(b"\x1f");
    hasher.update(seq.to_be_bytes());
    hasher.update(b"\x1f");
    hasher.update(ts.as_bytes());
    hasher.update(b"\x1f");
    hasher.update(kind.as_bytes());
    hasher.update(b"\x1f");
    hasher.update(step.unwrap_or("").as_bytes());
    hasher.update(b"\x1f");
    hasher.update(prev_hash.as_bytes());
    hasher.update(b"\x1f");
    hasher.update(&payload);
    let hash = hex::encode(hasher.finalize());

    Ok(AuditEvent {
        run_id: run_id.to_owned(),
        seq,
        ts: ts.to_owned(),
        kind: kind.to_owned(),
        step: step.map(ToOwned::to_owned),
        data: data.clone(),
        prev_hash: prev_hash.to_owned(),
        hash,
    })
}

/// Verify a full chain loaded in sequence order.
///
/// # Errors
/// Returns [`DomainError::InvalidAuditChain`] when the chain is broken or the
/// genesis/sequence invariants are violated.
pub fn verify_chain(events: &[AuditEvent]) -> Result<(), DomainError> {
    let mut prev = GENESIS_HASH.to_owned();
    for (offset, event) in events.iter().enumerate() {
        let expected_seq = u64::try_from(offset).unwrap_or(u64::MAX);
        if event.seq != expected_seq {
            return Err(DomainError::AuditSequence(event.seq));
        }
        if event.prev_hash != prev {
            return Err(DomainError::InvalidAuditChain {
                seq: event.seq,
                reason: "prev_hash does not match the previous event".to_owned(),
            });
        }
        let resealed = seal(
            &event.run_id,
            event.seq,
            &event.ts,
            &event.kind,
            event.step.as_deref(),
            &event.data,
            &event.prev_hash,
        )?;
        if resealed.hash != event.hash {
            return Err(DomainError::InvalidAuditChain {
                seq: event.seq,
                reason: "event hash does not match its content".to_owned(),
            });
        }
        prev.clone_from(&event.hash);
    }
    Ok(())
}

/// Digest of the execution-relevant state of a run.
///
/// The audit chain protects event history; this digest protects the mutable
/// rows that drive execution (`spec`, `params`, checkpoint and step outputs)
/// against edits that would otherwise be invisible to `audit verify`.
#[must_use]
pub fn state_digest(
    spec: &wf::WorkflowSpec,
    params: &Value,
    source_file: &str,
    resume_index: i64,
    steps: &[StepRecord],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(spec).unwrap_or_default());
    hasher.update(b"\x1f");
    hasher.update(serde_json::to_vec(params).unwrap_or_default());
    hasher.update(b"\x1f");
    hasher.update(source_file.as_bytes());
    hasher.update(b"\x1f");
    hasher.update(resume_index.to_be_bytes());
    let mut ordered: Vec<&StepRecord> = steps.iter().collect();
    ordered.sort_by(|a, b| a.name.cmp(&b.name));
    for step in ordered {
        hasher.update(b"\x1f");
        hasher.update(step.name.as_bytes());
        hasher.update(b"\x1f");
        hasher.update(step.status.as_str().as_bytes());
        hasher.update(b"\x1f");
        hasher.update(step.visits.to_be_bytes());
        hasher.update(b"\x1f");
        hasher.update(step.attempts.to_be_bytes());
        hasher.update(b"\x1f");
        hasher.update(step.error.as_deref().unwrap_or("").as_bytes());
        hasher.update(b"\x1f");
        hasher.update(
            step.output
                .as_ref()
                .map(|value| serde_json::to_vec(value).unwrap_or_default())
                .unwrap_or_default(),
        );
    }
    hex::encode(hasher.finalize())
}

/// The lifecycle status implied by the last lifecycle event in the chain.
///
/// Group events do not change this; only `run.started`, `run.resumed`,
/// `run.succeeded`, `run.failed` and `run.cancelled` do. It is used to detect
/// status-row edits that are not reflected in the tamper-evident history.
#[must_use]
pub fn lifecycle_status(events: &[AuditEvent]) -> Option<&'static str> {
    let mut status = None;
    for event in events {
        status = match event.kind.as_str() {
            "run.started" | "run.resumed" => Some("running"),
            "run.succeeded" => Some("succeeded"),
            "run.failed" => Some("failed"),
            "run.cancelled" => Some("cancelled"),
            _ => status,
        };
    }
    status
}
