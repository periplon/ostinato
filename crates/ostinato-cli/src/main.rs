use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use ostinato_adapters::{
    CommandAgent, CueCliLoader, NativeLoader, ProcessExecutor, SqliteStore, SystemClock,
    TracingTelemetry, UuidGen, init_tracing,
};
use ostinato_application::{
    Engine, EngineDeps, EngineOptions, ImproveRequest, LoadRequest, SpecLoader, params_patch,
};
use ostinato_domain::{RunStatus, audit, build_plan};

#[derive(Debug, Parser)]
#[command(
    name = "ostinato",
    version,
    about = "CUE-compatible durable, observable, agentic workflow engine",
    long_about = None
)]
struct Cli {
    /// SQLite journal path.
    #[arg(
        long,
        global = true,
        env = "OSTINATO_DB",
        default_value = ".ostinato/ostinato.db"
    )]
    db: PathBuf,

    /// Language evaluator. `cue` uses the official implementation.
    #[arg(long, global = true, value_enum, default_value_t = Evaluator::Rust)]
    evaluator: Evaluator,

    /// Path to the official `cue` executable.
    #[arg(long, global = true, env = "CUE_BIN")]
    cue: Option<PathBuf>,

    /// Increase log verbosity (-v, -vv).
    #[arg(short, long, global = true, action = ArgAction::Count)]
    verbose: u8,

    /// Emit machine-readable output where applicable.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Evaluator {
    /// Rust implementation of the required CUE workflow semantics (default).
    #[value(alias = "native")]
    Rust,
    /// Official CUE implementation, used as the conformance oracle.
    Cue,
}

