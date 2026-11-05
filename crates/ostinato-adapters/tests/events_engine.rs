#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ostinato_adapters::{CommandAgent, LocalWorkspace, ProcessExecutor, SqliteStore};
use ostinato_application::{
    Clock, Engine, EngineDeps, EngineOptions, IdGenerator, LoadedWorkflow, NoTelemetry, Store,
    WorkflowEventQuery,
};
use ostinato_domain::{
    AwaitSpec, CycleSpec, EventSource, ExecSpec, NewWorkflowEvent, RetryPolicy, RunStatus,
    StepKind, StepSpec, WorkflowSpec,
};
use serde_json::{Value, json};

struct FixedIds(String);

impl IdGenerator for FixedIds {
    fn new_run_id(&self) -> String {
        self.0.clone()
    }
}

struct ScriptedClock {
    now: String,
    store: Arc<SqliteStore>,
    run_id: String,
    inject: Option<(String, Value)>,
    marker: Option<PathBuf>,
    db_path: Option<PathBuf>,
    corrupt_event: bool,
    fired: AtomicBool,
}

impl ScriptedClock {
    fn new(store: Arc<SqliteStore>, run_id: &str) -> Self {
        Self {
            now: "2026-01-01T00:00:00Z".to_owned(),
            store,
            run_id: run_id.to_owned(),
            inject: None,
            marker: None,
            db_path: None,
            corrupt_event: false,
            fired: AtomicBool::new(false),
        }
    }

    fn with_event(mut self, name: &str, payload: Value) -> Self {
        self.inject = Some((name.to_owned(), payload));
        self
    }

    fn with_marker(mut self, marker: PathBuf) -> Self {
        self.marker = Some(marker);
        self
    }

    fn with_corrupt_event(mut self, db_path: PathBuf) -> Self {
        self.corrupt_event = true;
        self.db_path = Some(db_path);
        self
    }
}

impl Clock for ScriptedClock {
    fn now(&self) -> String {
        self.now.clone()
    }

    fn sleep(&self, duration: Duration) {
        if !self.fired.swap(true, Ordering::SeqCst) {
            if let Some(marker) = &self.marker {
                assert!(
                    marker.exists(),
                    "marker was not written before the await slept"
                );
            }
            if let Some((name, payload)) = &self.inject {
                self.store
                    .append_workflow_event(NewWorkflowEvent::new(
                        self.run_id.clone(),
                        name.clone(),
                        EventSource::External,
                        payload.clone(),
                        None,
                        self.now.clone(),
                    ))
                    .expect("inject event");
            }
            if self.corrupt_event {
                let path = self.db_path.as_ref().expect("db path");
                let connection = rusqlite::Connection::open(path).expect("open raw");
                connection
                    .execute(
                        "INSERT INTO workflow_events
                         (run_id, seq, name, source, payload_json, emitted_by, ts, external)
                         VALUES (?1, 0, 'go', 'external', '{', NULL, ?2, 1)",
                        rusqlite::params![self.run_id, self.now],
                    )
                    .expect("insert malformed event");
            }
        }
        std::thread::sleep(duration);
    }
}

struct SleepClock;

