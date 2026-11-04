#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use ostinato_adapters::SqliteStore;
use ostinato_application::{Completion, Store};
use ostinato_domain::{
    NewEvent, NewImprovement, NewTrial, RetryPolicy, RunRecord, RunStatus, StepKind, StepRecord,
    StepSpec, StepStatus, TrialStatus, WorkflowSpec, audit,
};
use serde_json::json;

fn run_record(id: &str) -> RunRecord {
    let mut steps = BTreeMap::new();
    steps.insert(
        "a".to_owned(),
        StepSpec {
            name: "a".to_owned(),
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
        },
    );
    let spec = WorkflowSpec {
        name: "store".to_owned(),
        max_parallel: 1,
        steps,
    };
    let params = json!({"x": 1});
    let state_hash = audit::state_digest(&spec, &params, "test.cue", 0, &[]);
    RunRecord {
        id: id.to_owned(),
        workflow_name: "store".to_owned(),
        source_file: "test.cue".to_owned(),
        spec,
        params,
        status: RunStatus::Running,
        resume_index: 0,
        parent_run_id: None,
        error: None,
        created_at: "2026-01-01T00:00:00Z".to_owned(),
        updated_at: "2026-01-01T00:00:00Z".to_owned(),
        audit_head: audit::GENESIS_HASH.to_owned(),
        state_hash,
    }
}

#[test]
fn sqlite_store_roundtrip_is_atomic_and_hash_chained() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = SqliteStore::open(&directory.path().join("db.sqlite")).expect("open");
    let run = run_record("run-1");
    store.create_run(&run).expect("create");

    let loaded = store.load_run("run-1").expect("load").expect("present");
    assert_eq!(loaded.spec.name, "store");
    assert_eq!(loaded.params["x"], 1);

    store
        .append_event(
            "run-1",
            NewEvent::new("run.started", None, json!({"workflow": "store"})),
            "2026-01-01T00:00:00Z",
        )
        .expect("started");

    let step = StepRecord {
        run_id: "run-1".to_owned(),
        name: "a".to_owned(),
        status: StepStatus::Succeeded,
        visits: 1,
        attempts: 1,
        output: Some(json!({"stdout": "hi", "score": 1.0})),
        error: None,
        updated_at: "2026-01-01T00:00:01Z".to_owned(),
        round: 0,
    };
    let events = store
        .complete(Completion {
            run_id: "run-1".to_owned(),
            resume_index: 1,
            status: RunStatus::Succeeded,
            error: None,
            updated_at: "2026-01-01T00:00:01Z".to_owned(),
            steps: vec![step],
            events: vec![NewEvent::new(
                "run.succeeded",
                None,
                json!({"resumeIndex": 1}),
            )],
        })
        .expect("complete");
    assert_eq!(events.len(), 2);

    let loaded = store.load_run("run-1").expect("load").expect("present");
    assert_eq!(loaded.status, RunStatus::Succeeded);
    assert_eq!(loaded.resume_index, 1);
    let steps = store.steps("run-1").expect("steps");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].status, StepStatus::Succeeded);

    let events = store.events("run-1").expect("events");
    assert_eq!(events.len(), 3);
    audit::verify_chain(&events).expect("audit chain");
    let loaded = store.load_run("run-1").expect("load").expect("present");
    let steps = store.steps("run-1").expect("steps");
    assert_eq!(
        loaded.state_hash,
        audit::state_digest(
            &loaded.spec,
            &loaded.params,
            &loaded.source_file,
            loaded.resume_index,
            &steps,
        )
    );

    store
        .record_trial(&NewTrial {
            run_id: "run-1".to_owned(),
            node_index: 0,
            step: "a".to_owned(),
            round: 0,
            attempt: 0,
            status: TrialStatus::Succeeded,
            score: 1.0,
            duration_ms: 3,
            input_hash: "in".to_owned(),
            output_hash: "out".to_owned(),
            input: json!({"kind": "noop"}),
            output: Some(json!({"score": 1.0})),
            error: None,
            created_at: "2026-01-01T00:00:01Z".to_owned(),
        })
        .expect("trial");
    assert_eq!(store.trials("run-1").expect("trials").len(), 1);

    store
        .record_improvement(&NewImprovement {
            run_id: "run-1".to_owned(),
            parent_id: None,
            round: 0,
            diagnosis: "ok".to_owned(),
            proposal: json!({"params_patch": {"x": 2}}),
            created_at: "2026-01-01T00:00:02Z".to_owned(),
        })
        .expect("improvement");
    assert_eq!(store.improvements("run-1").expect("improvements").len(), 1);

    let metrics = store.metrics(Some("run-1")).expect("metrics");
    assert_eq!(metrics.runs_total, 1);
    assert_eq!(metrics.runs_succeeded, 1);
    assert_eq!(metrics.attempts_total, 1);
}

