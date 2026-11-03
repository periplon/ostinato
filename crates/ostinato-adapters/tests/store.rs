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
