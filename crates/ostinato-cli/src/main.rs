use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use ostinato_adapters::{
    CommandAgent, CueCliLoader, FileOutputSink, JsonYamlCodec, LocalWorkspace, NativeLoader,
    ProcessExecutor, SqliteStore, SystemClock, TracingTelemetry, UuidGen, cue_import, data,
    init_tracing,
};
use ostinato_application::{
    Clock, DataInput, DocumentCodec, DocumentFormat, Engine, EngineDeps, EngineOptions,
    ImproveRequest, LoadRequest, OutputOptions, OutputSink, SpecLoader, Store, WorkflowEventQuery,
    params_patch,
};
use ostinato_domain::{
    EventSource, JoinMode, NewWorkflowEvent, RunStatus, analyze, audit, build_plan,
};

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
        /// Unify a JSON/YAML file into the CUE root (repeatable).
        #[arg(long = "input", visible_alias = "data")]
        inputs: Vec<PathBuf>,
        /// Treat static-analysis warnings as errors.
        #[arg(long)]
        strict: bool,
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
        /// Unify a JSON/YAML file into the CUE root (repeatable).
        #[arg(long = "input", visible_alias = "data")]
        inputs: Vec<PathBuf>,
        /// Override workflow maxParallel.
        #[arg(long)]
        max_parallel: Option<usize>,
        /// Agent command; receives the prompt on stdin.
        #[arg(long, env = "OSTINATO_AGENT")]
        agent_cmd: Option<String>,
        /// Evaluate and print the workflow instead of executing it.
        #[arg(long)]
        dry_run: bool,
        /// Emit the result as JSON or YAML instead of text.
        #[arg(long = "out", value_enum)]
        out: Option<DataFormatArg>,
        /// Write the result to a file instead of stdout.
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
        /// Overwrite an existing output file.
        #[arg(short = 'f', long)]
        force: bool,
    },
    /// Export an evaluated CUE value or data document as JSON or YAML.
    Export {
        /// CUE package/file/directory or JSON/YAML data file.
        #[arg(required = true)]
        sources: Vec<PathBuf>,
        /// Select a value by path (repeatable; emits a JSON/YAML stream).
        #[arg(short = 'e', long = "expression")]
        expressions: Vec<String>,
        /// Output encoding (default: json, or inferred from --output).
        #[arg(long = "out", value_enum)]
        out: Option<DataFormatArg>,
        /// Write to a file instead of stdout (`-` means stdout).
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
        /// Wrap data documents into a top-level list (CUE `--list`).
        #[arg(long)]
        list: bool,
        #[arg(short = 'p', long = "param")]
        params: Vec<String>,
        /// Unify a JSON/YAML file into the CUE root (repeatable).
        #[arg(long = "input", visible_alias = "data")]
        inputs: Vec<PathBuf>,
        /// Overwrite an existing output file.
        #[arg(short = 'f', long)]
        force: bool,
    },
    /// Convert JSON or YAML files into CUE source.
    Import {
        /// JSON/YAML files to convert (repeatable).
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Write to one CUE file (`-` means stdout).
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
        /// CUE package name for the generated file(s).
        #[arg(short = 'p', long = "package")]
        package: Option<String>,
        /// Wrap documents into a top-level list (CUE `--list`).
        #[arg(long)]
        list: bool,
        /// Split multi-document YAML into `<stem>.cue`, `<stem>-1.cue`, ...
        #[arg(long)]
        files: bool,
        /// Unify all inputs into one CUE file (requires `-o`).
        #[arg(long)]
        merge: bool,
        /// Overwrite existing CUE files.
        #[arg(short = 'f', long)]
        force: bool,
        /// Skip existing files and extra inputs with exit status 0 (CUE compat).
        #[arg(long)]
        skip_existing: bool,
    },
    /// Run a CUE tool command declared in `*_tool.cue` files.
    #[command(disable_help_flag = true)]
    Cmd {
        /// Tool command name (`command: <name>`).
        name: Option<String>,
        /// Package file or directory (repeatable; `cue cmd` inputs).
        paths: Vec<PathBuf>,
        #[arg(short = 'p', long = "param")]
        params: Vec<String>,
        /// List commands or show `$usage`/`$short`/`$long` metadata.
        #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
        help: bool,
    },
    /// Mark a run as cancelled (observed between groups).
    Cancel { run: String },
    /// Resume a failed or interrupted run from its durable checkpoint.
    Resume {
        run: String,
        /// Agent command for `provider: "command"` steps (receives the prompt on stdin).
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
    /// Inject an external workflow event into a run.
    Event {
        run: String,
        name: String,
        /// JSON payload (default: null).
        #[arg(long)]
        payload: Option<String>,
        /// Optional idempotency key; retries with the same key are no-ops.
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// List or stream durable workflow events.
    Events {
        run: String,
        /// Keep polling until the run is terminal and no events remain.
        #[arg(long)]
        follow: bool,
        /// Only show events with seq greater than this value.
        #[arg(long)]
        after: Option<u64>,
        /// Only show events with this name.
        #[arg(long)]
        name: Option<String>,
        /// Only show events from this source.
        #[arg(long, value_enum)]
        source: Option<EventSourceArg>,
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
    /// List agent execution traces and verify their hashes.
    Agents {
        run: String,
        /// Only show traces for this step.
        #[arg(long)]
        step: Option<String>,
        /// Re-hash traces and artifacts; non-zero exit on mismatch.
        #[arg(long)]
        verify: bool,
    },
    /// List or extract agent artifacts.
    Artifacts {
        run: String,
        /// Only list artifacts for this step.
        #[arg(long)]
        step: Option<String>,
        /// Only list artifacts for this trace id.
        #[arg(long)]
        trace: Option<i64>,
        /// Only list artifacts for this round.
        #[arg(long)]
        round: Option<u32>,
        /// Only list artifacts for this attempt.
        #[arg(long)]
        attempt: Option<u32>,
        /// Extract artifact contents under this directory.
        #[arg(long)]
        extract: Option<PathBuf>,
        /// Allow writing into a non-empty directory or replacing files.
        #[arg(long)]
        force: bool,
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
enum EventSourceArg {
    Internal,
    External,
    Any,
}

impl EventSourceArg {
    fn as_source(self) -> ostinato_domain::EventSource {
        match self {
            Self::Internal => ostinato_domain::EventSource::Internal,
            Self::External => ostinato_domain::EventSource::External,
            Self::Any => ostinato_domain::EventSource::Any,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum GraphFormat {
    Dot,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum DataFormatArg {
    Json,
    Yaml,
}

impl DataFormatArg {
    fn as_document(self) -> DocumentFormat {
        match self {
            Self::Json => DocumentFormat::Json,
            Self::Yaml => DocumentFormat::Yaml,
        }
    }
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
        Command::Validate {
            file,
            params,
            inputs,
            strict,
        } => {
            let (loader, _) = build_loader(&cli)?;
            let loaded = loader.load(&LoadRequest {
                source: file,
                params: parse_params(&params)?,
                inputs: data_inputs(&inputs),
            })?;
            let plan = build_plan(&loaded.spec)?;
            let report = analyze(&loaded.spec, &plan);
            let strict_failure = strict && report.has_warnings();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "workflow": loaded.spec,
                        "params": loaded.params,
                        "evaluator": loaded.evaluator,
                        "findings": report.findings,
                        "findingsTruncated": report.truncated,
                        "findingsSchemaVersion": 1,
                    }))?
                );
            } else {
                println!(
                    "{} {} ({} steps, maxParallel={}, evaluator={})",
                    if strict_failure { "FAIL" } else { "OK" },
                    loaded.spec.name,
                    loaded.spec.steps.len(),
                    loaded.spec.max_parallel,
                    loaded.evaluator
                );
                for finding in &report.findings {
                    println!(
                        "{} {} {}: {}",
                        finding.severity.as_str(),
                        finding.code,
                        finding.path,
                        finding.message
                    );
                }
                if report.truncated {
                    println!("note findings truncated at the display limit");
                }
            }
            if strict_failure {
                Ok(ExitCode::from(2))
            } else {
                Ok(ExitCode::SUCCESS)
            }
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
                inputs: Vec::new(),
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
            inputs,
            max_parallel,
            agent_cmd,
            dry_run,
            out,
            output,
            force,
        } => {
            let (loader, _) = build_loader(&cli)?;
            let request = LoadRequest {
                source: file.clone(),
                params: parse_params(&params)?,
                inputs: data_inputs(&inputs),
            };
            let mut protected = vec![file];
            protected.extend(inputs);
            if dry_run {
                let loaded = loader.load(&request)?;
                let value = serde_json::json!({
                    "workflow": loaded.spec,
                    "params": loaded.params,
                    "evaluator": loaded.evaluator,
                    "source": loaded.source_file,
                });
                write_values(output.as_deref(), out, &[value], force, &protected)?;
                return Ok(ExitCode::SUCCESS);
            }
            let (engine, _) = build_runtime(&cli, agent_cmd, max_parallel)?;
            let loaded = loader.load(&request)?;
            let report = engine.start(&loaded)?;
            if out.is_some() || output.is_some() {
                let value =
                    serde_json::to_value(&report).context("cannot encode run report as data")?;
                write_values(output.as_deref(), out, &[value], force, &protected)?;
            } else {
                print_report(&report, json)?;
            }
            Ok(exit_for_status(report.status))
        }
        Command::Export {
            sources,
            expressions,
            out,
            output,
            list,
            params,
            inputs,
            force,
        } => {
            let params = parse_params(&params)?;
            if list && !inputs.is_empty() {
                bail!("--list does not support --input; pass the documents in the source files");
            }
            let mut protected = sources.clone();
            protected.extend(inputs.iter().cloned());
            let values = if sources
                .iter()
                .all(|source| data::DataFormat::is_data_path(source))
            {
                if !params.is_empty() {
                    bail!("-p/--param is only supported with CUE sources");
                }
                let codec = JsonYamlCodec;
                let source = &sources[0];
                let mut per_source = Vec::with_capacity(sources.len());
                for source in &sources {
                    per_source.push(
                        codec
                            .read(&DataInput::new(source.clone()))
                            .map_err(|error| anyhow::anyhow!(error.to_string()))?,
                    );
                }
                if list {
                    let length = per_source[0].len();
                    if let Some(documents) = per_source
                        .iter()
                        .find(|documents| documents.len() != length)
                    {
                        bail!(
                            "incompatible list lengths ({} and {})",
                            length,
                            documents.len()
                        );
                    }
                    let mut documents = Vec::with_capacity(length);
                    for index in 0..length {
                        let mut merged = per_source[0][index].clone();
                        for source_documents in &per_source[1..] {
                            merged = data::unify_values(&merged, &source_documents[index], source)
                                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                        }
                        documents.push(merged);
                    }
                    let value = serde_json::Value::Array(documents);
                    select_data_values(&value, &expressions)?
                } else {
                    let mut documents: Vec<serde_json::Value> =
                        per_source.into_iter().flatten().collect();
                    for input in &inputs {
                        documents.extend(
                            codec
                                .read(&DataInput::new(input.clone()))
                                .map_err(|error| anyhow::anyhow!(error.to_string()))?,
                        );
                    }
                    let merged = unify_export_documents(&documents, source)?;
                    select_data_values(&merged, &expressions)?
                }
            } else if !sources
                .iter()
                .all(|source| !data::DataFormat::is_data_path(source))
            {
                bail!("cannot mix CUE and data sources in one export");
            } else {
                let (loader, _) = build_loader(&cli)?;
                if sources.len() == 1 {
                    loader
                        .evaluate(
                            &LoadRequest {
                                source: sources[0].clone(),
                                params,
                                inputs: data_inputs(&inputs),
                            },
                            &expressions,
                            list,
                        )
                        .map_err(|error| anyhow::anyhow!(error.to_string()))?
                } else {
                    let mut root: Option<ostinato_lang::Value> = None;
                    for (index, source) in sources.iter().enumerate() {
                        let evaluated = loader
                            .evaluate(
                                &LoadRequest {
                                    source: source.clone(),
                                    params: params.clone(),
                                    inputs: if index == 0 {
                                        data_inputs(&inputs)
                                    } else {
                                        Vec::new()
                                    },
                                },
                                &[],
                                false,
                            )
                            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                        for value in evaluated {
                            let value = ostinato_lang::value_from_json(&value);
                            root = Some(match root {
                                Some(current) => ostinato_lang::unify(&current, &value)
                                    .map_err(|error| anyhow::anyhow!(error.to_string()))?,
                                None => value,
                            });
                        }
                    }
                    let root = root.ok_or_else(|| anyhow::anyhow!("no CUE value to export"))?;
                    if expressions.is_empty() {
                        vec![
                            root.to_json().ok_or_else(|| {
                                anyhow::anyhow!("cannot export an incomplete value")
                            })?,
                        ]
                    } else {
                        let mut values = Vec::with_capacity(expressions.len());
                        for expression in &expressions {
                            let selected = ostinato_lang::select_path(&root, expression)
                                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                            values.push(selected.to_json().ok_or_else(|| {
                                anyhow::anyhow!("cannot export `{expression}`: value is incomplete")
                            })?);
                        }
                        values
                    }
                }
            };
            write_values(output.as_deref(), out, &values, force, &protected)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Import {
            inputs,
            output,
            package,
            list,
            files,
            merge,
            force,
            skip_existing,
        } => {
            if merge && output.is_none() {
                bail!("--merge requires -o/--output");
            }
            if merge && files {
                bail!("--merge and --files are mutually exclusive");
            }
            let plan = cue_import::import_files(
                &inputs,
                output.as_deref(),
                package.as_deref(),
                list,
                files,
                merge,
                force,
            )?;
            preflight_outputs(&plan.files, &inputs, force, skip_existing)?;
            let options = OutputOptions {
                force,
                skip_existing,
            };
            if plan.files.iter().any(|file| file.path == Path::new("-")) {
                let sink = FileOutputSink;
                for file in &plan.files {
                    let outcome = sink
                        .write(Some(&file.path), &file.content, options, &inputs)
                        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                    if outcome == ostinato_application::WriteOutcome::Skipped {
                        eprintln!(
                            "skipping {}: already exists (--skip-existing)",
                            file.path.display()
                        );
                    }
                }
            } else {
                // Stage every output before replacing any destination so a
                // late failure cannot leave a partial import behind.
                let files: Vec<(PathBuf, String)> = plan
                    .files
                    .iter()
                    .map(|file| (file.path.clone(), file.content.clone()))
                    .collect();
                let outcomes = ostinato_adapters::write_all_atomic(&files, options, &inputs)
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                for (file, outcome) in plan.files.iter().zip(outcomes) {
                    if outcome == ostinato_application::WriteOutcome::Skipped {
                        eprintln!(
                            "skipping {}: already exists (--skip-existing)",
                            file.path.display()
                        );
                    }
                }
            }
            if !plan.skipped_inputs.is_empty() && !force {
                for input in &plan.skipped_inputs {
                    eprintln!(
                        "skipping {}: -o with multiple inputs writes only the first \
                         (use --merge for one file or omit -o)",
                        input.display()
                    );
                }
                if !skip_existing {
                    eprintln!(
                        "warning: some inputs were not written; pass --skip-existing for CUE-compatible exit status 0"
                    );
                    return Ok(ExitCode::FAILURE);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Cmd {
            name,
            paths,
            params,
            help,
        } => {
            if help {
                return Ok(tool_command_help(name.as_deref(), &paths));
            }
            let Some(name) = name else {
                eprintln!("cmd must be run as one of its subcommands");
                eprintln!("Run 'ostinato cmd --help' for known subcommands.");
                return Ok(ExitCode::from(1));
            };
            let paths = if paths.is_empty() {
                vec![PathBuf::from(".")]
            } else {
                paths
            };
            let code = match ostinato_adapters::run_tool_command_paths(
                &paths,
                &name,
                &parse_params(&params)?,
            ) {
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
        Command::Event {
            run,
            name,
            payload,
            idempotency_key,
        } => {
            let payload = match payload {
                Some(raw) => serde_json::from_str::<serde_json::Value>(&raw)
                    .with_context(|| format!("invalid --payload JSON: {raw}"))?,
                None => serde_json::Value::Null,
            };
            // Never create a database for an unknown run.
            if !cli.db.exists() {
                bail!("database not found: {}", cli.db.display());
            }
            let store = SqliteStore::open(&cli.db)?;
            let event = store.append_workflow_event(NewWorkflowEvent {
                run_id: run,
                name,
                // The CLI is an external writer; source is never user-chosen.
                source: EventSource::External,
                payload,
                emitted_by: None,
                ts: SystemClock.now(),
                idempotency_key,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&event)?);
            } else {
                println!(
                    "event run={} seq={} name={} source={} external={}",
                    event.run_id,
                    event.seq,
                    event.name,
                    event.source.as_str(),
                    event.external
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Events {
            run,
            follow,
            after,
            name,
            source,
        } => {
            // Read-only: `events` must never take a write lock or create a
            // database implicitly.
            let store = SqliteStore::open_read_only(&cli.db)?;
            let mut record = store
                .load_run(&run)?
                .with_context(|| format!("run not found: {run}"))?;
            let mut last_seq = after;
            if !follow {
                let events = store.workflow_events(
                    &run,
                    &WorkflowEventQuery {
                        name: name.clone(),
                        source: source.map(EventSourceArg::as_source),
                        after_seq: last_seq,
                    },
                )?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&events)?);
                } else {
                    for event in events {
                        println!(
                            "{} {} {} {} emittedBy={} {}",
                            event.seq,
                            event.ts,
                            event.source.as_str(),
                            event.name,
                            event.emitted_by.as_deref().unwrap_or("-"),
                            event.payload
                        );
                    }
                }
                return Ok(ExitCode::SUCCESS);
            }
            loop {
                let events = store.workflow_events(
                    &run,
                    &WorkflowEventQuery {
                        name: name.clone(),
                        source: source.map(EventSourceArg::as_source),
                        after_seq: last_seq,
                    },
                )?;
                let count = events.len();
                for event in events {
                    last_seq = Some(event.seq);
                    if json {
                        println!("{}", serde_json::to_string(&event)?);
                    } else {
                        println!(
                            "{} {} {} {} emittedBy={} {}",
                            event.seq,
                            event.ts,
                            event.source.as_str(),
                            event.name,
                            event.emitted_by.as_deref().unwrap_or("-"),
                            event.payload
                        );
                    }
                }
                if let Some(current) = store.load_run(&run)? {
                    record = current;
                }
                if record.status.is_terminal() && count == 0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Audit {
            command: AuditCommand::Verify { run },
        } => {
            let (engine, store) = build_runtime(&cli, None, None)?;
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
                    if let Err(err) =
                        ostinato_domain::verify_state_anchor(&events, &record.state_hash)
                    {
                        eprintln!(
                            "AUDIT FAILED run={run}: execution state is not anchored in the audit chain: {err}"
                        );
                        return Ok(ExitCode::FAILURE);
                    }
                    let trace_failures: Vec<_> = store
                        .verify_agent_traces(&run)?
                        .into_iter()
                        .filter(|item| !item.ok)
                        .collect();
                    if !trace_failures.is_empty() {
                        for item in trace_failures {
                            eprintln!(
                                "AUDIT FAILED run={run}: trace {} (step={} attempt={}): {}",
                                item.trace_id,
                                item.step,
                                item.attempt,
                                item.problems.join("; ")
                            );
                        }
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
        Command::Agents { run, step, verify } => {
            let (_, store) = build_runtime(&cli, None, None)?;
            if store.load_run(&run)?.is_none() {
                bail!("run not found: {run}");
            }
            let mut list_failed = false;
            let mut traces = match store.agent_traces(&run) {
                Ok(traces) => traces,
                Err(err) if verify => {
                    eprintln!("warning: cannot list traces: {err}");
                    list_failed = true;
                    Vec::new()
                }
                Err(err) => return Err(err.into()),
            };
            if let Some(step) = &step {
                traces.retain(|record| &record.trace.step == step);
            }
            let verifications = if verify {
                // Always verify the full run: a --step filter must not hide
                // tampering in another step.
                Some(store.verify_agent_traces(&run)?)
            } else {
                None
            };
            let verified = verifications
                .as_ref()
                .map(|items| items.iter().all(|item| item.ok));
            if json {
                if let Some(verifications) = &verifications {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "run": run,
                            "verified": verified,
                            "traces": traces,
                            "verifications": verifications,
                        }))?
                    );
                } else {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "run": run,
                            "traces": traces,
                        }))?
                    );
                }
            } else {
                if !list_failed {
                    println!(
                        "{:<6} {:<20} {:<5} {:<7} {:<9} {:<10} {:<12} {:<6} {:<8} {:<9} TRACE",
                        "ID",
                        "STEP",
                        "ROUND",
                        "ATTEMPT",
                        "STATUS",
                        "PROVIDER",
                        "MODEL",
                        "EXIT",
                        "DUR(ms)",
                        "ARTIFACTS"
                    );
                }
                for record in &traces {
                    let trace = &record.trace;
                    println!(
                        "{:<6} {:<20} {:<5} {:<7} {:<9} {:<10} {:<12} {:<6} {:<8} {:<9} {}",
                        record.id,
                        trace.step,
                        trace.round,
                        trace.attempt,
                        if trace.success {
                            "ok"
                        } else if trace.timed_out {
                            "timeout"
                        } else {
                            "failed"
                        },
                        trace.provider,
                        trace.model.as_deref().unwrap_or("-"),
                        trace
                            .exit_code
                            .map(|code| code.to_string())
                            .unwrap_or_else(|| "-".to_owned()),
                        trace.duration_ms,
                        trace.artifacts.len(),
                        record.trace_hash
                    );
                }
                if traces.is_empty() && !list_failed {
                    eprintln!("no agent traces matched");
                }
                if let Some(verifications) = &verifications {
                    for item in verifications {
                        if item.ok {
                            println!(
                                "OK trace={} step={} attempt={}",
                                item.trace_id, item.step, item.attempt
                            );
                        } else {
                            eprintln!(
                                "TRACE FAILED trace={} step={} attempt={}: {}",
                                item.trace_id,
                                item.step,
                                item.attempt,
                                item.problems.join("; ")
                            );
                        }
                    }
                }
            }
            if verified == Some(false) {
                Ok(ExitCode::FAILURE)
            } else {
                Ok(ExitCode::SUCCESS)
            }
        }
        Command::Artifacts {
            run,
            step,
            trace,
            round,
            attempt,
            extract,
            force,
        } => {
            let (_, store) = build_runtime(&cli, None, None)?;
            if store.load_run(&run)?.is_none() {
                bail!("run not found: {run}");
            }
            let traces = store.agent_traces(&run)?;
            let selected: Vec<&ostinato_domain::AgentTraceRecord> = traces
                .iter()
                .filter(|record| {
                    step.as_ref().is_none_or(|step| &record.trace.step == step)
                        && trace.is_none_or(|trace| record.id == trace)
                        && round.is_none_or(|round| record.trace.round == round)
                        && attempt.is_none_or(|attempt| record.trace.attempt == attempt)
                })
                .collect();
            let selected_ids: BTreeMap<i64, &ostinato_domain::AgentTraceRecord> =
                selected.iter().map(|record| (record.id, *record)).collect();
            if let Some(trace) = trace
                && !selected_ids.contains_key(&trace)
            {
                bail!("agent trace {trace} not found for run {run}");
            }
            let mut artifacts = store.agent_artifacts(&run, step.as_deref())?;
            artifacts.retain(|artifact| selected_ids.contains_key(&artifact.trace_id));
            let mut extracted = Vec::new();
            if let Some(directory) = &extract {
                if directory.exists()
                    && !force
                    && std::fs::read_dir(directory)
                        .map(|mut entries| entries.next().is_some())
                        .unwrap_or(false)
                {
                    bail!(
                        "extraction directory {} is not empty; pass --force to write into it",
                        directory.display()
                    );
                }
                let namespace = selected_ids.len() > 1;
                for artifact in &artifacts {
                    if artifact.kind == "deleted" {
                        continue;
                    }
                    let Some(record) = selected_ids.get(&artifact.trace_id) else {
                        continue;
                    };
                    let root = if namespace {
                        directory
                            .join(sanitize_path_component(&record.trace.step))
                            .join(format!(
                                "r{}-a{}-t{}",
                                record.trace.round, record.trace.attempt, record.id
                            ))
                    } else {
                        directory.clone()
                    };
                    let path = ostinato_adapters::write_artifact(
                        &root,
                        &artifact.name,
                        &artifact.content,
                        None,
                        force,
                    )
                    .map_err(anyhow::Error::msg)?;
                    extracted.push(path.display().to_string());
                }
            }
            if json {
                let items: Vec<serde_json::Value> = artifacts
                    .iter()
                    .map(|artifact| {
                        let record = selected_ids.get(&artifact.trace_id);
                        serde_json::json!({
                            "traceId": artifact.trace_id,
                            "step": record.map(|record| record.trace.step.clone()),
                            "round": record.map(|record| record.trace.round),
                            "attempt": record.map(|record| record.trace.attempt),
                            "name": artifact.name,
                            "kind": artifact.kind,
                            "sha256": artifact.sha256,
                            "size": artifact.size,
                            "storedSize": artifact.stored_size,
                            "truncated": artifact.truncated,
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "run": run,
                        "artifacts": items,
                        "extracted": extracted,
                    }))?
                );
            } else {
                println!(
                    "{:<8} {:<20} {:<7} {:<9} {:<9} {:<9} NAME",
                    "TRACE", "STEP", "ROUND", "ATTEMPT", "KIND", "SIZE"
                );
                for artifact in &artifacts {
                    let record = selected_ids.get(&artifact.trace_id);
                    println!(
                        "{:<8} {:<20} {:<7} {:<9} {:<9} {:<9} {}",
                        artifact.trace_id,
                        record.map_or("-", |record| record.trace.step.as_str()),
                        record.map_or(0, |record| record.trace.round),
                        record.map_or(0, |record| record.trace.attempt),
                        artifact.kind,
                        if artifact.truncated {
                            format!("{}*", artifact.size)
                        } else {
                            artifact.size.to_string()
                        },
                        display_safe(&artifact.name)
                    );
                }
                if artifacts.is_empty() {
                    eprintln!("no artifacts matched");
                }
                for path in &extracted {
                    println!("extracted {}", display_safe(path));
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
                    attempt: 0,
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
                    inputs: Vec::new(),
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
                            "gatewayFailures": metrics.gateway_failures,
                            "gatewayQuorumShort": metrics.gateway_quorum_short,
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
                    println!("gateway_failures  {}", metrics.gateway_failures);
                    println!("gateway_quorum_short {}", metrics.gateway_quorum_short);
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
                    println!(
                        "ostinato_gateway_failures_total {}",
                        metrics.gateway_failures
                    );
                    println!(
                        "ostinato_gateway_quorum_short_total {}",
                        metrics.gateway_quorum_short
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
                            "gatewayFailures": metrics.gateway_failures,
                            "gatewayQuorumShort": metrics.gateway_quorum_short,
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

fn display_safe(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_control() {
            out.push_str(&format!("\\x{:02x}", u32::from(ch)));
        } else {
            out.push(ch);
        }
    }
    out
}

fn sanitize_path_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len().min(64));
    for ch in value.chars().take(64) {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() || out == "." || out == ".." {
        "step".to_owned()
    } else {
        out
    }
}

fn find_in_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(program);
        if candidate.is_file() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(metadata) = candidate.metadata()
                    && metadata.permissions().mode() & 0o111 != 0
                {
                    return Some(candidate);
                }
            }
            #[cfg(not(unix))]
            {
                return Some(candidate);
            }
        }
    }
    None
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
            workspace: Arc::new(LocalWorkspace::new(None)),
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
        if value.starts_with('@') {
            bail!(
                "`-p @file` is not supported; use --input <file.json|yaml> to unify data into \
                 the CUE root. `-p` is reserved for scalar key=value parameters"
            );
        }
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

fn data_inputs(paths: &[PathBuf]) -> Vec<DataInput> {
    paths.iter().cloned().map(DataInput::new).collect()
}

fn resolve_output_format(
    requested: Option<DataFormatArg>,
    output: Option<&Path>,
) -> DocumentFormat {
    if let Some(requested) = requested {
        return requested.as_document();
    }
    match output
        .and_then(Path::extension)
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("yaml" | "yml") => DocumentFormat::Yaml,
        _ => DocumentFormat::Json,
    }
}

fn unify_export_documents(
    documents: &[serde_json::Value],
    origin: &Path,
) -> Result<serde_json::Value> {
    match documents {
        [] => bail!("{}: no documents to export", origin.display()),
        [document] => Ok(document.clone()),
        documents => data::unify_objects(documents, origin).map_err(Into::into),
    }
}

fn select_data_values(
    value: &serde_json::Value,
    expressions: &[String],
) -> Result<Vec<serde_json::Value>> {
    if expressions.is_empty() {
        return Ok(vec![value.clone()]);
    }
    let mut values = Vec::with_capacity(expressions.len());
    for expression in expressions {
        values.push(data::select_json_path(value, expression)?.clone());
    }
    Ok(values)
}

fn write_values(
    output: Option<&Path>,
    out: Option<DataFormatArg>,
    values: &[serde_json::Value],
    force: bool,
    protected: &[PathBuf],
) -> Result<()> {
    let format = resolve_output_format(out, output);
    let codec = JsonYamlCodec;
    let content = codec
        .encode_stream(format, values)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let sink = FileOutputSink;
    sink.write(
        output,
        &content,
        OutputOptions {
            force,
            skip_existing: false,
        },
        protected,
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(())
}

fn preflight_outputs(
    files: &[cue_import::PlannedFile],
    protected: &[PathBuf],
    force: bool,
    skip_existing: bool,
) -> Result<()> {
    for file in files {
        if file.path == Path::new("-") {
            continue;
        }
        let destination = std::fs::canonicalize(&file.path).unwrap_or_else(|_| file.path.clone());
        for input in protected {
            let input = std::fs::canonicalize(input).unwrap_or_else(|_| input.clone());
            if destination == input {
                bail!(
                    "refusing to write {}: it is one of the inputs",
                    file.path.display()
                );
            }
        }
        if let Ok(metadata) = std::fs::symlink_metadata(&file.path) {
            if metadata.file_type().is_symlink() {
                bail!(
                    "refusing to write {}: destination is a symbolic link",
                    file.path.display()
                );
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.nlink() > 1 {
                    bail!(
                        "refusing to write {}: destination is hard-linked",
                        file.path.display()
                    );
                }
            }
            if !force && !skip_existing {
                bail!(
                    "refusing to overwrite {}: pass --force or --skip-existing",
                    file.path.display()
                );
            }
        }
    }
    Ok(())
}

fn exit_for_status(status: RunStatus) -> ExitCode {
    match status {
        RunStatus::Succeeded => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

fn tool_command_help(name: Option<&str>, paths: &[PathBuf]) -> ExitCode {
    let load =
        |paths: &[PathBuf]| ostinato_adapters::tool_command_metadata(paths).unwrap_or_default();
    if let Some(name) = name {
        let metadata_paths = if paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            paths.to_vec()
        };
        let infos = load(&metadata_paths);
        if let Some(info) = infos.iter().find(|info| info.name == name) {
            print_tool_command_detail(info);
            return ExitCode::SUCCESS;
        }
        // `cmd -h <path>` lists that package when `<path>` is not a command.
        if paths.is_empty() && Path::new(name).exists() {
            let infos = load(&[PathBuf::from(name)]);
            print_tool_command_listing(&infos);
            return ExitCode::SUCCESS;
        }
        eprintln!("Unknown cmd command: {name}");
        eprintln!("Run 'ostinato cmd --help' for known subcommands.");
        return ExitCode::from(1);
    }
    let metadata_paths = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    let infos = load(&metadata_paths);
    print_tool_command_listing(&infos);
    ExitCode::SUCCESS
}

fn print_tool_command_flags() {
    println!();
    println!("Flags:");
    println!("  -h, --help             Show command help");
    println!("  -p, --param <PARAM>    Set a @tag parameter override");
}

fn print_tool_command_detail(info: &ostinato_adapters::ToolCommandInfo) {
    if let Some(long) = &info.long {
        println!("{long}\n");
    } else if let Some(short) = &info.short {
        println!("{short}\n");
    }
    let prefixed = info
        .usage
        .as_deref()
        .filter(|usage| usage.starts_with(&format!("{} ", info.name)))
        .unwrap_or(&info.name);
    println!("Usage:\n  ostinato cmd {prefixed} [flags]");
    print_tool_command_flags();
}

fn print_tool_command_listing(infos: &[ostinato_adapters::ToolCommandInfo]) {
    println!("Usage:");
    println!("  ostinato cmd [flags] <name> [inputs]");
    println!("  ostinato cmd [command]\n");
    println!("Available Commands:");
    let width = infos.iter().map(|info| info.name.len()).max().unwrap_or(0);
    for info in infos {
        println!(
            "  {:<width$}  {}",
            info.name,
            info.short.as_deref().unwrap_or("")
        );
    }
    print_tool_command_flags();
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
    let has_rounds = report.steps.iter().any(|step| step.round > 0);
    if has_rounds {
        println!(
            "{:<24} {:<10} {:<6} {:<8} {:<6}",
            "STEP", "STATUS", "VISITS", "ATTEMPTS", "ROUND"
        );
        for step in &report.steps {
            println!(
                "{:<24} {:<10} {:<6} {:<8} {:<6}",
                step.name,
                step.status.as_str(),
                step.visits,
                step.attempts,
                step.round
            );
        }
    } else {
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
    let has_rounds = steps.iter().any(|step| step.round > 0);
    if has_rounds {
        println!(
            "{:<24} {:<10} {:<6} {:<8} {:<6}",
            "STEP", "STATUS", "VISITS", "ATTEMPTS", "ROUND"
        );
        for step in steps {
            println!(
                "{:<24} {:<10} {:<6} {:<8} {:<6}",
                step.name,
                step.status.as_str(),
                step.visits,
                step.attempts,
                step.round
            );
        }
    } else {
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
                .or_else(|| {
                    gateway.join.map(|mode| match mode {
                        JoinMode::NOf => {
                            format!("join: n_of (n={})", gateway.n.unwrap_or(0))
                        }
                        JoinMode::Expression => "join: expression".to_owned(),
                        _ => format!("join: {}", mode.as_str()),
                    })
                })
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
            |_| "not configured (command steps need agent.command)".to_owned(),
            |_| "configured".to_owned()
        )
    );
    for provider in ["claude", "codex", "opencode"] {
        println!(
            "agent provider {} {}",
            provider,
            find_in_path(provider)
                .map_or_else(|| "not found".to_owned(), |path| path.display().to_string())
        );
    }
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