#[derive(Debug, Clone, Subcommand)]
enum Command {
    /// Write a starter workflow and .gitignore.
    Init {
        /// Directory to initialise (default: current directory).
        path: Option<PathBuf>,
    },
    /// Evaluate and validate a workflow.
    Validate {
        file: PathBuf,
        #[arg(short = 'p', long = "param")]
        params: Vec<String>,
    },
    /// Print the execution plan as DOT or JSON.
    Graph {
        file: PathBuf,
        #[arg(long, value_enum, default_value_t = GraphFormat::Dot)]
        format: GraphFormat,
        #[arg(short = 'p', long = "param")]
        params: Vec<String>,
    },
    /// Start a new run.
    Run {
        file: PathBuf,
        #[arg(short = 'p', long = "param")]
        params: Vec<String>,
        /// Override workflow maxParallel.
        #[arg(long)]
        max_parallel: Option<usize>,
        /// Agent command; receives the prompt on stdin.
        #[arg(long, env = "OSTINATO_AGENT")]
        agent_cmd: Option<String>,
    },
    /// Run a CUE tool command declared in `_tool.cue` files.
    Cmd {
        /// Tool command name (`command: <name>`).
        name: String,
        /// Package file or directory.
        path: PathBuf,
        #[arg(short = 'p', long = "param")]
        params: Vec<String>,
    },
    /// Mark a run as cancelled (observed between groups).
    Cancel { run: String },
    /// Resume a failed or interrupted run from its durable checkpoint.
    Resume {
        run: String,
        #[arg(long, env = "OSTINATO_AGENT")]
        agent_cmd: Option<String>,
        #[arg(long)]
        max_parallel: Option<usize>,
    },
    /// Show run and step status.
    Status { run: String },
    /// List recent runs.
    Runs {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Print the audit log (and optionally follow it).
    Logs {
        run: String,
        #[arg(long)]
        follow: bool,
    },
    /// Verify the tamper-evident audit chain.
    Audit {
        #[command(subcommand)]
        command: AuditCommand,
    },
    /// Print the trial log (one row per attempt).
    Trials {
        run: String,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Ask an agent for an improvement proposal (optionally recursive).
    Improve {
        run: String,
        /// Number of recursive improvement rounds.
        #[arg(long, default_value_t = 1)]
        rounds: u32,
        #[arg(long, env = "OSTINATO_AGENT")]
        agent_cmd: Option<String>,
        #[arg(long)]
        prompt: Option<String>,
    },
    /// Show aggregated metrics.
    Metrics {
        #[arg(long)]
        run: Option<String>,
        #[arg(long, value_enum, default_value_t = MetricFormat::Text)]
        format: MetricFormat,
    },
    /// Environment and compliance diagnostics.
    Doctor,
}

#[derive(Debug, Clone, Subcommand)]
enum AuditCommand {
    /// Verify a run's hash chain.
    Verify { run: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum GraphFormat {
    Dot,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum MetricFormat {
    Text,
    Prometheus,
    Json,
}

fn main() -> ExitCode {
    std::thread::Builder::new()
        .name("ostinato".to_owned())
        .stack_size(512 * 1024 * 1024)
        .spawn(run)
        .ok()
        .and_then(|handle| handle.join().ok())
        .unwrap_or_else(run)
}

fn run() -> ExitCode {
    let cli = Cli::parse();
    if let Err(err) = init_tracing(cli.verbose, cli.json) {
        eprintln!("warning: {err}");
    }
    match execute(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn execute(cli: Cli) -> Result<ExitCode> {
    let json = cli.json;
    match cli.command.clone() {
        Command::Init { path } => {
            let directory = path.unwrap_or_else(|| PathBuf::from("."));
            init_project(&directory, json)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Validate { file, params } => {
            let (loader, _) = build_loader(&cli)?;
            let loaded = loader.load(&LoadRequest {
                source: file,
                params: parse_params(&params)?,
            })?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "workflow": loaded.spec,
                        "params": loaded.params,
                        "evaluator": loaded.evaluator,
                    }))?
                );
            } else {
                println!(
                    "OK {} ({} steps, maxParallel={}, evaluator={})",
                    loaded.spec.name,
                    loaded.spec.steps.len(),
                    loaded.spec.max_parallel,
                    loaded.evaluator
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Graph {
            file,
            format,
            params,
        } => {
            let (loader, _) = build_loader(&cli)?;
            let loaded = loader.load(&LoadRequest {
                source: file,
                params: parse_params(&params)?,
            })?;
            let plan = build_plan(&loaded.spec)?;
            match format {
                GraphFormat::Dot => print_dot(&loaded.spec),
                GraphFormat::Json => println!("{}", serde_json::to_string_pretty(&plan)?),
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Run {
            file,
            params,
            max_parallel,
            agent_cmd,
        } => {
            let (loader, _) = build_loader(&cli)?;
            let (engine, _) = build_runtime(&cli, agent_cmd, max_parallel)?;
            let loaded = loader.load(&LoadRequest {
                source: file,
                params: parse_params(&params)?,
            })?;
            let report = engine.start(&loaded)?;
            print_report(&report, json)?;
            Ok(exit_for_status(report.status))
        }
        Command::Cmd { name, path, params } => {
            let code =
                match ostinato_adapters::run_tool_command(&path, &name, &parse_params(&params)?) {
                    Ok(code) => code,
                    Err(err) => {
                        eprintln!("error: {err}");
                        return Ok(ExitCode::from(1));
                    }
                };
            Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
        }
        Command::Cancel { run } => {
            let (engine, _) = build_runtime(&cli, None, None)?;
            let event = engine.cancel(&run)?;
            println!("cancelled run={run} seq={} hash={}", event.seq, event.hash);
            Ok(ExitCode::SUCCESS)
        }
        Command::Resume {
            run,
            agent_cmd,
            max_parallel,
        } => {
            let (engine, _) = build_runtime(&cli, agent_cmd, max_parallel)?;
            let report = engine.resume(&run)?;
            print_report(&report, json)?;
            Ok(exit_for_status(report.status))
        }
        Command::Status { run } => {
            let (engine, _) = build_runtime(&cli, None, None)?;
            let record = engine
                .store()
                .load_run(&run)?
                .with_context(|| format!("run not found: {run}"))?;
            print_status(&record, engine.store().steps(&run)?, json)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Runs { limit } => {
            let (engine, _) = build_runtime(&cli, None, None)?;
            let runs = engine.store().list_runs(limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&runs)?);
            } else {
                println!(
                    "{:<38} {:<16} {:<10} {:<8} {:<24}",
                    "RUN", "WORKFLOW", "STATUS", "STEPS", "CREATED"
                );
                for run in runs {
                    println!(
                        "{:<38} {:<16} {:<10} {:<8} {:<24}",
                        run.id,
                        run.workflow_name,
                        run.status.as_str(),
                        run.spec.steps.len(),
                        run.created_at
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Logs { run, follow } => {
            let (engine, _) = build_runtime(&cli, None, None)?;
            let mut record = engine
                .store()
                .load_run(&run)?
                .with_context(|| format!("run not found: {run}"))?;
            let mut last_seq: i64 = -1;
            loop {
                let events = engine.store().events(&run)?;
                for event in events {
                    if i64::try_from(event.seq).unwrap_or(i64::MAX) > last_seq {
                        last_seq = i64::try_from(event.seq).unwrap_or(i64::MAX);
                        if json {
                            println!("{}", serde_json::to_string(&event)?);
                        } else {
                            println!(
                                "{} {} {} step={} {}",
                                event.seq,
                                event.ts,
                                event.kind,
                                event.step.as_deref().unwrap_or("-"),
                                event.data
                            );
                        }
                    }
                }
                if !follow {
                    break;
                }
                if record.status.is_terminal() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                if let Some(current) = engine.store().load_run(&run)? {
                    record = current;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Audit {
            command: AuditCommand::Verify { run },
        } => {
            let (engine, _) = build_runtime(&cli, None, None)?;
            let snapshot = engine
                .store()
                .snapshot(&run)?
                .with_context(|| format!("run not found: {run}"))?;
            let events = snapshot.events;
            let record = snapshot.run;
            let steps = snapshot.steps;
            match audit::verify_chain(&events) {
                Ok(()) => {
                    let head = events.last().map_or_else(
                        || audit::GENESIS_HASH.to_owned(),
                        |event| event.hash.clone(),
                    );
                    if head != record.audit_head {
                        eprintln!(
                            "AUDIT FAILED run={run}: audit tail does not match the persisted head"
                        );
                        return Ok(ExitCode::FAILURE);
                    }
                    if let Some(expected) = ostinato_domain::lifecycle_status(&events)
                        && record.status.as_str() != expected
                    {
                        eprintln!(
                            "AUDIT FAILED run={run}: status row is `{}` but the audit chain ends in `{expected}`",
                            record.status.as_str()
                        );
                        return Ok(ExitCode::FAILURE);
                    }
                    let digest = ostinato_domain::state_digest(
                        &record.spec,
                        &record.params,
                        &record.source_file,
                        record.resume_index,
                        &steps,
                    );
                    if digest != record.state_hash {
                        eprintln!(
                            "AUDIT FAILED run={run}: execution state does not match the persisted digest"
                        );
                        return Ok(ExitCode::FAILURE);
                    }
                    let anchored = events
                        .iter()
                        .rev()
                        .find(|event| event.kind == "state.committed")
                        .and_then(|event| event.data.get("stateHash"))
                        .and_then(|value| value.as_str())
                        .or_else(|| {
                            events
                                .iter()
                                .find(|event| event.kind == "run.started")
                                .and_then(|event| event.data.get("stateHash"))
                                .and_then(|value| value.as_str())
                        });
                    if let Some(anchored) = anchored
                        && anchored != record.state_hash
                    {
                        eprintln!(
                            "AUDIT FAILED run={run}: execution state is not anchored in the audit chain"
                        );
                        return Ok(ExitCode::FAILURE);
                    }
                    println!("OK run={run} events={} head={head}", events.len());
                    Ok(ExitCode::SUCCESS)
                }
                Err(err) => {
                    eprintln!("AUDIT FAILED run={run}: {err}");
                    Ok(ExitCode::FAILURE)
                }
            }
        }
        Command::Trials { run, limit } => {
            let (engine, _) = build_runtime(&cli, None, None)?;
            let mut trials = engine.store().trials(&run)?;
            if let Some(limit) = limit {
                let start = trials.len().saturating_sub(limit);
                trials = trials.split_off(start);
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&trials)?);
            } else {
                println!(
                    "{:<20} {:<5} {:<6} {:<8} {:<10} {:<8} ERROR",
                    "STEP", "ROUND", "ATTEMPT", "STATUS", "SCORE", "DUR(ms)"
                );
                for trial in trials {
                    println!(
                        "{:<20} {:<5} {:<6} {:<8} {:<10.3} {:<8} {}",
                        trial.step,
                        trial.round,
                        trial.attempt,
                        trial.status.as_str(),
                        trial.score,
                        trial.duration_ms,
                        trial.error.unwrap_or_default()
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Improve {
            run,
            rounds,
            agent_cmd,
            prompt,
        } => {
            let (loader, _) = build_loader(&cli)?;
            let (engine, _) = build_runtime(&cli, agent_cmd, None)?;
            let mut current_run = run;
            let mut last_improvement_id: Option<i64> = None;
            let mut improvements_json = Vec::new();
            let mut children_json = Vec::new();
            for round in 0..rounds.max(1) {
                let record = engine
                    .store()
                    .load_run(&current_run)?
                    .with_context(|| format!("run not found: {current_run}"))?;
                let improvement = engine.improve(&ImproveRequest {
                    run_id: current_run.clone(),
                    parent_id: last_improvement_id,
                    round,
                    prompt: prompt.clone(),
                    max_samples: 20,
                    command: None,
                    cwd: workflow_cwd(&record.source_file),
                    timeout: None,
                })?;
                last_improvement_id = Some(improvement.id);
                if json {
                    improvements_json.push(serde_json::to_value(&improvement)?);
                } else {
                    println!(
                        "improvement #{} run={} diagnosis={}",
                        improvement.id, improvement.run_id, improvement.diagnosis
                    );
                    println!("{}", serde_json::to_string_pretty(&improvement.proposal)?);
                }

                let patch = params_patch(&improvement.proposal)?;
                if patch.is_empty() {
                    if !json {
                        println!("no params_patch in proposal; stopping recursion");
                    }
                    break;
                }
                let mut params = record.params.as_object().cloned().unwrap_or_default();
                for (key, value) in patch {
                    params.insert(key, value);
                }
                let merged = serde_json::Value::Object(params);
                // Re-evaluate CUE with the patched parameters so that both
                // compile-time interpolation and runtime templates see it.
                let loaded = loader.load(&LoadRequest {
                    source: PathBuf::from(&record.source_file),
                    params: json_param_strings(&merged),
                })?;
                let child = engine.start_with(
                    loaded.source_file,
                    loaded.spec,
                    loaded.params,
                    Some(record.id.clone()),
                )?;
                let child_json = serde_json::json!({
                    "run": child.run_id,
                    "status": child.status.as_str(),
                    "parent": record.id,
                });
                if json {
                    children_json.push(child_json);
                } else {
                    println!(
                        "child run {} status={} parent={}",
                        child.run_id,
                        child.status.as_str(),
                        record.id
                    );
                }
                let child_failed = !matches!(child.status, RunStatus::Succeeded);
                current_run = child.run_id;
                if child_failed {
                    if !json {
                        println!("child run did not succeed; stopping recursion");
                    }
                    break;
                }
            }
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "improvements": improvements_json,
                        "children": children_json,
                    }))?
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Metrics { run, format } => {
            let (engine, _) = build_runtime(&cli, None, None)?;
            let metrics = engine.metrics(run.as_deref())?;
            match format {
                MetricFormat::Text if json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "runsTotal": metrics.runs_total,
                            "runsSucceeded": metrics.runs_succeeded,
                            "runsFailed": metrics.runs_failed,
                            "runsRunning": metrics.runs_running,
                            "stepsTotal": metrics.steps_total,
                            "stepsFailed": metrics.steps_failed,
                            "stepsSkipped": metrics.steps_skipped,
                            "gatewaysEvaluated": metrics.gateways_evaluated,
                            "attemptsTotal": metrics.attempts_total,
                            "retriesTotal": metrics.retries_total,
                            "avgAttemptMs": metrics.avg_attempt_ms,
                        }))?
                    );
                }
                MetricFormat::Text => {
                    println!("runs_total        {}", metrics.runs_total);
                    println!("runs_succeeded    {}", metrics.runs_succeeded);
                    println!("runs_failed       {}", metrics.runs_failed);
                    println!("runs_running      {}", metrics.runs_running);
                    println!("steps_total       {}", metrics.steps_total);
                    println!("steps_failed      {}", metrics.steps_failed);
                    println!("steps_skipped     {}", metrics.steps_skipped);
                    println!("gateways_evaluated {}", metrics.gateways_evaluated);
                    println!("attempts_total    {}", metrics.attempts_total);
                    println!("retries_total     {}", metrics.retries_total);
                    println!("avg_attempt_ms    {:.2}", metrics.avg_attempt_ms);
                }
                MetricFormat::Prometheus => {
                    println!("# HELP ostinato_runs_total Runs started.");
                    println!("# TYPE ostinato_runs_total counter");
                    println!("ostinato_runs_total {}", metrics.runs_total);
                    println!("ostinato_runs_succeeded_total {}", metrics.runs_succeeded);
                    println!("ostinato_runs_failed_total {}", metrics.runs_failed);
                    println!("ostinato_runs_running {}", metrics.runs_running);
                    println!("ostinato_steps_total {}", metrics.steps_total);
                    println!("ostinato_steps_failed_total {}", metrics.steps_failed);
                    println!("ostinato_steps_skipped_total {}", metrics.steps_skipped);
                    println!(
                        "ostinato_gateways_evaluated_total {}",
                        metrics.gateways_evaluated
                    );
                    println!("ostinato_attempts_total {}", metrics.attempts_total);
                    println!("ostinato_retries_total {}", metrics.retries_total);
                    println!(
                        "ostinato_attempt_duration_ms_avg {:.2}",
                        metrics.avg_attempt_ms
                    );
                }
                MetricFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "runsTotal": metrics.runs_total,
                            "runsSucceeded": metrics.runs_succeeded,
                            "runsFailed": metrics.runs_failed,
                            "runsRunning": metrics.runs_running,
                            "stepsTotal": metrics.steps_total,
                            "stepsFailed": metrics.steps_failed,
                            "stepsSkipped": metrics.steps_skipped,
                            "gatewaysEvaluated": metrics.gateways_evaluated,
                            "attemptsTotal": metrics.attempts_total,
                            "retriesTotal": metrics.retries_total,
                            "avgAttemptMs": metrics.avg_attempt_ms,
                        }))?
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor => doctor(&cli),
    }
}

fn build_loader(cli: &Cli) -> Result<(Arc<dyn SpecLoader>, String)> {
    match cli.evaluator {
        Evaluator::Cue => {
            let loader = CueCliLoader::new(cli.cue.clone())?;
            let version = loader.version().to_owned();
            Ok((Arc::new(loader), version))
        }
        Evaluator::Rust => Ok((Arc::new(NativeLoader), "rust".to_owned())),
    }
}

fn workflow_cwd(source: &str) -> Option<PathBuf> {
    let path = Path::new(source);
    if path.is_dir() {
        return Some(path.to_path_buf());
    }
    path.parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
}

fn json_param_strings(value: &serde_json::Value) -> BTreeMap<String, String> {
    match value {
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(key, value)| {
                let raw = match value {
                    serde_json::Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                (key.clone(), raw)
            })
            .collect(),
        _ => BTreeMap::new(),
    }
}

fn build_runtime(
    cli: &Cli,
    agent_cmd: Option<String>,
    max_parallel: Option<usize>,
) -> Result<(Engine, Arc<SqliteStore>)> {
    let store = Arc::new(SqliteStore::open(&cli.db)?);
    let engine = Engine::new(
        EngineDeps {
            store: store.clone(),
            executor: Arc::new(ProcessExecutor),
            agent: Arc::new(CommandAgent::new(agent_cmd)),
            clock: Arc::new(SystemClock),
            ids: Arc::new(UuidGen),
            telemetry: Arc::new(TracingTelemetry),
        },
        EngineOptions {
            max_parallel,
            ..EngineOptions::default()
        },
    );
    Ok((engine, store))
}

fn parse_params(values: &[String]) -> Result<BTreeMap<String, String>> {
    let mut params = BTreeMap::new();
    for value in values {
        let Some((key, raw)) = value.split_once('=') else {
            bail!("invalid parameter `{value}`; expected key=value");
        };
        if key.is_empty() {
            bail!("invalid parameter `{value}`; empty key");
        }
        if params.insert(key.to_owned(), raw.to_owned()).is_some() {
            bail!("duplicate parameter `{key}`");
        }
    }
    Ok(params)
}

fn exit_for_status(status: RunStatus) -> ExitCode {
    match status {
        RunStatus::Succeeded => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

fn print_report(report: &ostinato_domain::RunReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }
    println!(
        "run={} workflow={} status={} durationMs={}",
        report.run_id,
        report.workflow_name,
        report.status.as_str(),
        report.duration_ms
    );
    if let Some(error) = &report.error {
        println!("error: {error}");
    }
    println!(
        "{:<24} {:<10} {:<6} {:<8}",
        "STEP", "STATUS", "VISITS", "ATTEMPTS"
    );
    for step in &report.steps {
        println!(
            "{:<24} {:<10} {:<6} {:<8}",
            step.name,
            step.status.as_str(),
            step.visits,
            step.attempts
        );
    }
    Ok(())
}

fn print_status(
    run: &ostinato_domain::RunRecord,
    steps: Vec<ostinato_domain::StepRecord>,
    json: bool,
) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "run": run,
                "steps": steps,
            }))?
        );
        return Ok(());
    }
    println!(
        "run={} workflow={} status={} resumeIndex={}",
        run.id,
        run.workflow_name,
        run.status.as_str(),
        run.resume_index
    );
    println!("source={}", run.source_file);
    println!("created={} updated={}", run.created_at, run.updated_at);
    if let Some(parent) = &run.parent_run_id {
        println!("parent={parent}");
    }
    if let Some(error) = &run.error {
        println!("error={error}");
    }
    println!(
        "{:<24} {:<10} {:<6} {:<8}",
        "STEP", "STATUS", "VISITS", "ATTEMPTS"
    );
    for step in steps {
        println!(
            "{:<24} {:<10} {:<6} {:<8}",
            step.name,
            step.status.as_str(),
            step.visits,
            step.attempts
        );
    }
    Ok(())
}

fn print_dot(spec: &ostinato_domain::WorkflowSpec) {
    println!("digraph {} {{", sanitize(&spec.name));
    println!("  rankdir=LR;");
    for (name, step) in &spec.steps {
        let mode = step.gateway.as_ref().and_then(|gateway| {
            gateway
                .split
                .map(|mode| format!("split: {}", mode.as_str()))
                .or_else(|| gateway.join.map(|mode| format!("join: {}", mode.as_str())))
        });
        let label = match &mode {
            Some(mode) => format!("{}\\n({})", dot_escape(name), dot_escape(mode)),
            None => format!("{}\\n({})", dot_escape(name), step.kind.as_str()),
        };
        if step.gateway.is_some() {
            println!(
                "  \"{}\" [label=\"{}\", shape=diamond];",
                sanitize(name),
                label
            );
        } else {
            println!("  \"{}\" [label=\"{}\"];", sanitize(name), label);
        }
    }
    for (name, step) in &spec.steps {
        for need in &step.needs {
            println!("  \"{}\" -> \"{}\";", sanitize(need), sanitize(name));
        }
        let Some(gateway) = &step.gateway else {
            continue;
        };
        if gateway.split.is_none() {
            continue;
        }
        for (branch, branch_spec) in &gateway.branches {
            let condition = branch_spec.condition.as_deref().unwrap_or("true");
            let label = if gateway.default.as_deref() == Some(branch.as_str()) {
                format!("default: {condition}")
            } else {
                condition.to_owned()
            };
            println!(
                "  \"{}\" -> \"{}\" [label=\"{}\"];",
                sanitize(name),
                sanitize(branch),
                dot_escape(&label)
            );
        }
    }
    println!("}}");
}

fn sanitize(name: &str) -> String {
    name.replace('"', "_")
}

fn dot_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn init_project(directory: &Path, json: bool) -> Result<()> {
    std::fs::create_dir_all(directory)?;
    let workflow_path = directory.join("workflow.cue");
    let gitignore_path = directory.join(".gitignore");
    if !workflow_path.exists() {
        std::fs::write(&workflow_path, STARTER)?;
    }
    if !gitignore_path.exists() {
        std::fs::write(&gitignore_path, ".ostinato/\n")?;
    } else {
        let existing = std::fs::read_to_string(&gitignore_path).unwrap_or_default();
        if !existing.lines().any(|line| line.trim() == ".ostinato/") {
            let mut updated = existing;
            if !updated.ends_with('\n') && !updated.is_empty() {
                updated.push('\n');
            }
            updated.push_str(".ostinato/\n");
            std::fs::write(&gitignore_path, updated)?;
        }
    }
    if json {
        println!(
            "{}",
            serde_json::json!({
                "workflow": workflow_path,
                "gitignore": gitignore_path,
            })
        );
    } else {
        println!("wrote {}", workflow_path.display());
        println!("wrote {}", gitignore_path.display());
        println!(
            "next: ostinato validate {} && ostinato run {}",
            workflow_path.display(),
            workflow_path.display()
        );
    }
    Ok(())
}

fn doctor(cli: &Cli) -> Result<ExitCode> {
    println!("ostinato {}", env!("CARGO_PKG_VERSION"));
    println!(
        "evaluator {}",
        match cli.evaluator {
            Evaluator::Rust => "rust (required CUE workflow semantics; default)",
            Evaluator::Cue => "cue (official conformance oracle)",
        }
    );
    match CueCliLoader::new(cli.cue.clone()) {
        Ok(loader) => println!("oracle {}", loader.version()),
        Err(err) if cli.evaluator == Evaluator::Cue => {
            eprintln!("cue oracle unavailable: {err}");
            return Ok(ExitCode::FAILURE);
        }
        Err(err) => println!("oracle unavailable: {err}"),
    }
    println!("db {}", cli.db.display());
    println!(
        "agent {}",
        std::env::var("OSTINATO_AGENT").map_or_else(
            |_| "not configured (agent steps will fail)".to_owned(),
            |_| "configured".to_owned()
        )
    );
    // Prove the store can be opened and migrated.
    let _ = SqliteStore::open(&cli.db)?;
    println!("store OK");
    Ok(ExitCode::SUCCESS)
}

const STARTER: &str = r#"package ostinato

// Parameters are ordinary CUE values. `@tag` lets the CLI inject them with
// `-p name=value`, using the official CUE implementation.
params: {
	name: string | *"world" @tag(name)
}

workflow: {
	name:        "hello"
	maxParallel: 2

	steps: {
		prepare: {
			kind: "exec"
			exec: {
				cmd: ["sh", "-c", "echo preparing \(params.name)"]
			}
		}

		greet: {
			kind:  "exec"
			needs: ["prepare"]
			retry: {max: 2, baseMs: 50}
			exec: {
				cmd: ["sh", "-c", "echo hello \(params.name)"]
			}
		}
	}
}
"#;
