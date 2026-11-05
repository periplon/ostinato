use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ostinato_domain::{
    ActivationScope, AgentSpec, AgentTrace, AuditEvent, CapturedArtifact, DEFAULT_AWAIT_TIMEOUT_MS,
    DomainError, EdgeOrigin, EdgeScope, EventSource, Improvement, IsolationMode, NewEvent,
    NewTrial, NewWorkflowEvent, Plan, PlanNode, RunRecord, RunReport, RunStatus, StepKind,
    StepRecord, StepSpec, StepStatus, TrialStatus, audit, build_plan, sha256_hex,
    validate_workflow_event,
};
use ostinato_lang::{eval_condition, render};
use serde_json::{Map, Value as Json, json};
use sha2::{Digest, Sha256};

use crate::activation::{self, Activation, ActivationError};
use crate::error::{EngineError, Result};
use crate::improve::{ImproveRequest, propose_improvement};
use crate::ports::{
    AgentOutcome, AgentRequest, AgentRunner, CaptureRequest, Clock, CommandExecutor, Completion,
    EventCursor, ExecRequest, IdGenerator, IsolationRequest, LoadedWorkflow, MetricsSummary,
    PortError, SpecLoader, Store, Telemetry, WorkflowEventQuery, WorkspaceProvider,
};

/// Dependencies injected into the engine (hexagonal ports).
#[derive(Clone)]
pub struct EngineDeps {
    pub store: Arc<dyn Store>,
    pub executor: Arc<dyn CommandExecutor>,
    pub agent: Arc<dyn AgentRunner>,
    pub workspace: Arc<dyn WorkspaceProvider>,
    pub clock: Arc<dyn Clock>,
    pub ids: Arc<dyn IdGenerator>,
    pub telemetry: Arc<dyn Telemetry>,
}

#[derive(Debug, Clone)]
pub struct EngineOptions {
    /// Override `workflow.maxParallel` from the CLI.
    pub max_parallel: Option<usize>,
    pub agent_timeout: Duration,
}

impl std::fmt::Debug for EngineDeps {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EngineDeps")
            .field("store", &"dyn Store")
            .field("executor", &"dyn CommandExecutor")
            .field("agent", &"dyn AgentRunner")
            .field("workspace", &"dyn WorkspaceProvider")
            .field("clock", &"dyn Clock")
            .field("ids", &"dyn IdGenerator")
            .field("telemetry", &"dyn Telemetry")
            .finish()
    }
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            max_parallel: None,
            agent_timeout: Duration::from_secs(300),
        }
    }
}

/// The workflow execution engine. All I/O goes through ports.
pub struct Engine {
    store: Arc<dyn Store>,
    executor: Arc<dyn CommandExecutor>,
    agent: Arc<dyn AgentRunner>,
    workspace: Arc<dyn WorkspaceProvider>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGenerator>,
    telemetry: Arc<dyn Telemetry>,
    options: EngineOptions,
}

struct NodeResult {
    index: usize,
    step: StepRecord,
    event: NewEvent,
    extra_events: Vec<NewEvent>,
    workflow_events: Vec<NewWorkflowEvent>,
    cursor_advances: Vec<EventCursor>,
    hard_failure: bool,
    continue_on_error: bool,
    skipped: bool,
    skip_reason: Option<String>,
    gateway_reason: Option<&'static str>,
}

struct GatewayFailure {
    step: String,
    error: String,
}

struct AttemptOutcome {
    success: bool,
    output: Json,
    input: Json,
    error: Option<String>,
    trial_status: TrialStatus,
    score: f64,
    event_effect: Option<Box<EventEffect>>,
}

#[derive(Default)]
struct EventEffect {
    extra_events: Vec<NewEvent>,
    cursor_advance: Option<EventCursor>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Engine")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl Engine {
    #[must_use]
    pub fn new(deps: EngineDeps, options: EngineOptions) -> Self {
        Self {
            store: deps.store,
            executor: deps.executor,
            agent: deps.agent,
            workspace: deps.workspace,
            clock: deps.clock,
            ids: deps.ids,
            telemetry: deps.telemetry,
            options,
        }
    }

    /// Start a new run from a loaded workflow.
    ///
    /// # Errors
    /// Fails when the store cannot create the run or execution fails.
    pub fn start(&self, loaded: &LoadedWorkflow) -> Result<RunReport> {
        self.start_with(
            loaded.source_file.clone(),
            loaded.spec.clone(),
            loaded.params.clone(),
            None,
        )
    }

    /// Start a run with an explicit parent (used by recursive improvement).
    ///
    /// # Errors
    /// Fails when the store cannot create the run or execution fails.
    pub fn start_with(
        &self,
        source_file: String,
        spec: ostinato_domain::WorkflowSpec,
        params: Json,
        parent_run_id: Option<String>,
    ) -> Result<RunReport> {
        let id = self.ids.new_run_id();
        let now = self.clock.now();
        let state_hash = audit::state_digest(&spec, &params, &source_file, 0, &[]);
        let mut run = RunRecord {
            id,
            workflow_name: spec.name.clone(),
            source_file,
            spec,
            params,
            status: RunStatus::Running,
            resume_index: 0,
            parent_run_id,
            error: None,
            created_at: now.clone(),
            updated_at: now.clone(),
            audit_head: audit::GENESIS_HASH.to_owned(),
            state_hash: state_hash.clone(),
        };
        let event = self.store.create_run_with_event(
            &run,
            NewEvent::new(
                "run.started",
                None,
                json!({
                    "workflow": run.workflow_name.clone(),
                    "params": run.params.clone(),
                    "stateHash": state_hash,
                }),
            ),
            &now,
        )?;
        run.audit_head.clone_from(&event.hash);
        self.telemetry.event(&event);
        self.drive(run)
    }

    /// Resume an interrupted or failed run from its durable checkpoint.
    ///
    /// # Errors
    /// Fails for unknown/cancelled runs or infrastructure errors.
    pub fn resume(&self, run_id: &str) -> Result<RunReport> {
        let snapshot = self
            .store
            .snapshot(run_id)?
            .ok_or_else(|| EngineError::RunNotFound(run_id.to_owned()))?;
        let mut run = snapshot.run;
        if run.status == RunStatus::Succeeded {
            return self.report(&run, Instant::now());
        }
        if run.status == RunStatus::Cancelled {
            return Err(EngineError::Cancelled(run.id));
        }
        let events = snapshot.events;
        audit::verify_chain(&events)?;
        if let Some(expected) = audit::lifecycle_status(&events)
            && run.status.as_str() != expected
        {
            return Err(EngineError::InvalidState(format!(
                "status row is `{}` but the audit chain ends in `{expected}` for run {}",
                run.status.as_str(),
                run.id
            )));
        }
        if events.iter().any(|event| event.kind == "run.cancelled") {
            return Err(EngineError::Cancelled(run.id));
        }
        let head = events.last().map_or_else(
            || audit::GENESIS_HASH.to_owned(),
            |event| event.hash.clone(),
        );
        if head != run.audit_head {
            return Err(EngineError::InvalidState(format!(
                "audit head mismatch for run {}; refusing to resume",
                run.id
            )));
        }
        let steps = snapshot.steps;
        let digest = audit::state_digest(
            &run.spec,
            &run.params,
            &run.source_file,
            run.resume_index,
            &steps,
        );
        if digest != run.state_hash {
            return Err(EngineError::InvalidState(format!(
                "state digest mismatch for run {}; refusing to resume a tampered or stale run",
                run.id
            )));
        }
        audit::verify_state_anchor(&events, &run.state_hash)?;
        let now = self.clock.now();
        run.status = RunStatus::Running;
        run.updated_at.clone_from(&now);
        let event = match self.store.resume_run(
            &run.id,
            run.resume_index,
            &now,
            NewEvent::new(
                "run.resumed",
                None,
                json!({"resumeIndex": run.resume_index}),
            ),
        ) {
            Ok(event) => event,
            Err(PortError::Cancelled(_)) => {
                let current = self
                    .store
                    .load_run(&run.id)?
                    .ok_or_else(|| EngineError::RunNotFound(run.id.clone()))?;
                return self.report(&current, Instant::now());
            }
            Err(err) => return Err(err.into()),
        };
        self.telemetry.event(&event);
        self.drive(run)
    }

    /// Mark a run as cancelled. The scheduler observes this between groups.
    ///
    /// # Errors
    /// Fails when the run does not exist or the store cannot commit.
    pub fn cancel(&self, run_id: &str) -> Result<AuditEvent> {
        let now = self.clock.now();
        let event = self.store.cancel(run_id, &now)?;
        self.telemetry.event(&event);
        Ok(event)
    }