impl Clock for SleepClock {
    fn now(&self) -> String {
        "2026-01-01T00:00:00Z".to_owned()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

fn engine(store: Arc<SqliteStore>, clock: Arc<dyn Clock>, run_id: &str) -> Engine {
    Engine::new(
        EngineDeps {
            store,
            executor: Arc::new(ProcessExecutor),
            agent: Arc::new(CommandAgent::new(None)),
            workspace: Arc::new(LocalWorkspace::new(None)),
            clock,
            ids: Arc::new(FixedIds(run_id.to_owned())),
            telemetry: Arc::new(NoTelemetry),
        },
        EngineOptions::default(),
    )
}

fn step(name: &str, kind: StepKind) -> StepSpec {
    StepSpec {
        name: name.to_owned(),
        kind,
        needs: Vec::new(),
        retry: RetryPolicy::default(),
        cycle: None,
        exec: None,
        agent: None,
        improve: None,
        gateway: None,
        await_spec: None,
        emit: Vec::new(),
        continue_on_error: false,
        timeout_ms: None,
    }
}

fn exec(name: &str, command: &str) -> StepSpec {
    let mut step = step(name, StepKind::Exec);
    step.exec = Some(ExecSpec {
        cmd: vec!["sh".to_owned(), "-c".to_owned(), command.to_owned()],
        cwd: None,
        env: BTreeMap::new(),
        timeout_ms: None,
    });
    step
}

fn waiter(name: &str, event: &str, source: Option<EventSource>, timeout_ms: u64) -> StepSpec {
    let mut step = step(name, StepKind::Await);
    step.await_spec = Some(AwaitSpec {
        event: event.to_owned(),
        source,
        timeout_ms: Some(timeout_ms),
    });
    step
}

fn loaded(spec: WorkflowSpec) -> LoadedWorkflow {
    LoadedWorkflow {
        spec,
        params: json!({}),
        source_file: "events.cue".to_owned(),
        evaluator: "rust".to_owned(),
    }
}

fn workflow(steps: Vec<StepSpec>) -> WorkflowSpec {
    WorkflowSpec {
        name: "events".to_owned(),
        max_parallel: 4,
        steps: steps
            .into_iter()
            .map(|step| (step.name.clone(), step))
            .collect(),
    }
}

fn store(path: &std::path::Path) -> Arc<SqliteStore> {
    Arc::new(SqliteStore::open(path).expect("open store"))
}

#[test]
fn internal_emit_is_durable_and_broadcast_to_multiple_awaits() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = store(&directory.path().join("db.sqlite"));
    let mut emit = exec("emit", "true");
    emit.emit = vec![ostinato_domain::EmitSpec {
        name: "ready".to_owned(),
        external: false,
        payload: Some(json!({"value": 42})),
    }];
    let mut wait_a = waiter("wait_a", "ready", Some(EventSource::Internal), 1000);
    wait_a.needs = vec!["emit".to_owned()];
    let mut wait_b = waiter("wait_b", "ready", None, 1000);
    wait_b.needs = vec!["emit".to_owned()];
    let mut consume = exec("consume", "printf '%s' '{{ events.ready.payload.value }}'");
    consume.needs = vec!["wait_a".to_owned(), "wait_b".to_owned()];
    let engine = engine(store.clone(), Arc::new(SleepClock), "run-internal");

    let report = engine
        .start(&loaded(workflow(vec![emit, wait_a, wait_b, consume])))
        .expect("run");
    assert_eq!(report.status, RunStatus::Succeeded, "{:?}", report.error);

    let events = store
        .workflow_events("run-internal", &WorkflowEventQuery::default())
        .expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].source, EventSource::Internal);
    assert_eq!(events[0].name, "ready");
    let cursor_a = store
        .event_cursor("run-internal", "wait_a", "ready")
        .expect("cursor a")
        .expect("present");
    let cursor_b = store
        .event_cursor("run-internal", "wait_b", "ready")
        .expect("cursor b")
        .expect("present");
    assert_eq!(cursor_a.next_seq, 1);
    assert_eq!(cursor_b.next_seq, 1);
    let consume = report
        .steps
        .iter()
        .find(|step| step.name == "consume")
        .expect("consume step");
    assert_eq!(
        consume.output.as_ref().unwrap()["stdout"].as_str(),
        Some("42")
    );
    let audit = store.events("run-internal").expect("audit");
    assert!(audit.iter().any(|event| event.kind == "event.emitted"));
    assert_eq!(
        audit
            .iter()
            .filter(|event| event.kind == "event.received")
            .count(),
        2
    );
    ostinato_domain::audit::verify_chain(&audit).expect("chain");
}