#[cfg(unix)]
#[test]
fn database_and_wal_files_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("db.sqlite");
    let store = SqliteStore::open(&path).expect("open");
    let run = run_record("private");
    store.create_run(&run).expect("create");
    store
        .append_event(
            "private",
            NewEvent::new("run.started", None, json!({})),
            "2026-01-01T00:00:00Z",
        )
        .expect("event");
    for suffix in ["", "-wal", "-shm"] {
        let candidate = PathBuf::from(format!("{}{suffix}", path.display()));
        if candidate.exists() {
            let mode = std::fs::metadata(&candidate)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "{} mode is {mode:o}", candidate.display());
        }
    }
}

#[test]
fn reopened_store_does_not_repair_blank_integrity_hashes() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("db.sqlite");
    {
        let store = SqliteStore::open(&path).expect("open");
        let run = run_record("no-repair");
        store.create_run(&run).expect("create");
    }
    let connection = rusqlite::Connection::open(&path).expect("open sqlite");
    connection
        .execute(
            "UPDATE runs SET state_hash = '', audit_head = '' WHERE id = 'no-repair'",
            [],
        )
        .expect("clear hashes");
    drop(connection);

    let store = SqliteStore::open(&path).expect("reopen");
    let run = store.load_run("no-repair").expect("load").expect("present");
    assert!(run.state_hash.is_empty());
    assert!(run.audit_head.is_empty());
}

#[test]
fn sqlite_store_roundtrips_step_rounds() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = SqliteStore::open(&directory.path().join("db.sqlite")).expect("open");
    let run = run_record("run-round");
    store.create_run(&run).expect("create");
    store
        .append_event(
            "run-round",
            NewEvent::new("run.started", None, json!({"workflow": "store"})),
            "2026-01-01T00:00:00Z",
        )
        .expect("started");
    let step = StepRecord {
        run_id: "run-round".to_owned(),
        name: "a".to_owned(),
        status: StepStatus::Succeeded,
        visits: 2,
        attempts: 1,
        output: Some(json!({"round": 1})),
        error: None,
        updated_at: "2026-01-01T00:00:02Z".to_owned(),
        round: 1,
    };
    store
        .complete(Completion {
            run_id: "run-round".to_owned(),
            resume_index: 2,
            status: RunStatus::Running,
            error: None,
            updated_at: "2026-01-01T00:00:02Z".to_owned(),
            steps: vec![step],
            events: vec![NewEvent::new(
                "step.succeeded",
                Some("a".to_owned()),
                json!({"round": 1}),
            )],
        })
        .expect("complete");
    let steps = store.steps("run-round").expect("steps");
    assert_eq!(steps[0].round, 1);
    let loaded = store.load_run("run-round").expect("load").expect("present");
    assert_eq!(
        loaded.state_hash,
        audit::state_digest(
            &loaded.spec,
            &loaded.params,
            &loaded.source_file,
            loaded.resume_index,
            &steps,
        )
    );
    audit::verify_chain(&store.events("run-round").expect("events")).expect("audit");
}

#[test]
fn opening_a_v0_database_migrates_steps_round_and_backfills_integrity() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("v1.sqlite");
    let run = run_record("v1-run");
    {
        let connection = rusqlite::Connection::open(&path).expect("open v1");
        connection
            .execute_batch(
                "CREATE TABLE runs (
                    id TEXT PRIMARY KEY, workflow_name TEXT NOT NULL, source_file TEXT NOT NULL,
                    spec_json TEXT NOT NULL, params_json TEXT NOT NULL, status TEXT NOT NULL,
                    resume_index INTEGER NOT NULL, parent_run_id TEXT, error TEXT,
                    created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
                    audit_head TEXT NOT NULL DEFAULT '', state_hash TEXT NOT NULL DEFAULT ''
                );
                 CREATE TABLE steps (
                    run_id TEXT NOT NULL, name TEXT NOT NULL, status TEXT NOT NULL,
                    visits INTEGER NOT NULL, attempts INTEGER NOT NULL, output_json TEXT,
                    error TEXT, updated_at TEXT NOT NULL, PRIMARY KEY (run_id, name)
                );
                 PRAGMA user_version = 0;",
            )
            .expect("v1 schema");
        connection
            .execute(
                "INSERT INTO runs
                 (id, workflow_name, source_file, spec_json, params_json, status, resume_index,
                  parent_run_id, error, created_at, updated_at, audit_head, state_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, ?8, ?9, '', '')",
                rusqlite::params![
                    run.id,
                    run.workflow_name,
                    run.source_file,
                    serde_json::to_string(&run.spec).expect("spec"),
                    serde_json::to_string(&run.params).expect("params"),
                    run.status.as_str(),
                    run.resume_index,
                    run.created_at,
                    run.updated_at,
                ],
            )
            .expect("insert v1 run");
    }
    let store = SqliteStore::open(&path).expect("migrate");
    let loaded = store.load_run("v1-run").expect("load").expect("present");
    assert!(!loaded.state_hash.is_empty());
    let connection = rusqlite::Connection::open(&path).expect("open migrated");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("version");
    assert_eq!(version, 2);
    let has_round: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('steps') WHERE name = 'round'",
            [],
            |row| row.get(0),
        )
        .expect("round column");
    assert_eq!(has_round, 1);
}