    /// Ask the agent for a recursive-improvement proposal.
    ///
    /// A trace is persisted for every attempt, including failed ones.
    ///
    /// # Errors
    /// Fails when trials cannot be read, the trace cannot be stored, or the
    /// agent fails.
    pub fn improve(&self, request: &ImproveRequest) -> Result<Improvement> {
        let run = propose_improvement(
            self.store.as_ref(),
            self.agent.as_ref(),
            self.clock.as_ref(),
            request,
        )?;
        let outcome = &run.outcome;
        let trace = AgentTrace {
            run_id: request.run_id.clone(),
            step: "improve".to_owned(),
            round: request.round,
            attempt: request.attempt,
            definition: None,
            provider: outcome.provider.clone(),
            model: None,
            prompt: run.prompt.clone(),
            prompt_sha256: sha256_hex(run.prompt.as_bytes()),
            system: Some(run.system.clone()),
            argv: outcome.argv.clone(),
            env_keys: outcome.env_keys.clone(),
            env_hashes: outcome.env_hashes.clone(),
            cwd: request
                .cwd
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            isolation: "none".to_owned(),
            isolation_path: None,
            isolation_base: None,
            started_at: run.started_at.clone(),
            finished_at: run.finished_at.clone(),
            duration_ms: run.duration_ms,
            success: outcome.success,
            exit_code: outcome.exit_code,
            timed_out: outcome.timed_out,
            signal: outcome.signal,
            pid: outcome.pid,
            program: outcome.argv.first().cloned(),
            ostinato_version: env!("CARGO_PKG_VERSION").to_owned(),
            stdout_truncated: outcome.stdout_truncated,
            stderr_truncated: outcome.stderr_truncated,
            stdin_sha256: outcome.stdin_sha256.clone(),
            error: outcome.error.clone(),
            stdout: outcome.stdout.clone(),
            stderr: outcome.stderr.clone(),
            transcript: parse_transcript(&outcome.stdout),
            artifacts: Vec::new(),
            capture_warnings: Vec::new(),
            capture_error: None,
            cleanup_error: None,
        };
        match self.store.record_agent_trace(&trace, &[]) {
            Ok(_) => {}
            Err(PortError::Cancelled(_)) => {
                return Err(EngineError::Cancelled(request.run_id.clone()));
            }
            Err(err) => return Err(err.into()),
        }
        match run.improvement {
            Some(improvement) => Ok(improvement),
            None => Err(EngineError::Agent(
                outcome
                    .error
                    .clone()
                    .unwrap_or_else(|| "improvement agent failed".to_owned()),
            )),
        }
    }

    /// Metrics aggregated from the durable store.
    ///
    /// # Errors
    /// Fails when the store cannot answer the aggregation query.
    pub fn metrics(&self, run_id: Option<&str>) -> Result<MetricsSummary> {
        Ok(self.store.metrics(run_id)?)
    }

    #[must_use]
    pub fn store(&self) -> &dyn Store {
        self.store.as_ref()
    }

    #[must_use]
    pub fn clock(&self) -> &dyn Clock {
        self.clock.as_ref()
    }

    #[must_use]
    pub fn agent(&self) -> &dyn AgentRunner {
        self.agent.as_ref()
    }

    fn drive(&self, mut run: RunRecord) -> Result<RunReport> {
        let started = Instant::now();
        let plan = build_plan(&run.spec)?;
        let index = usize::try_from(run.resume_index).map_err(|_| {
            EngineError::InvalidState(format!(
                "negative resume index {} for run {}",
                run.resume_index, run.id
            ))
        })?;
        if index > plan.nodes.len() {
            return Err(EngineError::InvalidState(format!(
                "resume index {} is beyond the plan ({} nodes) for run {}",
                index,
                plan.nodes.len(),
                run.id
            )));
        }
        if index == plan.nodes.len() {
            return self.finish_success(run, plan.nodes.len(), started);
        }
        let mut index = index;

        let mut records: BTreeMap<String, StepRecord> = self
            .store
            .steps(&run.id)?
            .into_iter()
            .map(|step| (step.name.clone(), step))
            .collect();
        let mut outputs: BTreeMap<String, Json> = records
            .values()
            .filter_map(|step| {
                step.output
                    .clone()
                    .map(|output| (step.name.clone(), output))
            })
            .collect();

        while index < plan.nodes.len() {
            if let Some(current) = self.store.load_run(&run.id)?
                && current.status == RunStatus::Cancelled
            {
                run.status = RunStatus::Cancelled;
                run.updated_at = current.updated_at;
                return self.report(&run, started);
            }
            let group_id = plan.nodes[index].group;
            let mut end = index + 1;
            while end < plan.nodes.len() && plan.nodes[end].group == group_id {
                end += 1;
            }
            let group = plan.nodes[index..end].to_vec();
            let context = self.context(&run, group[0].round, &outputs, &records);
            let (mut results, to_execute, gateway_failure) =
                self.activate_group(&run, &group, &context, &mut records, &mut outputs, &plan)?;

            if let Some(failure) = gateway_failure {
                let now = self.clock.now();
                let resume_index = i64::try_from(group[0].index).unwrap_or(i64::MAX);
                let error = failure.error.clone();
                let mut group_events: Vec<NewEvent> = Vec::new();
                for result in &results {
                    group_events.push(result.event.clone());
                    group_events.extend(result.extra_events.iter().cloned());
                }
                group_events.push(NewEvent::new(
                    "run.failed",
                    Some(failure.step.clone()),
                    json!({"error": error, "resumeIndex": resume_index}),
                ));
                let completion = Completion {
                    run_id: run.id.clone(),
                    resume_index,
                    status: RunStatus::Failed,
                    error: Some(error.clone()),
                    updated_at: now.clone(),
                    steps: results.iter().map(|result| result.step.clone()).collect(),
                    audit_events: group_events,
                    events: results
                        .iter()
                        .flat_map(|result| result.workflow_events.iter().cloned())
                        .collect(),
                    cursor_advances: results
                        .iter()
                        .flat_map(|result| result.cursor_advances.iter().cloned())
                        .collect(),
                };
                let events = match self.store.complete(completion) {
                    Ok(events) => events,
                    Err(PortError::Cancelled(_)) => {
                        run.status = RunStatus::Cancelled;
                        run.updated_at = now;
                        return self.report(&run, started);
                    }
                    Err(err) => return Err(err.into()),
                };
                for event in &events {
                    self.telemetry.event(event);
                }
                self.emit_result_metrics(&results, &run.spec);
                run.status = RunStatus::Failed;
                run.error = Some(error);
                run.resume_index = resume_index;
                run.updated_at = now;
                return self.report(&run, started);
            }

            let executed = match self.execute_group(&run, &to_execute, &context) {
                Ok(executed) => executed,
                Err(EngineError::Cancelled(_)) => {
                    run.status = RunStatus::Cancelled;
                    run.updated_at = self.clock.now();
                    return self.report(&run, started);
                }
                Err(err) => return Err(err),
            };
            results.extend(executed);
            results.sort_by_key(|result| result.index);

            for result in &results {
                records.insert(result.step.name.clone(), result.step.clone());
                if result.skipped {
                    outputs.remove(&result.step.name);
                } else if result.step.status == StepStatus::Succeeded {
                    if let Some(output) = &result.step.output {
                        outputs.insert(result.step.name.clone(), output.clone());
                    }
                } else {
                    outputs.remove(&result.step.name);
                }
            }

            let aborting = results
                .iter()
                .any(|result| result.hard_failure && !result.continue_on_error);
            let first_error = results
                .iter()
                .find(|result| result.hard_failure && !result.continue_on_error)
                .or_else(|| results.iter().find(|result| result.hard_failure))
                .and_then(|result| result.step.error.clone());
            let mut resume_after_group = if aborting {
                i64::try_from(group[0].index).unwrap_or(i64::MAX)
            } else {
                i64::try_from(end).unwrap_or(i64::MAX)
            };
            let now = self.clock.now();
            let mut group_events: Vec<NewEvent> = Vec::new();
            for result in &results {
                group_events.push(result.event.clone());
                group_events.extend(result.extra_events.iter().cloned());
            }

            if aborting {
                group_events.push(NewEvent::new(
                    "run.failed",
                    results
                        .iter()
                        .find(|result| result.hard_failure && !result.continue_on_error)
                        .map(|result| result.step.name.clone()),
                    json!({"error": first_error, "resumeIndex": resume_after_group}),
                ));
            }

            let mut cycle_error: Option<String> = None;
            if !aborting {
                let mut round_checked = false;
                for node in &group {
                    if !node.is_round_end || round_checked {
                        continue;
                    }
                    round_checked = true;

                    if node.activation == ActivationScope::RoundCycle {
                        let Some(cycle) = plan
                            .cycle_info
                            .iter()
                            .find(|cycle| cycle.members.iter().any(|member| member == &node.step))
                        else {
                            return Err(EngineError::InvalidState(format!(
                                "no cycle metadata for round-cycled step `{}`",
                                node.step
                            )));
                        };
                        let next_entered =
                            activation::cycle_entered(cycle, &records, &run.spec, node.round + 1)
                                .map_err(|err| EngineError::InvalidState(err.to_string()))?;
                        if !next_entered {
                            let mut skip = end;
                            while skip < plan.nodes.len() && plan.nodes[skip].scc == node.scc {
                                skip += 1;
                            }
                            group_events.push(NewEvent::new(
                                "cycle.exited",
                                Some(node.step.clone()),
                                json!({
                                    "round": node.round,
                                    "reason": "dead_path",
                                    "resumeIndex": skip,
                                }),
                            ));
                            resume_after_group = i64::try_from(skip).unwrap_or(i64::MAX);
                            break;
                        }
                    }

                    let Some(until) = &node.cycle_until else {
                        if node.in_cycle && node.round + 1 >= node.cycle_max_rounds {
                            group_events.push(NewEvent::new(
                                "cycle.exhausted",
                                Some(node.step.clone()),
                                json!({
                                    "round": node.round,
                                    "maxRounds": node.cycle_max_rounds,
                                    "until": Json::Null,
                                }),
                            ));
                        }
                        continue;
                    };
                    let context = self.context(&run, node.round, &outputs, &records);
                    let done = match eval_condition(until, &context) {
                        Ok(done) => done,
                        Err(err) => {
                            cycle_error = Some(format!("invalid cycle condition `{until}`: {err}"));
                            resume_after_group = i64::try_from(group[0].index).unwrap_or(i64::MAX);
                            break;
                        }
                    };
                    if done {
                        let mut skip = end;
                        while skip < plan.nodes.len() && plan.nodes[skip].scc == node.scc {
                            skip += 1;
                        }
                        group_events.push(NewEvent::new(
                            "cycle.completed",
                            Some(node.step.clone()),
                            json!({
                                "round": node.round,
                                "until": until,
                                "resumeIndex": skip,
                            }),
                        ));
                        resume_after_group = i64::try_from(skip).unwrap_or(i64::MAX);
                        break;
                    }
                    if node.round + 1 >= node.cycle_max_rounds {
                        group_events.push(NewEvent::new(
                            "cycle.exhausted",
                            Some(node.step.clone()),
                            json!({
                                "round": node.round,
                                "maxRounds": node.cycle_max_rounds,
                                "until": until,
                            }),
                        ));
                    }
                }
            }

            let status = if aborting || cycle_error.is_some() {
                RunStatus::Failed
            } else {
                RunStatus::Running
            };
            let error = cycle_error.clone().or_else(|| first_error.clone());
            if let Some(message) = &cycle_error {
                group_events.push(NewEvent::new(
                    "run.failed",
                    group.first().map(|node| node.step.clone()),
                    json!({"error": message, "resumeIndex": resume_after_group}),
                ));
            }
            let completion = Completion {
                run_id: run.id.clone(),
                resume_index: resume_after_group,
                status,
                error: error.clone(),
                updated_at: now.clone(),
                steps: results.iter().map(|result| result.step.clone()).collect(),
                audit_events: group_events,
                events: results
                    .iter()
                    .flat_map(|result| result.workflow_events.iter().cloned())
                    .collect(),
                cursor_advances: results
                    .iter()
                    .flat_map(|result| result.cursor_advances.iter().cloned())
                    .collect(),
            };
            let events = match self.store.complete(completion) {
                Ok(events) => events,
                Err(PortError::Cancelled(_)) => {
                    run.status = RunStatus::Cancelled;
                    run.updated_at = now;
                    return self.report(&run, started);
                }
                Err(err) => return Err(err.into()),
            };
            for event in &events {
                self.telemetry.event(event);
            }
            self.emit_result_metrics(&results, &run.spec);

            if aborting || cycle_error.is_some() {
                run.status = RunStatus::Failed;
                run.error = error;
                run.resume_index = resume_after_group;
                run.updated_at = now;
                return self.report(&run, started);
            }

            index = usize::try_from(resume_after_group).unwrap_or(plan.nodes.len());
            run.resume_index = resume_after_group;
        }

        self.finish_success(run, plan.nodes.len(), started)
    }