#[test]
fn external_injection_is_visible_to_a_running_await() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = store(&directory.path().join("db.sqlite"));
    let clock = Arc::new(
        ScriptedClock::new(store.clone(), "run-external").with_event("go", json!({"ready": true})),
    );
    let engine = engine(store.clone(), clock, "run-external");
    let report = engine
        .start(&loaded(workflow(vec![waiter(
            "wait",
            "go",
            Some(EventSource::External),
            5000,
        )])))
        .expect("run");
    assert_eq!(report.status, RunStatus::Succeeded, "{:?}", report.error);
    let events = store
        .workflow_events("run-external", &WorkflowEventQuery::default())
        .expect("events");
    assert_eq!(events.len(), 1);
    assert!(events[0].external);
    assert_eq!(events[0].source, EventSource::External);
    let audit = store.events("run-external").expect("audit");
    assert!(audit.iter().any(|event| event.kind == "event.received"));
    assert!(audit.iter().any(|event| event.kind == "event.ingested"));
}

#[test]
fn await_timeout_fails_the_run_and_audits_event_timeout() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = store(&directory.path().join("db.sqlite"));
    let engine = engine(store.clone(), Arc::new(SleepClock), "run-timeout");
    let report = engine
        .start(&loaded(workflow(vec![waiter(
            "wait",
            "never",
            Some(EventSource::External),
            60,
        )])))
        .expect("run");
    assert_eq!(report.status, RunStatus::Failed);
    assert!(report.steps.iter().any(|step| {
        step.error
            .as_deref()
            .is_some_and(|error| error.contains("timed out"))
    }));
    let audit = store.events("run-timeout").expect("audit");
    assert!(audit.iter().any(|event| event.kind == "event.timeout"));
    ostinato_domain::audit::verify_chain(&audit).expect("chain");
}

#[test]
fn cancelling_a_long_await_returns_cancelled_promptly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = store(&directory.path().join("db.sqlite"));
    let engine = Arc::new(engine(
        store.clone(),
        Arc::new(SleepClock),
        "run-cancel-await",
    ));
    let started = std::time::Instant::now();
    let report = std::thread::scope(|scope| {
        let engine_for_cancel = Arc::clone(&engine);
        scope.spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            engine_for_cancel
                .cancel("run-cancel-await")
                .expect("cancel");
        });
        engine
            .start(&loaded(workflow(vec![waiter(
                "wait",
                "never",
                Some(EventSource::External),
                10_000,
            )])))
            .expect("run")
    });
    assert_eq!(report.status, RunStatus::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn resume_does_not_duplicate_emitted_events() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = store(&directory.path().join("db.sqlite"));
    let marker = directory.path().join("flag");
    let mut emit = exec("emit", "true");
    emit.emit = vec![ostinato_domain::EmitSpec {
        name: "note".to_owned(),
        external: false,
        payload: Some(json!({"n": 1})),
    }];
    let mut wait = waiter("wait", "note", Some(EventSource::Internal), 1000);
    wait.needs = vec!["emit".to_owned()];
    let mut fail_once = exec(
        "fail_once",
        &format!(
            "if [ -f {flag} ]; then exit 0; else touch {flag}; exit 1; fi",
            flag = marker.display()
        ),
    );
    fail_once.needs = vec!["wait".to_owned()];
    let engine = engine(store.clone(), Arc::new(SleepClock), "run-resume-events");

    let first = engine
        .start(&loaded(workflow(vec![emit, wait, fail_once])))
        .expect("first run");
    assert_eq!(first.status, RunStatus::Failed);
    let second = engine.resume("run-resume-events").expect("resume");
    assert_eq!(second.status, RunStatus::Succeeded, "{:?}", second.error);

    let events = store
        .workflow_events(
            "run-resume-events",
            &WorkflowEventQuery {
                name: Some("note".to_owned()),
                ..WorkflowEventQuery::default()
            },
        )
        .expect("events");
    assert_eq!(events.len(), 1, "resume must not re-emit a committed step");
    assert_eq!(
        store
            .event_cursor("run-resume-events", "wait", "note")
            .expect("cursor")
            .expect("present")
            .next_seq,
        1
    );
}