#[test]
fn resume_run_is_atomic_and_refuses_terminal_runs() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = SqliteStore::open(&directory.path().join("db.sqlite")).expect("open");
    let run = run_record("run-resume");
    store.create_run(&run).expect("create");
    store
        .append_event(
            "run-resume",
            NewEvent::new("run.started", None, json!({"workflow": "store"})),
            "2026-01-01T00:00:00Z",
        )
        .expect("started");
    store
        .complete(Completion {
            run_id: "run-resume".to_owned(),
            resume_index: 1,
            status: RunStatus::Succeeded,
            error: None,
            updated_at: "2026-01-01T00:00:01Z".to_owned(),
            steps: Vec::new(),
            events: vec![NewEvent::new(
                "run.succeeded",
                None,
                json!({"resumeIndex": 1}),
            )],
        })
        .expect("complete");
    let refused = store.resume_run(
        "run-resume",
        0,
        "2026-01-01T00:00:02Z",
        NewEvent::new("run.resumed", None, json!({"resumeIndex": 0})),
    );
    assert!(matches!(
        refused,
        Err(ostinato_application::PortError::Cancelled(_))
    ));
    let loaded = store
        .load_run("run-resume")
        .expect("load")
        .expect("present");
    assert_eq!(loaded.status, RunStatus::Succeeded);
}

#[test]
fn create_run_with_event_writes_the_start_anchor_atomically() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = SqliteStore::open(&directory.path().join("db.sqlite")).expect("open");
    let run = run_record("run-start");
    let event = store
        .create_run_with_event(
            &run,
            NewEvent::new(
                "run.started",
                None,
                json!({"workflow": "store", "stateHash": run.state_hash}),
            ),
            "2026-01-01T00:00:00Z",
        )
        .expect("create with event");
    let loaded = store.load_run("run-start").expect("load").expect("present");
    assert_eq!(loaded.audit_head, event.hash);
    let events = store.events("run-start").expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "run.started");
    audit::verify_chain(&events).expect("chain");
    audit::verify_state_anchor(&events, &loaded.state_hash).expect("anchor");
}

fn sample_trace(
    run_id: &str,
) -> (
    ostinato_domain::AgentTrace,
    Vec<ostinato_domain::CapturedArtifact>,
) {
    let artifact = ostinato_domain::CapturedArtifact {
        name: "out/report.txt".to_owned(),
        kind: "declared".to_owned(),
        bytes: b"report\n".to_vec(),
        truncated: false,
        original_size: 7,
        mode: Some(0o644),
    };
    let trace = ostinato_domain::AgentTrace {
        run_id: run_id.to_owned(),
        step: "review".to_owned(),
        round: 0,
        attempt: 0,
        definition: Some("reviewer".to_owned()),
        provider: "command".to_owned(),
        model: Some("m".to_owned()),
        prompt: "hello".to_owned(),
        prompt_sha256: ostinato_domain::sha256_hex(b"hello"),
        system: Some("system".to_owned()),
        argv: vec!["fake".to_owned()],
        env_keys: vec!["OSTINATO_STEP".to_owned()],
        env_hashes: BTreeMap::new(),
        cwd: "/tmp".to_owned(),
        isolation: "none".to_owned(),
        isolation_path: None,
        isolation_base: None,
        started_at: "2026-01-01T00:00:00Z".to_owned(),
        finished_at: "2026-01-01T00:00:01Z".to_owned(),
        duration_ms: 1000,
        success: true,
        exit_code: Some(0),
        timed_out: false,
        signal: None,
        pid: Some(1),
        program: Some("fake".to_owned()),
        ostinato_version: "test".to_owned(),
        stdout_truncated: false,
        stderr_truncated: false,
        stdin_sha256: Some(ostinato_domain::sha256_hex(b"hello")),
        error: None,
        stdout: "ok".to_owned(),
        stderr: String::new(),
        transcript: None,
        artifacts: vec![artifact.meta()],
        capture_warnings: Vec::new(),
        capture_error: None,
        cleanup_error: None,
    };
    (trace, vec![artifact])
}