    fn finish_success(
        &self,
        mut run: RunRecord,
        total_nodes: usize,
        started: Instant,
    ) -> Result<RunReport> {
        let now = self.clock.now();
        let completion = Completion {
            run_id: run.id.clone(),
            resume_index: i64::try_from(total_nodes).unwrap_or(i64::MAX),
            status: RunStatus::Succeeded,
            error: None,
            updated_at: now.clone(),
            steps: Vec::new(),
            audit_events: vec![NewEvent::new(
                "run.succeeded",
                None,
                json!({"resumeIndex": total_nodes}),
            )],
            events: Vec::new(),
            cursor_advances: Vec::new(),
        };
        let events = match self.store.complete(completion) {
            Ok(events) => events,
            Err(PortError::Cancelled(_)) => {
                run.status = RunStatus::Cancelled;
                return self.report(&run, started);
            }
            Err(err) => return Err(err.into()),
        };
        for event in &events {
            self.telemetry.event(event);
        }
        run.status = RunStatus::Succeeded;
        run.error = None;
        run.resume_index = i64::try_from(total_nodes).unwrap_or(i64::MAX);
        run.updated_at = now;
        self.report(&run, started)
    }

    fn report(&self, run: &RunRecord, started: Instant) -> Result<RunReport> {
        Ok(RunReport {
            run_id: run.id.clone(),
            workflow_name: run.workflow_name.clone(),
            status: run.status,
            resume_index: run.resume_index,
            error: run.error.clone(),
            steps: self.store.steps(&run.id)?,
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }

    fn context(
        &self,
        run: &RunRecord,
        round: u32,
        outputs: &BTreeMap<String, Json>,
        records: &BTreeMap<String, StepRecord>,
    ) -> Json {
        let mut steps = Map::new();
        for (name, value) in outputs {
            steps.insert(name.clone(), value.clone());
        }
        let mut step_status = Map::new();
        let mut step_round = Map::new();
        for (name, record) in records {
            step_status.insert(
                name.clone(),
                Json::String(record.status.as_str().to_owned()),
            );
            step_round.insert(name.clone(), Json::from(record.round));
        }
        let events = self.event_context(run);
        json!({
            "run_id": run.id,
            "workflow": run.workflow_name.clone(),
            "round": round,
            "params": run.params.clone(),
            "steps": steps,
            "stepStatus": step_status,
            "stepRound": step_round,
            "events": events,
        })
    }

    /// Latest durable event per name for template/condition context.
    fn event_context(&self, run: &RunRecord) -> Json {
        let mut events = Map::new();
        let Ok(stored) = self
            .store
            .workflow_events(&run.id, &WorkflowEventQuery::default())
        else {
            return Json::Object(events);
        };
        for event in stored {
            events.insert(
                event.name.clone(),
                json!({
                    "event": event.name,
                    "source": event.source.as_str(),
                    "seq": event.seq,
                    "payload": event.payload,
                    "emittedBy": event.emitted_by,
                    "ts": event.ts,
                    "external": event.external,
                }),
            );
        }
        Json::Object(events)
    }

    /// Pure activation pass over a group. Produces gateway/skip rows and
    /// returns the normal steps that still need execution.
    fn activate_group(
        &self,
        run: &RunRecord,
        group: &[PlanNode],
        context: &Json,
        records: &mut BTreeMap<String, StepRecord>,
        outputs: &mut BTreeMap<String, Json>,
        plan: &Plan,
    ) -> Result<(Vec<NodeResult>, Vec<PlanNode>, Option<GatewayFailure>)> {
        let mut results = Vec::new();
        let mut to_execute = Vec::new();

        for node in group {
            if let Some(record) = records.get(&node.step)
                && record.round == node.round
                && (node.gateway.is_some() || record.status == StepStatus::Skipped)
                && matches!(
                    record.status,
                    StepStatus::Succeeded | StepStatus::Skipped | StepStatus::Failed
                )
            {
                if record.status == StepStatus::Failed && node.gateway.is_some() {
                    return Ok((
                        results,
                        to_execute,
                        Some(GatewayFailure {
                            step: node.step.clone(),
                            error: record.error.clone().unwrap_or_default(),
                        }),
                    ));
                }
                // Reuse a committed decision/skip from an earlier attempt of
                // this group. No duplicate event or metric is emitted.
                continue;
            }
            let activation = if node.activation == ActivationScope::RoundCycle {
                let cycle = plan
                    .cycle_info
                    .iter()
                    .find(|cycle| cycle.members.iter().any(|member| member == &node.step))
                    .ok_or_else(|| {
                        EngineError::InvalidState(format!(
                            "no cycle metadata for round-cycled step `{}`",
                            node.step
                        ))
                    })?;
                let entered = activation::cycle_entered(cycle, records, &run.spec, node.round)
                    .map_err(|err| EngineError::InvalidState(err.to_string()))?;
                activation::decide_round(node, records, &run.spec, context, cycle, entered)
            } else {
                activation::decide_with_context(node, records, &run.spec, context)
            }
            .map_err(|err| EngineError::InvalidState(err.to_string()))?;
            match activation {
                Activation::Dead { reason } => {
                    if records.get(&node.step).is_some_and(|record| {
                        record.status == StepStatus::Skipped && record.round == node.round
                    }) {
                        // The row and its event were committed by a previous
                        // attempt of this group. Do not duplicate the audit
                        // event or the skip metric on resume.
                        continue;
                    }
                    let now = self.clock.now();
                    let record = StepRecord {
                        run_id: run.id.clone(),
                        name: node.step.clone(),
                        status: StepStatus::Skipped,
                        visits: 0,
                        attempts: 0,
                        output: None,
                        error: None,
                        updated_at: now,
                        round: node.round,
                    };
                    let mut data = json!({
                        "step": node.step,
                        "reason": reason.clone(),
                        "round": node.round,
                    });
                    if reason == "dead_path"
                        && let Some((gateway, branch)) =
                            dead_branch_source(node, records, node.round)
                    {
                        data["gateway"] = Json::String(gateway);
                        data["branch"] = Json::String(branch);
                    }
                    let event = NewEvent::new("step.skipped", Some(node.step.clone()), data);
                    records.insert(node.step.clone(), record.clone());
                    outputs.remove(&node.step);
                    results.push(NodeResult {
                        index: node.index,
                        step: record,
                        event,
                        extra_events: Vec::new(),
                        workflow_events: Vec::new(),
                        cursor_advances: Vec::new(),
                        hard_failure: false,
                        continue_on_error: false,
                        skipped: true,
                        skip_reason: Some(reason),
                        gateway_reason: None,
                    });
                }
                Activation::Fail { error } => {
                    let now = self.clock.now();
                    let reason = matches!(error, DomainError::GatewayQuorumNotMet { .. })
                        .then_some("quorum");
                    let message = error.to_string();
                    let record = StepRecord {
                        run_id: run.id.clone(),
                        name: node.step.clone(),
                        status: StepStatus::Failed,
                        visits: node.round + 1,
                        attempts: 0,
                        output: None,
                        error: Some(message.clone()),
                        updated_at: now,
                        round: node.round,
                    };
                    let mode = node.gateway.as_ref().map_or("gateway", |gateway| {
                        if gateway.split.is_some() {
                            "split"
                        } else {
                            "join"
                        }
                    });
                    let event = NewEvent::new(
                        "gateway.failed",
                        Some(node.step.clone()),
                        json!({
                            "gateway": node.step,
                            "mode": mode,
                            "error": message,
                            "reason": reason,
                            "round": node.round,
                        }),
                    );
                    records.insert(node.step.clone(), record.clone());
                    outputs.remove(&node.step);
                    results.push(NodeResult {
                        index: node.index,
                        step: record,
                        event,
                        extra_events: Vec::new(),
                        workflow_events: Vec::new(),
                        cursor_advances: Vec::new(),
                        hard_failure: true,
                        continue_on_error: false,
                        skipped: false,
                        skip_reason: None,
                        gateway_reason: reason,
                    });
                    return Ok((
                        results,
                        to_execute,
                        Some(GatewayFailure {
                            step: node.step.clone(),
                            error: message,
                        }),
                    ));
                }
                Activation::Active => {
                    let Some(gateway) = &node.gateway else {
                        to_execute.push(node.clone());
                        continue;
                    };
                    if gateway.split.is_some() {
                        let decision = match activation::evaluate_split(node, context) {
                            Ok(decision) => decision,
                            Err(ActivationError::Gateway { error, .. }) => {
                                let message = error.to_string();
                                let now = self.clock.now();
                                let record = StepRecord {
                                    run_id: run.id.clone(),
                                    name: node.step.clone(),
                                    status: StepStatus::Failed,
                                    visits: node.round + 1,
                                    attempts: 0,
                                    output: None,
                                    error: Some(message.clone()),
                                    updated_at: now,
                                    round: node.round,
                                };
                                let event = NewEvent::new(
                                    "gateway.failed",
                                    Some(node.step.clone()),
                                    json!({
                                        "gateway": node.step,
                                        "mode": "split",
                                        "error": message,
                                        "round": node.round,
                                    }),
                                );
                                records.insert(node.step.clone(), record.clone());
                                outputs.remove(&node.step);
                                results.push(NodeResult {
                                    index: node.index,
                                    step: record,
                                    event,
                                    extra_events: Vec::new(),
                                    workflow_events: Vec::new(),
                                    cursor_advances: Vec::new(),
                                    hard_failure: true,
                                    continue_on_error: false,
                                    skipped: false,
                                    skip_reason: None,
                                    gateway_reason: None,
                                });
                                return Ok((
                                    results,
                                    to_execute,
                                    Some(GatewayFailure {
                                        step: node.step.clone(),
                                        error: message,
                                    }),
                                ));
                            }
                            Err(err) => {
                                return Err(EngineError::InvalidState(err.to_string()));
                            }
                        };
                        let output = activation::split_output(gateway, &decision, node.round);
                        let event = NewEvent::new(
                            "gateway.evaluated",
                            Some(node.step.clone()),
                            json!({
                                "gateway": node.step,
                                "split": gateway.split.map(|mode| mode.as_str()),
                                "evaluated": output.get("evaluated").cloned().unwrap_or(Json::Null),
                                "chosen": decision.chosen,
                                "default": gateway.default,
                                "usedDefault": decision.used_default,
                                "round": node.round,
                            }),
                        );
                        let record = gateway_record(run, node, output, &self.clock.now());
                        records.insert(node.step.clone(), record.clone());
                        outputs.insert(
                            node.step.clone(),
                            record.output.clone().unwrap_or(Json::Null),
                        );
                        results.push(NodeResult {
                            index: node.index,
                            step: record,
                            event,
                            extra_events: Vec::new(),
                            workflow_events: Vec::new(),
                            cursor_advances: Vec::new(),
                            hard_failure: false,
                            continue_on_error: false,
                            skipped: false,
                            skip_reason: None,
                            gateway_reason: None,
                        });
                    } else if gateway.join.is_some() {
                        let cycle = plan
                            .cycle_info
                            .iter()
                            .find(|cycle| cycle.members.iter().any(|member| member == &node.step));
                        let edges = activation::incoming_edges_at(
                            node, records, &run.spec, node.round, cycle,
                        )
                        .map_err(|err| EngineError::InvalidState(err.to_string()))?;
                        let incoming: Vec<String> =
                            edges.iter().map(|(edge, _)| edge.from.clone()).collect();
                        let active: Vec<String> = edges
                            .iter()
                            .filter(|(_, state)| state.is_active())
                            .map(|(edge, _)| edge.from.clone())
                            .collect();
                        let dead: Vec<String> = edges
                            .iter()
                            .filter(|(_, state)| matches!(state, activation::EdgeState::Dead))
                            .map(|(edge, _)| edge.from.clone())
                            .collect();
                        let edge_values = gateway
                            .expression
                            .as_ref()
                            .map(|_| activation::expression_edges(gateway, &edges));
                        let output = activation::join_output(
                            gateway,
                            &incoming,
                            &active,
                            &dead,
                            "taken",
                            node.round,
                            edge_values.as_ref(),
                        );
                        let mut event_data = json!({
                            "gateway": node.step,
                            "join": gateway.join.map(|mode| mode.as_str()),
                            "incoming": incoming,
                            "active": active,
                            "dead": dead,
                            "decision": "taken",
                            "round": node.round,
                        });
                        if let Some(n) = gateway.n {
                            event_data["n"] = Json::from(n);
                            event_data["quorum"] = json!({
                                "required": n,
                                "active": output
                                    .get("quorum")
                                    .and_then(|quorum| quorum.get("active"))
                                    .cloned()
                                    .unwrap_or(Json::from(0)),
                                "met": output
                                    .get("quorum")
                                    .and_then(|quorum| quorum.get("met"))
                                    .cloned()
                                    .unwrap_or(Json::Bool(true)),
                            });
                        }
                        if let Some(edges) = &edge_values {
                            event_data["edges"] = edges.clone();
                        }
                        let event =
                            NewEvent::new("gateway.joined", Some(node.step.clone()), event_data);
                        let record = gateway_record(run, node, output, &self.clock.now());
                        records.insert(node.step.clone(), record.clone());
                        outputs.insert(
                            node.step.clone(),
                            record.output.clone().unwrap_or(Json::Null),
                        );
                        results.push(NodeResult {
                            index: node.index,
                            step: record,
                            event,
                            extra_events: Vec::new(),
                            workflow_events: Vec::new(),
                            cursor_advances: Vec::new(),
                            hard_failure: false,
                            continue_on_error: false,
                            skipped: false,
                            skip_reason: None,
                            gateway_reason: None,
                        });
                    } else {
                        return Err(EngineError::InvalidState(format!(
                            "gateway `{}` has neither split nor join",
                            node.step
                        )));
                    }
                }
            }
        }

        Ok((results, to_execute, None))
    }

    /// Emit telemetry after the group transaction has committed, so counters
    /// never include work that was rolled back or cancelled.
    fn emit_result_metrics(&self, results: &[NodeResult], spec: &ostinato_domain::WorkflowSpec) {
        for result in results {
            let name = &result.step.name;
            let gateway = spec.steps.get(name).and_then(|step| step.gateway.as_ref());
            if result.skipped {
                self.telemetry.metric(
                    "ostinato_step_skipped_total",
                    1.0,
                    &[
                        ("step", name),
                        (
                            "reason",
                            result.skip_reason.as_deref().unwrap_or("dead_path"),
                        ),
                    ],
                );
                if let Some(gateway) = gateway {
                    if let Some(mode) = gateway.join {
                        self.telemetry.metric(
                            "ostinato_gateway_join_total",
                            1.0,
                            &[
                                ("gateway", name),
                                ("mode", mode.as_str()),
                                ("decision", "dead"),
                            ],
                        );
                    }
                    if let Some(mode) = gateway.split {
                        for branch in gateway.branches.keys() {
                            self.telemetry.metric(
                                "ostinato_gateway_branch_total",
                                1.0,
                                &[
                                    ("gateway", name),
                                    ("mode", mode.as_str()),
                                    ("branch", branch),
                                    ("decision", "skipped"),
                                ],
                            );
                        }
                    }
                }
            } else if result.step.status == StepStatus::Succeeded {
                let step_spec = spec.steps.get(name);
                for emit in step_spec.map_or(&[][..], |step| step.emit.as_slice()) {
                    self.telemetry.metric(
                        "ostinato_workflow_events_emitted_total",
                        1.0,
                        &[("step", name), ("event", &emit.name)],
                    );
                    if emit.external {
                        self.telemetry.metric(
                            "ostinato_workflow_events_external_total",
                            1.0,
                            &[("step", name), ("event", &emit.name)],
                        );
                    }
                }
                if step_spec.is_some_and(|step| step.kind == StepKind::Await) {
                    self.telemetry.metric(
                        "ostinato_workflow_events_received_total",
                        1.0,
                        &[("step", name)],
                    );
                }
                if let Some(gateway) = gateway {
                    if let Some(mode) = gateway.split {
                        let chosen = result
                            .step
                            .output
                            .as_ref()
                            .and_then(|output| output.get("chosen"))
                            .and_then(Json::as_array);
                        for branch in gateway.branches.keys() {
                            let decision = if chosen.is_some_and(|chosen| {
                                chosen
                                    .iter()
                                    .any(|value| value.as_str() == Some(branch.as_str()))
                            }) {
                                "taken"
                            } else {
                                "skipped"
                            };
                            self.telemetry.metric(
                                "ostinato_gateway_branch_total",
                                1.0,
                                &[
                                    ("gateway", name),
                                    ("mode", mode.as_str()),
                                    ("branch", branch),
                                    ("decision", decision),
                                ],
                            );
                        }
                    }
                    if let Some(mode) = gateway.join {
                        self.telemetry.metric(
                            "ostinato_gateway_join_total",
                            1.0,
                            &[
                                ("gateway", name),
                                ("mode", mode.as_str()),
                                ("decision", "taken"),
                            ],
                        );
                    }
                } else {
                    self.telemetry
                        .metric("ostinato_step_success_total", 1.0, &[("step", name)]);
                }
            } else {
                if spec
                    .steps
                    .get(name)
                    .is_some_and(|step| step.kind == StepKind::Await)
                {
                    self.telemetry.metric(
                        "ostinato_workflow_events_timeout_total",
                        1.0,
                        &[("step", name)],
                    );
                }
                if let Some(gateway) = gateway {
                    if let Some(mode) = gateway.join {
                        self.telemetry.metric(
                            "ostinato_gateway_join_total",
                            1.0,
                            &[
                                ("gateway", name),
                                ("mode", mode.as_str()),
                                (
                                    "decision",
                                    match result.gateway_reason {
                                        Some("quorum") => "quorum_failed",
                                        Some(reason) => reason,
                                        None => "failed",
                                    },
                                ),
                            ],
                        );
                    }
                    self.telemetry.metric(
                        "ostinato_gateway_failure_total",
                        1.0,
                        &[("gateway", name)],
                    );
                } else {
                    self.telemetry
                        .metric("ostinato_step_failure_total", 1.0, &[("step", name)]);
                }
            }
        }
    }

    fn execute_group(
        &self,
        run: &RunRecord,
        group: &[PlanNode],
        context: &Json,
    ) -> Result<Vec<NodeResult>> {
        let max_parallel = self
            .options
            .max_parallel
            .unwrap_or(run.spec.max_parallel)
            .max(1);
        let mut results = Vec::with_capacity(group.len());
        // Run ordinary work first so a sleeping await never takes the only
        // runnable slot away from a step that could finish immediately.
        let (await_nodes, work_nodes): (Vec<PlanNode>, Vec<PlanNode>) = group
            .iter()
            .cloned()
            .partition(|node| run.spec.steps[&node.step].kind == StepKind::Await);
        for chunk in work_nodes.chunks(max_parallel) {
            let chunk_results: Vec<Result<NodeResult>> = std::thread::scope(|scope| {
                let mut handles = Vec::with_capacity(chunk.len());
                for node in chunk {
                    let node = node.clone();
                    handles.push(scope.spawn(move || self.execute_node(run, &node, context)));
                }
                handles
                    .into_iter()
                    .map(|handle| match handle.join() {
                        Ok(result) => result,
                        Err(_) => Err(EngineError::Internal(
                            "step worker thread panicked".to_owned(),
                        )),
                    })
                    .collect()
            });
            for result in chunk_results {
                results.push(result?);
            }
        }
        // Awaits are bounded separately by the same maxParallel limit. They
        // can run concurrently with each other but not with ordinary work.
        for chunk in await_nodes.chunks(max_parallel) {
            let chunk_results: Vec<Result<NodeResult>> = std::thread::scope(|scope| {
                let mut handles = Vec::with_capacity(chunk.len());
                for node in chunk {
                    let node = node.clone();
                    handles.push(scope.spawn(move || self.execute_node(run, &node, context)));
                }
                handles
                    .into_iter()
                    .map(|handle| match handle.join() {
                        Ok(result) => result,
                        Err(_) => Err(EngineError::Internal(
                            "step worker thread panicked".to_owned(),
                        )),
                    })
                    .collect()
            });
            for result in chunk_results {
                results.push(result?);
            }
        }
        Ok(results)
    }

    fn execute_node(&self, run: &RunRecord, node: &PlanNode, context: &Json) -> Result<NodeResult> {
        let step_spec = run
            .spec
            .steps
            .get(&node.step)
            .ok_or_else(|| EngineError::Internal(format!("unknown step `{}`", node.step)))?;
        let (mut outcome, attempts) = self.run_with_retries(run, node, step_spec, context)?;
        let success = outcome.success;
        let status = if success {
            StepStatus::Succeeded
        } else {
            StepStatus::Failed
        };
        let output_hash = hash_json(&outcome.output);
        let score = outcome.score;
        let updated_at = self.clock.now();
        let step = StepRecord {
            run_id: run.id.clone(),
            name: node.step.clone(),
            status,
            visits: if success { node.round + 1 } else { node.round },
            attempts,
            output: success.then_some(outcome.output.clone()),
            error: outcome.error.clone(),
            updated_at: updated_at.clone(),
            round: node.round,
        };
        let mut workflow_events = Vec::new();
        if success {
            for (index, emit) in step_spec.emit.iter().enumerate() {
                let payload = emit.payload.clone().unwrap_or(Json::Null);
                validate_workflow_event(&emit.name, &payload)?;
                let source = if emit.external {
                    EventSource::External
                } else {
                    EventSource::Internal
                };
                workflow_events.push(
                    NewWorkflowEvent::new(
                        run.id.clone(),
                        emit.name.clone(),
                        source,
                        payload,
                        Some(node.step.clone()),
                        updated_at.clone(),
                    )
                    .with_idempotency_key(format!(
                        "emit:{}:{}:{}:{}",
                        run.id, node.step, node.round, index
                    )),
                );
            }
        }
        let (extra_events, cursor_advances) = outcome.event_effect.take().map_or_else(
            || (Vec::new(), Vec::new()),
            |effect| {
                (
                    effect.extra_events,
                    effect.cursor_advance.into_iter().collect(),
                )
            },
        );
        let event = NewEvent::new(
            if success {
                "step.succeeded"
            } else {
                "step.failed"
            },
            Some(node.step.clone()),
            json!({
                "round": node.round,
                "attempts": attempts,
                "score": score,
                "outputHash": output_hash,
                "error": outcome.error,
            }),
        );
        Ok(NodeResult {
            index: node.index,
            step,
            event,
            extra_events,
            workflow_events,
            cursor_advances,
            hard_failure: !success,
            continue_on_error: step_spec.continue_on_error,
            skipped: false,
            skip_reason: None,
            gateway_reason: None,
        })
    }

    fn run_with_retries(
        &self,
        run: &RunRecord,
        node: &PlanNode,
        spec: &StepSpec,
        context: &Json,
    ) -> Result<(AttemptOutcome, u32)> {
        let max_attempts = spec.retry.max_retries.saturating_add(1);
        let seed = name_seed(&node.step);
        let mut last: Option<AttemptOutcome> = None;
        for attempt in 0..max_attempts {
            let started = Instant::now();
            let outcome = self.execute_attempt(run, node, spec, context, attempt)?;
            let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let trial = NewTrial {
                run_id: run.id.clone(),
                node_index: i64::try_from(node.index).unwrap_or(i64::MAX),
                step: node.step.clone(),
                round: node.round,
                attempt,
                status: outcome.trial_status,
                score: outcome.score,
                duration_ms,
                input_hash: hash_json(&outcome.input),
                output_hash: hash_json(&outcome.output),
                input: outcome.input.clone(),
                output: Some(outcome.output.clone()),
                error: outcome.error.clone(),
                created_at: self.clock.now(),
            };
            self.store.record_trial(&trial)?;
            self.telemetry.metric(
                "ostinato_attempt_duration_ms",
                duration_ms as f64,
                &[("step", &node.step)],
            );
            if outcome.success {
                return Ok((outcome, attempt + 1));
            }
            if attempt + 1 < max_attempts {
                let delay = spec.retry.delay_for(attempt, seed);
                self.clock.sleep(delay);
                self.telemetry
                    .metric("ostinato_retry_total", 1.0, &[("step", &node.step)]);
            }
            last = Some(outcome);
        }
        match last {
            Some(outcome) => Ok((outcome, max_attempts)),
            None => Err(EngineError::Internal(
                "retry loop produced no attempt".to_owned(),
            )),
        }
    }

    fn execute_attempt(
        &self,
        run: &RunRecord,
        node: &PlanNode,
        spec: &StepSpec,
        context: &Json,
        attempt: u32,
    ) -> Result<AttemptOutcome> {
        match spec.kind {
            StepKind::Noop => Ok(AttemptOutcome {
                success: true,
                output: json!({"text": "", "score": 1.0, "exitCode": 0}),
                input: json!({"kind": "noop"}),
                error: None,
                trial_status: TrialStatus::Succeeded,
                score: 1.0,
                event_effect: None,
            }),
            StepKind::Exec => self.execute_exec(run, node, spec, context),
            StepKind::Agent => self.execute_agent(run, node, spec, context, attempt),
            StepKind::Improve => self.execute_improve(run, node, spec, attempt),
            StepKind::Await => self.execute_await(run, node, spec),
            StepKind::Gateway => Err(EngineError::Internal(format!(
                "gateway `{}` reached the execution pass; gateways must be evaluated during activation",
                node.step
            ))),
        }
    }

    fn execute_await(
        &self,
        run: &RunRecord,
        node: &PlanNode,
        spec: &StepSpec,
    ) -> Result<AttemptOutcome> {
        let Some(await_spec) = &spec.await_spec else {
            return Err(EngineError::InvalidState(format!(
                "step `{}` is await but has no await spec",
                node.step
            )));
        };
        let name = await_spec.event.clone();
        let source = await_spec.source();
        let timeout_ms = await_spec
            .timeout_ms
            .or(spec.timeout_ms)
            .unwrap_or(DEFAULT_AWAIT_TIMEOUT_MS);
        let timeout = Duration::from_millis(timeout_ms);
        let deadline = Instant::now() + timeout;
        let input = json!({
            "kind": "await",
            "event": name,
            "source": source.as_str(),
            "timeoutMs": timeout_ms,
        });
        let cursor = self
            .store
            .event_cursor(&run.id, &node.step, &name)?
            .map_or(0, |cursor| cursor.next_seq);
        let mut interval = Duration::from_millis(10);
        loop {
            if let Some(current) = self.store.load_run(&run.id)?
                && current.status == RunStatus::Cancelled
            {
                return Err(EngineError::Cancelled(run.id.clone()));
            }
            let query = WorkflowEventQuery {
                name: Some(name.clone()),
                source: Some(source),
                // `next_seq` is inclusive; the store filter is exclusive.
                after_seq: cursor.checked_sub(1),
            };
            match self.store.workflow_events(&run.id, &query) {
                Ok(events) => {
                    if let Some(event) = events.first() {
                        let payload = event.payload.clone();
                        let text = payload
                            .as_str()
                            .map_or_else(|| payload.to_string(), ToOwned::to_owned);
                        let event_name = event.name.clone();
                        let emitted_by = event.emitted_by.clone();
                        let source_name = event.source.as_str();
                        let output = json!({
                            "text": text.clone(),
                            "stdout": text,
                            "event": event_name.clone(),
                            "source": source_name,
                            "payload": payload.clone(),
                            "json": payload.clone(),
                            "seq": event.seq,
                            "emittedBy": emitted_by.clone(),
                            "ts": event.ts.clone(),
                            "exitCode": 0,
                            "score": 1.0,
                        });
                        let audit = NewEvent::new(
                            "event.received",
                            Some(node.step.clone()),
                            json!({
                                "event": event_name,
                                "source": source_name,
                                "seq": event.seq,
                                "emittedBy": emitted_by,
                                "payload": payload,
                            }),
                        );
                        return Ok(AttemptOutcome {
                            success: true,
                            output,
                            input,
                            error: None,
                            trial_status: TrialStatus::Succeeded,
                            score: 1.0,
                            event_effect: Some(Box::new(EventEffect {
                                extra_events: vec![audit],
                                cursor_advance: Some(EventCursor {
                                    run_id: run.id.clone(),
                                    consumer: node.step.clone(),
                                    event: name,
                                    next_seq: event.seq.saturating_add(1),
                                }),
                            })),
                        });
                    }
                }
                Err(PortError::Corrupt(message)) => {
                    let error = format!("event payload for `{name}` is malformed: {message}");
                    let audit = NewEvent::new(
                        "event.malformed",
                        Some(node.step.clone()),
                        json!({"event": name, "error": message}),
                    );
                    return Ok(AttemptOutcome {
                        success: false,
                        output: json!({"error": error, "score": 0.0}),
                        input,
                        error: Some(error),
                        trial_status: TrialStatus::Failed,
                        score: 0.0,
                        event_effect: Some(Box::new(EventEffect {
                            extra_events: vec![audit],
                            cursor_advance: None,
                        })),
                    });
                }
                Err(err) => return Err(err.into()),
            }

            let now = Instant::now();
            if now >= deadline {
                let error = format!("timed out waiting for event `{name}` after {timeout_ms} ms");
                let audit = NewEvent::new(
                    "event.timeout",
                    Some(node.step.clone()),
                    json!({
                        "event": name,
                        "source": source.as_str(),
                        "timeoutMs": timeout_ms,
                    }),
                );
                return Ok(AttemptOutcome {
                    success: false,
                    output: json!({"error": error, "score": 0.0}),
                    input,
                    error: Some(error),
                    trial_status: TrialStatus::TimedOut,
                    score: 0.0,
                    event_effect: Some(Box::new(EventEffect {
                        extra_events: vec![audit],
                        cursor_advance: None,
                    })),
                });
            }
            let remaining = deadline.duration_since(now);
            self.clock.sleep(interval.min(remaining));
            interval = (interval * 2).min(Duration::from_millis(250));
        }
    }

    fn execute_exec(
        &self,
        run: &RunRecord,
        node: &PlanNode,
        spec: &StepSpec,
        context: &Json,
    ) -> Result<AttemptOutcome> {
        let Some(exec) = &spec.exec else {
            return Err(EngineError::InvalidState(format!(
                "step `{}` is exec but has no exec spec",
                node.step
            )));
        };
        let mut rendered: Vec<String> = Vec::with_capacity(exec.cmd.len());
        for argument in &exec.cmd {
            match render(argument, context) {
                Ok(argument) => rendered.push(argument),
                Err(err) => {
                    return Ok(failure(
                        json!({"kind": "exec", "renderError": err.to_string()}),
                        err.to_string(),
                        0.0,
                    ));
                }
            }
        }
        let Some(program) = rendered.first().cloned() else {
            return Err(EngineError::InvalidState(format!(
                "step `{}` has an empty command",
                node.step
            )));
        };
        let args = rendered.into_iter().skip(1).collect();
        let cwd = match &exec.cwd {
            Some(path) => match render(path, context) {
                Ok(path) => Some(PathBuf::from(path)),
                Err(err) => {
                    return Ok(failure(
                        json!({"kind": "exec", "renderError": err.to_string()}),
                        err.to_string(),
                        0.0,
                    ));
                }
            },
            None => workflow_dir(run),
        };
        let mut env = BTreeMap::new();
        for (key, value) in &exec.env {
            match render(value, context) {
                Ok(value) => {
                    env.insert(key.clone(), value);
                }
                Err(err) => {
                    return Ok(failure(
                        json!({"kind": "exec", "renderError": err.to_string()}),
                        err.to_string(),
                        0.0,
                    ));
                }
            }
        }
        let env_keys: Vec<&String> = env.keys().collect();
        let input = json!({
            "kind": "exec",
            "program": program,
            "args": args,
            "cwd": cwd.clone(),
            // Never persist rendered environment *values* in the trial log.
            "envKeys": env_keys,
        });
        let timeout = spec
            .timeout_ms
            .or(exec.timeout_ms)
            .map(Duration::from_millis)
            .or_else(|| Some(Duration::from_secs(600)));
        match self.executor.execute(&ExecRequest {
            step: node.step.clone(),
            program,
            args,
            cwd,
            env,
            timeout,
        }) {
            Ok(outcome) => {
                let success = outcome.succeeded();
                let score = extract_score_from_text(&outcome.stdout).unwrap_or(if success {
                    1.0
                } else {
                    0.0
                });
                let mut output = Map::new();
                output.insert("stdout".to_owned(), Json::String(outcome.stdout.clone()));
                output.insert("stderr".to_owned(), Json::String(outcome.stderr.clone()));
                output.insert("text".to_owned(), Json::String(outcome.stdout.clone()));
                output.insert("exitCode".to_owned(), json!(outcome.exit_code));
                output.insert("score".to_owned(), json!(score));
                if let Some(parsed) = parse_json_maybe(&outcome.stdout) {
                    output.insert("json".to_owned(), parsed);
                }
                let error = if success {
                    None
                } else if outcome.timed_out {
                    Some("command timed out".to_owned())
                } else {
                    Some(format!(
                        "command exited with {}: {}",
                        outcome.exit_code,
                        truncate(outcome.stderr.trim(), 500)
                    ))
                };
                Ok(AttemptOutcome {
                    success,
                    output: Json::Object(output),
                    input,
                    error,
                    trial_status: if success {
                        TrialStatus::Succeeded
                    } else if outcome.timed_out {
                        TrialStatus::TimedOut
                    } else {
                        TrialStatus::Failed
                    },
                    score,
                    event_effect: None,
                })
            }
            Err(err) => Ok(failure(input, err.to_string(), 0.0)),
        }
    }

    fn execute_agent(
        &self,
        run: &RunRecord,
        node: &PlanNode,
        spec: &StepSpec,
        context: &Json,
        attempt: u32,
    ) -> Result<AttemptOutcome> {
        let Some(agent) = &spec.agent else {
            return Err(EngineError::InvalidState(format!(
                "step `{}` is agent but has no agent spec",
                node.step
            )));
        };
        let render_or_fail = |value: &str| -> std::result::Result<String, AttemptOutcome> {
            render(value, context).map_err(|err| {
                failure(
                    json!({"kind": "agent", "renderError": err.to_string()}),
                    err.to_string(),
                    0.0,
                )
            })
        };
        let prompt = match render_or_fail(&agent.prompt) {
            Ok(prompt) => prompt,
            Err(failed) => return Ok(failed),
        };
        let system = match &agent.system {
            Some(system) => match render_or_fail(system) {
                Ok(system) => Some(system),
                Err(failed) => return Ok(failed),
            },
            None => None,
        };
        let command = match &agent.command {
            Some(command) => {
                let mut rendered = Vec::with_capacity(command.len());
                for argument in command {
                    match render_or_fail(argument) {
                        Ok(argument) => rendered.push(argument),
                        Err(failed) => return Ok(failed),
                    }
                }
                Some(rendered)
            }
            None => None,
        };
        let mut env = BTreeMap::new();
        for (key, value) in &agent.env {
            match render(value, context) {
                Ok(value) => {
                    env.insert(key.clone(), value);
                }
                Err(err) => {
                    return Ok(failure(
                        json!({"kind": "agent", "renderError": err.to_string()}),
                        err.to_string(),
                        0.0,
                    ));
                }
            }
        }
        let isolation = agent.isolation.clone().unwrap_or_default();
        let base_dir = workflow_dir(run);
        let input = json!({
            "kind": "agent",
            "prompt": prompt.clone(),
            "system": system.clone(),
            "model": agent.model,
            "run": run.id.clone(),
            "provider": agent.provider.as_str(),
            "definition": agent.definition,
            "isolation": isolation.mode.as_str(),
            "artifacts": agent.artifacts,
            "envKeys": env.keys().collect::<Vec<&String>>(),
        });
        let started_at = self.clock.now();
        let started = Instant::now();
        let prepared = self.workspace.prepare(&IsolationRequest {
            run_id: run.id.clone(),
            step: node.step.clone(),
            round: node.round,
            attempt,
            mode: isolation.mode,
            base: isolation.base.clone(),
            keep: isolation.keep,
            base_dir: base_dir.clone(),
        });
        let (handle, mut preparation_error) = match prepared {
            Ok(handle) => (Some(handle), None),
            Err(err) => (None, Some(err.to_string())),
        };
        let mut outcome = None;
        if let Some(handle) = &handle {
            match self.agent.run(&AgentRequest {
                run_id: run.id.clone(),
                step: node.step.clone(),
                definition: agent.definition.clone(),
                provider: agent.provider,
                prompt,
                system,
                model: agent.model.clone(),
                command,
                prompt_delivery: agent.prompt_delivery,
                env,
                timeout: Some(
                    spec.timeout_ms
                        .or(agent.timeout_ms)
                        .map(Duration::from_millis)
                        .unwrap_or(self.options.agent_timeout),
                ),
                cwd: Some(handle.dir.clone()),
            }) {
                Ok(value) => outcome = Some(value),
                Err(err) => preparation_error = Some(err.to_string()),
            }
        }
        let mut capture_warnings = Vec::new();
        let mut capture_error = None;
        let mut artifacts: Vec<CapturedArtifact> = Vec::new();
        if let Some(handle) = &handle {
            match self.workspace.capture(&CaptureRequest {
                cwd: handle.dir.clone(),
                patterns: agent.artifacts.clone(),
                base: (handle.mode == IsolationMode::Worktree)
                    .then(|| handle.base.clone())
                    .flatten(),
            }) {
                Ok(captured) => {
                    artifacts = captured.artifacts;
                    capture_warnings = captured.warnings;
                }
                Err(err) => capture_error = Some(err.to_string()),
            }
        }
        let mut cleanup_error = None;
        if let Some(handle) = &handle
            && let Err(err) = self.workspace.finish(handle)
        {
            cleanup_error = Some(err.to_string());
        }
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let finished_at = self.clock.now();

        let outcome = outcome.unwrap_or_else(|| empty_agent_outcome(agent));
        let combined_error = join_errors([
            preparation_error.clone(),
            outcome.error.clone(),
            capture_error.clone(),
        ]);
        let success = preparation_error.is_none() && outcome.success && capture_error.is_none();
        let cwd = handle
            .as_ref()
            .map(|handle| handle.dir.display().to_string())
            .or_else(|| base_dir.as_ref().map(|path| path.display().to_string()))
            .unwrap_or_default();
        let program = outcome.argv.first().cloned();
        let transcript = parse_transcript(&outcome.stdout);
        let trace = AgentTrace {
            run_id: run.id.clone(),
            step: node.step.clone(),
            round: node.round,
            attempt,
            definition: agent.definition.clone(),
            provider: agent.provider.as_str().to_owned(),
            model: agent.model.clone(),
            prompt: input
                .get("prompt")
                .and_then(Json::as_str)
                .unwrap_or_default()
                .to_owned(),
            prompt_sha256: sha256_hex(
                input
                    .get("prompt")
                    .and_then(Json::as_str)
                    .unwrap_or_default()
                    .as_bytes(),
            ),
            system: agent.system.clone().and_then(|_| {
                input
                    .get("system")
                    .and_then(Json::as_str)
                    .map(ToOwned::to_owned)
            }),
            argv: outcome.argv.clone(),
            env_keys: outcome.env_keys.clone(),
            env_hashes: outcome.env_hashes.clone(),
            cwd,
            isolation: handle.as_ref().map_or_else(
                || isolation.mode.as_str().to_owned(),
                |handle| handle.mode.as_str().to_owned(),
            ),
            isolation_path: handle
                .as_ref()
                .and_then(|handle| handle.path.as_ref().map(|path| path.display().to_string())),
            isolation_base: handle.as_ref().and_then(|handle| handle.base.clone()),
            started_at,
            finished_at,
            duration_ms,
            success,
            exit_code: outcome.exit_code,
            timed_out: outcome.timed_out,
            signal: outcome.signal,
            pid: outcome.pid,
            program: program.clone(),
            ostinato_version: env!("CARGO_PKG_VERSION").to_owned(),
            stdout_truncated: outcome.stdout_truncated,
            stderr_truncated: outcome.stderr_truncated,
            stdin_sha256: outcome.stdin_sha256.clone(),
            error: combined_error.clone(),
            stdout: outcome.stdout.clone(),
            stderr: outcome.stderr.clone(),
            transcript: transcript.clone(),
            artifacts: artifacts.iter().map(CapturedArtifact::meta).collect(),
            capture_warnings,
            capture_error,
            cleanup_error,
        };
        let record = match self.store.record_agent_trace(&trace, &artifacts) {
            Ok(record) => record,
            Err(PortError::Cancelled(_)) => {
                return Err(EngineError::Cancelled(run.id.clone()));
            }
            Err(err) => return Err(err.into()),
        };
        let trace_hash = record.trace_hash.clone();

        let parsed = parse_json_maybe(&outcome.stdout);
        let score = parsed
            .as_ref()
            .and_then(extract_score_from_json)
            .or_else(|| transcript_score(&outcome.stdout))
            .unwrap_or(0.0);
        let output = agent_output(&trace, &record, &trace_hash, &outcome, parsed, score);
        if success {
            Ok(AttemptOutcome {
                success: true,
                output,
                input,
                error: None,
                trial_status: TrialStatus::Succeeded,
                score,
                event_effect: None,
            })
        } else {
            Ok(failure_output(
                input,
                output,
                combined_error.unwrap_or_else(|| "agent attempt failed".to_owned()),
                score,
                if outcome.timed_out {
                    TrialStatus::TimedOut
                } else {
                    TrialStatus::Failed
                },
            ))
        }
    }

    fn execute_improve(
        &self,
        run: &RunRecord,
        node: &PlanNode,
        spec: &StepSpec,
        attempt: u32,
    ) -> Result<AttemptOutcome> {
        let Some(improve) = &spec.improve else {
            return Err(EngineError::InvalidState(format!(
                "step `{}` is improve but has no improve spec",
                node.step
            )));
        };
        let input = json!({
            "kind": "improve",
            "prompt": improve.prompt,
            "maxSamples": improve.max_samples,
        });
        match self.improve(&ImproveRequest {
            run_id: run.id.clone(),
            parent_id: None,
            round: node.round,
            attempt,
            prompt: improve.prompt.clone(),
            max_samples: improve.max_samples,
            command: None,
            cwd: workflow_dir(run),
            timeout: spec.timeout_ms.map(Duration::from_millis),
        }) {
            Ok(improvement) => {
                let score = extract_score_from_json(&improvement.proposal).unwrap_or(0.0);
                let mut output = Map::new();
                output.insert(
                    "text".to_owned(),
                    Json::String(improvement.proposal.to_string()),
                );
                output.insert("json".to_owned(), improvement.proposal.clone());
                output.insert("score".to_owned(), json!(score));
                output.insert("improvementId".to_owned(), json!(improvement.id));
                Ok(AttemptOutcome {
                    success: true,
                    output: Json::Object(output),
                    input,
                    error: None,
                    trial_status: TrialStatus::Succeeded,
                    score,
                    event_effect: None,
                })
            }
            Err(err) => Ok(failure(input, err.to_string(), 0.0)),
        }
    }
}

fn dead_branch_source(
    node: &PlanNode,
    records: &BTreeMap<String, StepRecord>,
    round: u32,
) -> Option<(String, String)> {
    for edge in &node.incoming {
        if edge.scope != EdgeScope::SameRound {
            continue;
        }
        let EdgeOrigin::Branch { gateway, branch } = &edge.origin else {
            continue;
        };
        let dead = records.get(&edge.from).is_none_or(|record| {
            record.round == round
                && (record.status == StepStatus::Skipped
                    || (record.status == StepStatus::Succeeded
                        && record
                            .output
                            .as_ref()
                            .and_then(|output| output.get("chosen"))
                            .and_then(Json::as_array)
                            .is_none_or(|chosen| {
                                !chosen
                                    .iter()
                                    .any(|value| value.as_str() == Some(branch.as_str()))
                            })))
        });
        if dead {
            return Some((gateway.clone(), branch.clone()));
        }
    }
    None
}

fn gateway_record(run: &RunRecord, node: &PlanNode, output: Json, now: &str) -> StepRecord {
    StepRecord {
        run_id: run.id.clone(),
        name: node.step.clone(),
        status: StepStatus::Succeeded,
        visits: node.round + 1,
        attempts: 0,
        output: Some(output),
        error: None,
        updated_at: now.to_owned(),
        round: node.round,
    }
}

fn workflow_dir(run: &RunRecord) -> Option<PathBuf> {
    let path = Path::new(&run.source_file);
    if path.is_dir() {
        return Some(path.to_path_buf());
    }
    path.parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
}

fn failure(input: Json, error: String, score: f64) -> AttemptOutcome {
    failure_output(
        input,
        json!({"error": error, "score": score}),
        error,
        score,
        TrialStatus::Failed,
    )
}

fn failure_output(
    input: Json,
    output: Json,
    error: String,
    score: f64,
    status: TrialStatus,
) -> AttemptOutcome {
    AttemptOutcome {
        success: false,
        output,
        input,
        error: Some(error),
        trial_status: status,
        score,
        event_effect: None,
    }
}

fn join_errors(errors: [Option<String>; 3]) -> Option<String> {
    let messages: Vec<String> = errors.into_iter().flatten().collect();
    if messages.is_empty() {
        None
    } else {
        Some(messages.join("; "))
    }
}

fn empty_agent_outcome(agent: &AgentSpec) -> AgentOutcome {
    AgentOutcome {
        text: String::new(),
        stdout: String::new(),
        stderr: String::new(),
        provider: agent.provider.as_str().to_owned(),
        argv: Vec::new(),
        exit_code: None,
        timed_out: false,
        success: false,
        error: None,
        signal: None,
        pid: None,
        env_keys: Vec::new(),
        env_hashes: BTreeMap::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        stdin_sha256: None,
    }
}

fn agent_output(
    trace: &AgentTrace,
    record: &ostinato_domain::AgentTraceRecord,
    trace_hash: &str,
    outcome: &AgentOutcome,
    parsed: Option<Json>,
    score: f64,
) -> Json {
    let mut output = Map::new();
    output.insert("kind".to_owned(), json!("agent"));
    output.insert("text".to_owned(), Json::String(outcome.stdout.clone()));
    output.insert("stdout".to_owned(), Json::String(outcome.stdout.clone()));
    output.insert("stderr".to_owned(), Json::String(outcome.stderr.clone()));
    output.insert("score".to_owned(), json!(score));
    output.insert("exitCode".to_owned(), json!(trace.exit_code.unwrap_or(-1)));
    // Backwards compatibility: this flat field has always been the executable.
    output.insert(
        "provider".to_owned(),
        Json::String(
            trace
                .program
                .clone()
                .unwrap_or_else(|| trace.provider.clone()),
        ),
    );
    output.insert("program".to_owned(), json!(trace.program));
    output.insert("definition".to_owned(), json!(trace.definition));
    output.insert(
        "isolation".to_owned(),
        Json::String(trace.isolation.clone()),
    );
    output.insert("traceHash".to_owned(), Json::String(trace_hash.to_owned()));
    output.insert(
        "agent".to_owned(),
        json!({
            "traceId": record.id,
            "traceHash": trace_hash,
            "provider": trace.provider,
            "program": trace.program,
            "model": trace.model,
            "definition": trace.definition,
            "isolation": trace.isolation,
            "isolationPath": trace.isolation_path,
            "isolationBase": trace.isolation_base,
            "promptSha256": trace.prompt_sha256,
            "stdinSha256": trace.stdin_sha256,
            "envKeys": trace.env_keys,
            "startedAt": trace.started_at,
            "finishedAt": trace.finished_at,
            "durationMs": trace.duration_ms,
            "exitCode": trace.exit_code,
            "signal": trace.signal,
            "pid": trace.pid,
            "timedOut": trace.timed_out,
            "stdoutTruncated": trace.stdout_truncated,
            "stderrTruncated": trace.stderr_truncated,
            "artifacts": trace.artifacts,
            "captureWarnings": trace.capture_warnings,
            "captureError": trace.capture_error,
            "cleanupError": trace.cleanup_error,
        }),
    );
    if let Some(parsed) = parsed {
        output.insert("json".to_owned(), parsed);
    }
    Json::Object(output)
}

fn parse_transcript(text: &str) -> Option<Json> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<Json>(trimmed) {
        return Some(value);
    }
    let mut events = Vec::new();
    for line in trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        match serde_json::from_str::<Json>(line) {
            Ok(value) => events.push(value),
            Err(_) => return None,
        }
    }
    if events.len() > 1 {
        Some(Json::Array(events))
    } else {
        None
    }
}

