use std::time::{Duration, Instant};

use ostinato_domain::{AgentProvider, Improvement, NewImprovement, Trial};
use serde_json::{Map, Value as Json};

use crate::error::EngineError;
use crate::ports::{AgentOutcome, AgentRequest, AgentRunner, Clock, PortError, Store};

/// Input for the recursive-improvement primitive.
#[derive(Debug, Clone)]
pub struct ImproveRequest {
    pub run_id: String,
    pub parent_id: Option<i64>,
    pub round: u32,
    /// Attempt number within the current retry loop (0-based).
    pub attempt: u32,
    pub prompt: Option<String>,
    pub max_samples: usize,
    pub command: Option<Vec<String>>,
    pub cwd: Option<std::path::PathBuf>,
    pub timeout: Option<Duration>,
}

/// One agent invocation for improvement, including the full outcome so the
/// engine can persist a trace even when the agent fails.
#[derive(Debug, Clone)]
pub struct ImprovementRun {
    pub improvement: Option<Improvement>,
    pub outcome: AgentOutcome,
    pub prompt: String,
    pub system: String,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u64,
}

/// Summarise trials, ask an agent for an improvement proposal, persist it.
///
/// The returned [`ImprovementRun`] always carries the agent outcome. A failed
/// agent yields `improvement: None`; the caller records the trace and decides
/// how to fail.
///
/// # Errors
/// Fails when the store cannot be read/written or the agent cannot be spawned.
pub fn propose_improvement(
    store: &dyn Store,
    agent: &dyn AgentRunner,
    clock: &dyn Clock,
    request: &ImproveRequest,
) -> Result<ImprovementRun, EngineError> {
    let trials = store.trials(&request.run_id).map_err(EngineError::from)?;
    let prompt = build_prompt(request, &trials);
    let system =
        "You are an expert workflow reliability engineer. Return strict JSON only.".to_owned();
    let started_at = clock.now();
    let started = Instant::now();
    let outcome = agent.run(&AgentRequest {
        run_id: request.run_id.clone(),
        step: "improve".to_owned(),
        definition: None,
        provider: AgentProvider::Command,
        prompt: prompt.clone(),
        system: Some(system.clone()),
        model: None,
        command: request.command.clone(),
        prompt_delivery: None,
        env: std::collections::BTreeMap::new(),
        timeout: Some(request.timeout.unwrap_or(Duration::from_secs(300))),
        cwd: request.cwd.clone(),
    })?;
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let finished_at = clock.now();
    if !outcome.success {
        return Ok(ImprovementRun {
            improvement: None,
            outcome,
            prompt,
            system,
            started_at,
            finished_at,
            duration_ms,
        });
    }
    let proposal = parse_proposal(&outcome.text);
    let diagnosis = proposal
        .get("diagnosis")
        .and_then(Json::as_str)
        .unwrap_or("agent did not provide a diagnosis")
        .to_owned();
    let new = NewImprovement {
        run_id: request.run_id.clone(),
        parent_id: request.parent_id,
        round: request.round,
        diagnosis: diagnosis.clone(),
        proposal: proposal.clone(),
        created_at: finished_at.clone(),
    };
    let id = store.record_improvement(&new).map_err(EngineError::from)?;
    Ok(ImprovementRun {
        improvement: Some(Improvement {
            id,
            run_id: new.run_id,
            parent_id: new.parent_id,
            round: new.round,
            diagnosis,
            proposal,
            created_at: new.created_at,
        }),
        outcome,
        prompt,
        system,
        started_at,
        finished_at,
        duration_ms,
    })
}

fn build_prompt(request: &ImproveRequest, trials: &[Trial]) -> String {
    let start = trials.len().saturating_sub(request.max_samples.max(1));
    let samples = &trials[start..];
    let mut out = String::new();
    out.push_str(
        "Improve the following workflow run. Analyse failures, retries and low scores.\n\
         Return strict JSON with this shape:\n\
         {\"diagnosis\": string, \"changes\": [string], \"params_patch\": object, \
          \"prompt_patch\": string, \"next_prompt\": string, \"confidence\": number}\n\n",
    );
    if let Some(extra) = &request.prompt {
        out.push_str("Additional instruction:\n");
        out.push_str(extra);
        out.push_str("\n\n");
    }
    out.push_str(&format!(
        "Run: {}\nRound: {}\n",
        request.run_id, request.round
    ));
    out.push_str("Trials (oldest to newest):\n");
    for trial in samples {
        out.push_str(&format!(
            "- step={} round={} attempt={} status={} score={:.3} durationMs={} error={}\n",
            trial.step,
            trial.round,
            trial.attempt,
            trial.status.as_str(),
            trial.score,
            trial.duration_ms,
            trial.error.as_deref().unwrap_or("")
        ));
        if let Some(output) = &trial.output {
            let text = output.to_string();
            out.push_str("  output=");
            out.push_str(&truncate(&text, 600));
            out.push('\n');
        }
    }
    out
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push_str("...");
    out
}

fn parse_proposal(text: &str) -> Json {
    let trimmed = text.trim();
    let candidate = if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            &trimmed[start..=end]
        } else {
            trimmed
        }
    } else {
        trimmed
    };
    if let Ok(value) = serde_json::from_str::<Json>(candidate)
        && value.is_object()
    {
        return value;
    }
    let mut fallback = Map::new();
    fallback.insert("diagnosis".to_owned(), Json::String(trimmed.to_owned()));
    fallback.insert("changes".to_owned(), Json::Array(Vec::new()));
    fallback.insert("params_patch".to_owned(), Json::Object(Map::new()));
    fallback.insert("raw".to_owned(), Json::String(trimmed.to_owned()));
    Json::Object(fallback)
}

/// Convert a proposal into a parameter patch suitable for a child run.
///
/// # Errors
/// Returns a configuration error if the patch is not an object.
pub fn params_patch(proposal: &Json) -> Result<Map<String, Json>, PortError> {
    match proposal.get("params_patch") {
        None => Ok(Map::new()),
        Some(Json::Object(map)) => Ok(map.clone()),
        Some(_) => Err(PortError::Config(
            "improvement `params_patch` must be an object".to_owned(),
        )),
    }
}