#[test]
fn agent_traces_roundtrip_and_verify() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = SqliteStore::open(&directory.path().join("db.sqlite")).expect("open");
    let run = run_record("trace-run");
    store.create_run(&run).expect("create");
    let (trace, artifacts) = sample_trace("trace-run");
    let record = store
        .record_agent_trace(&trace, &artifacts)
        .expect("record");
    assert!(record.id > 0);
    assert_eq!(record.trace_hash, trace.trace_hash());

    let traces = store.agent_traces("trace-run").expect("list");
    assert_eq!(traces.len(), 1);
    assert_eq!(traces[0].trace.prompt, "hello");
    let stored = store
        .agent_artifacts("trace-run", Some("review"))
        .expect("artifacts");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].content, b"report\n");
    assert_eq!(stored[0].sha256, artifacts[0].sha256());

    let verified = store.verify_agent_traces("trace-run").expect("verify");
    assert_eq!(verified.len(), 1);
    assert!(verified[0].ok, "{verified:?}");

    // The anchoring event is in the audit chain.
    let events = store.events("trace-run").expect("events");
    assert!(events.iter().any(|event| event.kind == "agent.traced"));
    audit::verify_chain(&events).expect("chain");
}

#[test]
fn agent_trace_and_artifact_tampering_are_detected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let store = SqliteStore::open(&db).expect("open");
    let run = run_record("tamper-run");
    store.create_run(&run).expect("create");
    let (trace, artifacts) = sample_trace("tamper-run");
    store
        .record_agent_trace(&trace, &artifacts)
        .expect("record");

    let connection = rusqlite::Connection::open(&db).expect("second connection");
    connection
        .execute(
            "UPDATE agent_traces SET trace_json = '{}' WHERE run_id = 'tamper-run'",
            [],
        )
        .expect("tamper trace");
    let verified = store.verify_agent_traces("tamper-run").expect("verify");
    assert!(!verified[0].ok);
    assert!(
        verified[0]
            .problems
            .iter()
            .any(|problem| problem.contains("hash") || problem.contains("invalid")),
        "{verified:?}"
    );

    // Restore the trace row and tamper with the artifact blob instead.
    store
        .record_agent_trace(&trace, &artifacts)
        .expect("re-record");
    // Column tampering is caught even when the JSON/hash is intact.
    connection
        .execute(
            "UPDATE agent_traces SET step = 'other' WHERE run_id = 'tamper-run'",
            [],
        )
        .expect("tamper column");
    let verified = store.verify_agent_traces("tamper-run").expect("verify");
    assert!(
        verified.iter().any(|item| item
            .problems
            .iter()
            .any(|problem| problem.contains("columns"))),
        "{verified:?}"
    );

    connection
        .execute(
            "UPDATE agent_artifacts SET content = ?1 WHERE trace_id = (SELECT MAX(id) FROM agent_traces)",
            [b"evil".as_slice()],
        )
        .expect("tamper artifact");
    let verified = store.verify_agent_traces("tamper-run").expect("verify");
    assert!(
        verified.iter().any(|item| item
            .problems
            .iter()
            .any(|problem| problem.contains("content"))),
        "{verified:?}"
    );
}

#[test]
fn deleting_an_agent_event_breaks_trace_anchoring() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let store = SqliteStore::open(&db).expect("open");
    let run = run_record("anchor-run");
    store.create_run(&run).expect("create");
    let (trace, artifacts) = sample_trace("anchor-run");
    store
        .record_agent_trace(&trace, &artifacts)
        .expect("record");
    let connection = rusqlite::Connection::open(&db).expect("second connection");
    connection
        .execute(
            "DELETE FROM events WHERE run_id = 'anchor-run' AND kind = 'agent.traced'",
            [],
        )
        .expect("delete event");
    let verified = store.verify_agent_traces("anchor-run").expect("verify");
    assert!(!verified[0].ok);
    assert!(
        verified[0]
            .problems
            .iter()
            .any(|problem| problem.contains("not anchored")),
        "{verified:?}"
    );
}