fn parse_json_maybe(text: &str) -> Option<Json> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<Json>(trimmed) {
        return Some(value);
    }
    let start = trimmed.find(['{', '['])?;
    let end = trimmed.rfind(['}', ']'])?;
    if start < end {
        serde_json::from_str::<Json>(&trimmed[start..=end]).ok()
    } else {
        None
    }
}

fn extract_score_from_text(text: &str) -> Option<f64> {
    parse_json_maybe(text)
        .as_ref()
        .and_then(extract_score_from_json)
}

fn transcript_score(text: &str) -> Option<f64> {
    let values: Vec<Json> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str::<Json>(line).ok())
        .collect();
    // Prefer a top-level score on any event, then the terminal event, then a
    // recursive search (e.g. a JSON document encoded inside a string field).
    for value in &values {
        if let Some(score) = top_score(value) {
            return Some(score);
        }
    }
    if let Some(last) = values.last()
        && let Some(score) = find_score(last)
    {
        return Some(score);
    }
    values.iter().find_map(find_score)
}

fn top_score(value: &Json) -> Option<f64> {
    value
        .get("score")
        .or_else(|| value.get("confidence"))
        .and_then(Json::as_f64)
        .map(|score| score.clamp(0.0, 1.0))
}

fn find_score(value: &Json) -> Option<f64> {
    if let Some(score) = value
        .get("score")
        .or_else(|| value.get("confidence"))
        .and_then(Json::as_f64)
    {
        return Some(score.clamp(0.0, 1.0));
    }
    if let Some(text) = value.as_str()
        && let Ok(parsed) = serde_json::from_str::<Json>(text)
        && let Some(score) = find_score(&parsed)
    {
        return Some(score);
    }
    match value {
        Json::Object(map) => map.values().find_map(find_score),
        Json::Array(items) => items.iter().find_map(find_score),
        _ => None,
    }
}

fn extract_score_from_json(value: &Json) -> Option<f64> {
    value
        .get("score")
        .or_else(|| value.get("confidence"))
        .and_then(Json::as_f64)
        .map(|score| score.clamp(0.0, 1.0))
}

fn hash_json(value: &Json) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn name_seed(name: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(bytes)
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push_str("...");
    out
}

/// Load a workflow through a [`SpecLoader`] and start it.
///
/// # Errors
/// Propagates loader and engine errors.
pub fn run_loaded(
    engine: &Engine,
    loader: &dyn SpecLoader,
    request: &crate::ports::LoadRequest,
) -> Result<RunReport> {
    let loaded = loader.load(request)?;
    engine.start(&loaded)
}