#[test]
fn await_source_filter_only_consumes_matching_events() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = store(&directory.path().join("db.sqlite"));
    let mut emit = exec("emit", "true");
    emit.emit = vec![ostinato_domain::EmitSpec {
        name: "only_internal".to_owned(),
        external: false,
        payload: Some(json!({"x": 1})),
    }];
    let mut wait = waiter("wait", "only_internal", Some(EventSource::External), 60);
    wait.needs = vec!["emit".to_owned()];
    let engine = engine(store.clone(), Arc::new(SleepClock), "run-source-filter");
    let report = engine
        .start(&loaded(workflow(vec![emit, wait])))
        .expect("run");
    assert_eq!(
        report.status,
        RunStatus::Failed,
        "internal event must not satisfy an external await"
    );
    assert!(
        store
            .event_cursor("run-source-filter", "wait", "only_internal")
            .expect("cursor")
            .is_none()
    );
}

#[test]
fn a_cycle_consumes_each_round_event_once_in_sequence() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = store(&directory.path().join("db.sqlite"));
    let mut produce = exec("produce", "true");
    produce.needs = vec!["consume".to_owned()];
    produce.cycle = Some(CycleSpec {
        max_rounds: 2,
        until: None,
    });
    produce.emit = vec![ostinato_domain::EmitSpec {
        name: "tick".to_owned(),
        external: false,
        payload: Some(json!({"round": 1})),
    }];
    let mut consume = waiter("consume", "tick", Some(EventSource::Internal), 1000);
    consume.needs = vec!["produce".to_owned()];
    let engine = engine(store.clone(), Arc::new(SleepClock), "run-cycle-events");
    let report = engine
        .start(&loaded(workflow(vec![produce, consume])))
        .expect("run");
    assert_eq!(report.status, RunStatus::Succeeded, "{:?}", report.error);
    let events = store
        .workflow_events(
            "run-cycle-events",
            &WorkflowEventQuery {
                name: Some("tick".to_owned()),
                ..WorkflowEventQuery::default()
            },
        )
        .expect("events");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].seq, 0);
    assert_eq!(events[1].seq, 1);
    assert_eq!(
        store
            .event_cursor("run-cycle-events", "consume", "tick")
            .expect("cursor")
            .expect("present")
            .next_seq,
        2
    );
}

#[test]
fn malformed_persisted_payload_fails_with_event_malformed() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = store(&directory.path().join("db.sqlite"));
    let clock = Arc::new(
        ScriptedClock::new(store.clone(), "run-malformed")
            .with_corrupt_event(directory.path().join("db.sqlite")),
    );
    let engine = engine(store.clone(), clock, "run-malformed");
    let report = engine
        .start(&loaded(workflow(vec![waiter(
            "wait",
            "go",
            Some(EventSource::External),
            2000,
        )])))
        .expect("run");
    assert_eq!(report.status, RunStatus::Failed);
    assert!(report.steps.iter().any(|step| {
        step.error
            .as_deref()
            .is_some_and(|error| error.contains("malformed"))
    }));
    let audit = store.events("run-malformed").expect("audit");
    assert!(audit.iter().any(|event| event.kind == "event.malformed"));
}

#[test]
fn runnable_work_executes_before_a_sleeping_await_takes_a_slot() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = store(&directory.path().join("db.sqlite"));
    let marker = directory.path().join("fast-ran");
    let fast = exec("fast", &format!("touch {}", marker.display()));
    // Same group: independent of the await.
    let clock = Arc::new(
        ScriptedClock::new(store.clone(), "run-slot")
            .with_marker(marker)
            .with_event("go", json!(true)),
    );
    let engine = engine(store.clone(), clock, "run-slot");
    let report = engine
        .start(&loaded(workflow(vec![
            fast,
            waiter("wait", "go", Some(EventSource::External), 5000),
        ])))
        .expect("run");
    assert_eq!(report.status, RunStatus::Succeeded, "{:?}", report.error);
}
