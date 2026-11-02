use std::path::Path;
use std::sync::Mutex;

use ostinato_application::{Completion, MetricsSummary, PortError, PortResult, RunSnapshot, Store};
use ostinato_domain::{
    AuditEvent, Improvement, NewEvent, NewImprovement, NewTrial, RunRecord, RunStatus, StepRecord,
    StepStatus, Trial, WorkflowSpec, audit,
};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
use serde_json::{Value as Json, json};

/// SQLite-backed durable store. WAL + synchronous=FULL gives crash-safe
/// commits; every group completion and audit event is one transaction.
#[derive(Debug)]
pub struct SqliteStore {
    connection: Mutex<Connection>,
}

impl SqliteStore {
    /// Open (and migrate) a store at `path`.
    ///
    /// # Errors
    /// Returns a store error when the file cannot be opened or migrated.
    pub fn open(path: &Path) -> PortResult<Self> {
        let created_parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty() && !parent.exists());
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|err| PortError::Io(format!("cannot create store directory: {err}")))?;
        }
        let connection = Connection::open(path)
            .map_err(|err| PortError::Store(format!("cannot open database: {err}")))?;
        // Set the main file mode before WAL creates -wal/-shm, so they inherit it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            if let Some(parent) = created_parent {
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }

        let had_runs = table_exists(&connection, "runs")?;
        let had_state_hash = had_runs && column_exists(&connection, "runs", "state_hash")?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|err| PortError::Store(format!("read schema version: {err}")))?;

        connection
            .execute_batch(
                r#"
                PRAGMA busy_timeout = 5000;
                PRAGMA journal_mode = WAL;
                PRAGMA synchronous = FULL;
                PRAGMA foreign_keys = ON;

                CREATE TABLE IF NOT EXISTS runs (
                    id             TEXT PRIMARY KEY,
                    workflow_name  TEXT NOT NULL,
                    source_file    TEXT NOT NULL,
                    spec_json      TEXT NOT NULL,
                    params_json    TEXT NOT NULL,
                    status         TEXT NOT NULL,
                    resume_index   INTEGER NOT NULL,
                    parent_run_id  TEXT,
                    error          TEXT,
                    created_at     TEXT NOT NULL,
                    updated_at     TEXT NOT NULL,
                    audit_head     TEXT NOT NULL DEFAULT '',
                    state_hash     TEXT NOT NULL DEFAULT ''
                );

                CREATE TABLE IF NOT EXISTS steps (
                    run_id       TEXT NOT NULL,
                    name         TEXT NOT NULL,
                    status       TEXT NOT NULL,
                    visits       INTEGER NOT NULL,
                    attempts     INTEGER NOT NULL,
                    output_json  TEXT,
                    error        TEXT,
                    updated_at   TEXT NOT NULL,
                    PRIMARY KEY (run_id, name),
                    FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS events (
                    run_id     TEXT NOT NULL,
                    seq        INTEGER NOT NULL,
                    ts         TEXT NOT NULL,
                    kind       TEXT NOT NULL,
                    step       TEXT,
                    data_json  TEXT NOT NULL,
                    prev_hash  TEXT NOT NULL,
                    hash       TEXT NOT NULL,
                    PRIMARY KEY (run_id, seq),
                    FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS trials (
                    id           INTEGER PRIMARY KEY AUTOINCREMENT,
                    run_id       TEXT NOT NULL,
                    node_index   INTEGER NOT NULL,
                    step         TEXT NOT NULL,
                    round        INTEGER NOT NULL,
                    attempt      INTEGER NOT NULL,
                    status       TEXT NOT NULL,
                    score        REAL NOT NULL,
                    duration_ms  INTEGER NOT NULL,
                    input_hash   TEXT NOT NULL,
                    output_hash  TEXT NOT NULL,
                    input_json   TEXT NOT NULL,
                    output_json  TEXT,
                    error        TEXT,
                    created_at   TEXT NOT NULL,
                    FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS improvements (
                    id            INTEGER PRIMARY KEY AUTOINCREMENT,
                    run_id        TEXT NOT NULL,
                    parent_id     INTEGER,
                    round         INTEGER NOT NULL,
                    diagnosis     TEXT NOT NULL,
                    proposal_json TEXT NOT NULL,
                    created_at    TEXT NOT NULL,
                    FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
                );

                CREATE INDEX IF NOT EXISTS idx_runs_created ON runs(created_at DESC);
                CREATE INDEX IF NOT EXISTS idx_events_run_seq ON events(run_id, seq);
                CREATE INDEX IF NOT EXISTS idx_trials_run ON trials(run_id, id);
                "#,
            )
            .map_err(|err| PortError::Store(format!("migration failed: {err}")))?;

        // One-time migration. Never repair blank integrity fields after v1: a
        // blank hash after migration is evidence of tampering, not a hint to
        // recompute it.
        if had_runs {
            if version < 1 {
                if !had_state_hash {
                    ensure_column(
                        &connection,
                        "runs",
                        "audit_head",
                        "TEXT NOT NULL DEFAULT ''",
                    )?;
                    ensure_column(
                        &connection,
                        "runs",
                        "state_hash",
                        "TEXT NOT NULL DEFAULT ''",
                    )?;
                    backfill_audit_head(&connection)?;
                    backfill_state_hash(&connection)?;
                }
                connection
                    .execute_batch("PRAGMA user_version = 1")
                    .map_err(|err| PortError::Store(format!("set schema version: {err}")))?;
            } else {
                for column in ["audit_head", "state_hash"] {
                    if !column_exists(&connection, "runs", column)? {
                        return Err(PortError::Store(format!(
                            "database schema is missing runs.{column}; refusing to repair an integrity column"
                        )));
                    }
                }
            }
        } else {
            connection
                .execute_batch("PRAGMA user_version = 1")
                .map_err(|err| PortError::Store(format!("set schema version: {err}")))?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for suffix in ["-wal", "-shm"] {
                let sidecar = format!("{}{suffix}", path.display());
                let _ = std::fs::set_permissions(sidecar, std::fs::Permissions::from_mode(0o600));
            }
        }

        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn lock(&self) -> PortResult<std::sync::MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| PortError::Store("database mutex poisoned".to_owned()))
    }
}

impl Store for SqliteStore {
    fn create_run(&self, run: &RunRecord) -> PortResult<()> {
        let connection = self.lock()?;
        let spec = serde_json::to_string(&run.spec)
            .map_err(|err| PortError::Store(format!("serialize spec: {err}")))?;
        let params = serde_json::to_string(&run.params)
            .map_err(|err| PortError::Store(format!("serialize params: {err}")))?;
        connection
            .execute(
                "INSERT INTO runs
                 (id, workflow_name, source_file, spec_json, params_json, status,
                  resume_index, parent_run_id, error, created_at, updated_at,
                  audit_head, state_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    run.id,
                    run.workflow_name,
                    run.source_file,
                    spec,
                    params,
                    run.status.as_str(),
                    run.resume_index,
                    run.parent_run_id,
                    run.error,
                    run.created_at,
                    run.updated_at,
                    run.audit_head,
                    run.state_hash,
                ],
            )
            .map_err(|err| PortError::Store(format!("insert run: {err}")))?;
        Ok(())
    }

    fn load_run(&self, run_id: &str) -> PortResult<Option<RunRecord>> {
        let connection = self.lock()?;
        connection
            .query_row(
                "SELECT id, workflow_name, source_file, spec_json, params_json, status,
                        resume_index, parent_run_id, error, created_at, updated_at, audit_head, state_hash
                 FROM runs WHERE id = ?1",
                params![run_id],
                run_from_row,
            )
            .optional()
            .map_err(|err| PortError::Store(format!("load run: {err}")))
    }

    fn snapshot(&self, run_id: &str) -> PortResult<Option<RunSnapshot>> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
            .map_err(|err| PortError::Store(format!("begin snapshot: {err}")))?;
        let run = transaction
            .query_row(
                "SELECT id, workflow_name, source_file, spec_json, params_json, status,
                        resume_index, parent_run_id, error, created_at, updated_at, audit_head,
                        state_hash
                 FROM runs WHERE id = ?1",
                params![run_id],
                run_from_row,
            )
            .optional()
            .map_err(|err| PortError::Store(format!("snapshot run: {err}")))?;
        let Some(run) = run else {
            return Ok(None);
        };
        let mut step_statement = transaction
            .prepare(
                "SELECT run_id, name, status, visits, attempts, output_json, error, updated_at
                 FROM steps WHERE run_id = ?1 ORDER BY name",
            )
            .map_err(|err| PortError::Store(format!("snapshot steps: {err}")))?;
        let step_rows = step_statement
            .query_map(params![run_id], step_from_row)
            .map_err(|err| PortError::Store(format!("snapshot steps: {err}")))?;
        let mut steps = Vec::new();
        for row in step_rows {
            steps.push(row.map_err(|err| PortError::Store(format!("snapshot steps: {err}")))?);
        }
        drop(step_statement);
        let mut event_statement = transaction
            .prepare(
                "SELECT run_id, seq, ts, kind, step, data_json, prev_hash, hash
                 FROM events WHERE run_id = ?1 ORDER BY seq",
            )
            .map_err(|err| PortError::Store(format!("snapshot events: {err}")))?;
        let event_rows = event_statement
            .query_map(params![run_id], event_from_row)
            .map_err(|err| PortError::Store(format!("snapshot events: {err}")))?;
        let mut events = Vec::new();
        for row in event_rows {
            events.push(row.map_err(|err| PortError::Store(format!("snapshot events: {err}")))?);
        }
        Ok(Some(RunSnapshot { run, steps, events }))
    }

    fn list_runs(&self, limit: usize) -> PortResult<Vec<RunRecord>> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(
                "SELECT id, workflow_name, source_file, spec_json, params_json, status,
                        resume_index, parent_run_id, error, created_at, updated_at, audit_head, state_hash
                 FROM runs ORDER BY created_at DESC LIMIT ?1",
            )
            .map_err(|err| PortError::Store(format!("prepare list runs: {err}")))?;
        let rows = statement
            .query_map(
                params![i64::try_from(limit).unwrap_or(i64::MAX)],
                run_from_row,
            )
            .map_err(|err| PortError::Store(format!("list runs: {err}")))?;
        let mut runs = Vec::new();
        for row in rows {
            runs.push(row.map_err(|err| PortError::Store(format!("map run: {err}")))?);
        }
        Ok(runs)
    }

    fn update_run_status(
        &self,
        run_id: &str,
        status: RunStatus,
        resume_index: i64,
        error: Option<&str>,
        updated_at: &str,
    ) -> PortResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|err| PortError::Store(format!("begin status update: {err}")))?;
        let changed = transaction
            .execute(
                "UPDATE runs SET status = ?2, resume_index = ?3, error = ?4, updated_at = ?5
                 WHERE id = ?1
                   AND status IN ('pending','running','failed','interrupted')",
                params![run_id, status.as_str(), resume_index, error, updated_at],
            )
            .map_err(|err| PortError::Store(format!("update run: {err}")))?;
        if changed == 0 {
            return Err(PortError::Cancelled(run_id.to_owned()));
        }
        transaction
            .commit()
            .map_err(|err| PortError::Store(format!("commit status update: {err}")))?;
        Ok(())
    }

    fn steps(&self, run_id: &str) -> PortResult<Vec<StepRecord>> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(
                "SELECT run_id, name, status, visits, attempts, output_json, error, updated_at
                 FROM steps WHERE run_id = ?1 ORDER BY name",
            )
            .map_err(|err| PortError::Store(format!("prepare steps: {err}")))?;
        let rows = statement
            .query_map(params![run_id], step_from_row)
            .map_err(|err| PortError::Store(format!("list steps: {err}")))?;
        let mut steps = Vec::new();
        for row in rows {
            steps.push(row.map_err(|err| PortError::Store(format!("map step: {err}")))?);
        }
        Ok(steps)
    }

    fn complete(&self, completion: Completion) -> PortResult<Vec<AuditEvent>> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|err| PortError::Store(format!("begin completion: {err}")))?;
        for step in &completion.steps {
            let output = match &step.output {
                Some(value) => Some(
                    serde_json::to_string(value)
                        .map_err(|err| PortError::Store(format!("serialize output: {err}")))?,
                ),
                None => None,
            };
            transaction
                .execute(
                    "INSERT INTO steps
                     (run_id, name, status, visits, attempts, output_json, error, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(run_id, name) DO UPDATE SET
                        status = excluded.status,
                        visits = excluded.visits,
                        attempts = excluded.attempts,
                        output_json = excluded.output_json,
                        error = excluded.error,
                        updated_at = excluded.updated_at",
                    params![
                        step.run_id,
                        step.name,
                        step.status.as_str(),
                        step.visits,
                        step.attempts,
                        output,
                        step.error,
                        step.updated_at,
                    ],
                )
                .map_err(|err| PortError::Store(format!("upsert step: {err}")))?;
        }
        let mut events = Vec::with_capacity(completion.events.len());
        for event in completion.events {
            events.push(append_event_tx(
                &transaction,
                &completion.run_id,
                &event,
                &completion.updated_at,
            )?);
        }
        let state_hash = state_hash_tx(&transaction, &completion.run_id, completion.resume_index)?;
        events.push(append_event_tx(
            &transaction,
            &completion.run_id,
            &NewEvent::new(
                "state.committed",
                None,
                json!({"stateHash": state_hash, "resumeIndex": completion.resume_index}),
            ),
            &completion.updated_at,
        )?);
        let head = events.last().map_or_else(
            || current_head_tx(&transaction, &completion.run_id),
            |event| Ok(event.hash.clone()),
        )?;
        let changed = transaction
            .execute(
                "UPDATE runs SET status = ?2, resume_index = ?3, error = ?4,
                        updated_at = ?5, audit_head = ?6, state_hash = ?7
                 WHERE id = ?1 AND status != 'cancelled'",
                params![
                    completion.run_id,
                    completion.status.as_str(),
                    completion.resume_index,
                    completion.error,
                    completion.updated_at,
                    head,
                    state_hash,
                ],
            )
            .map_err(|err| PortError::Store(format!("update run: {err}")))?;
        if changed == 0 {
            return Err(PortError::Cancelled(completion.run_id));
        }
        transaction
            .commit()
            .map_err(|err| PortError::Store(format!("commit completion: {err}")))?;
        Ok(events)
    }

    fn advance(
        &self,
        run_id: &str,
        resume_index: i64,
        updated_at: &str,
        event: NewEvent,
    ) -> PortResult<AuditEvent> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|err| PortError::Store(format!("begin advance: {err}")))?;
        let event = append_event_tx(&transaction, run_id, &event, updated_at)?;
        let state_hash = state_hash_tx(&transaction, run_id, resume_index)?;
        let state_event = append_event_tx(
            &transaction,
            run_id,
            &NewEvent::new(
                "state.committed",
                None,
                json!({"stateHash": state_hash, "resumeIndex": resume_index}),
            ),
            updated_at,
        )?;
        let changed = transaction
            .execute(
                "UPDATE runs SET resume_index = ?2, updated_at = ?3, audit_head = ?4,
                        state_hash = ?5
                 WHERE id = ?1 AND status != 'cancelled'",
                params![
                    run_id,
                    resume_index,
                    updated_at,
                    state_event.hash,
                    state_hash
                ],
            )
            .map_err(|err| PortError::Store(format!("advance audit head: {err}")))?;
        if changed == 0 {
            return Err(PortError::Cancelled(run_id.to_owned()));
        }
        transaction
            .commit()
            .map_err(|err| PortError::Store(format!("commit advance: {err}")))?;
        Ok(event)
    }

    fn append_event(&self, run_id: &str, event: NewEvent, ts: &str) -> PortResult<AuditEvent> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|err| PortError::Store(format!("begin event: {err}")))?;
        let event = append_event_tx(&transaction, run_id, &event, ts)?;
        let changed = transaction
            .execute(
                "UPDATE runs SET audit_head = ?2 WHERE id = ?1 AND status != 'cancelled'",
                params![run_id, event.hash],
            )
            .map_err(|err| PortError::Store(format!("update audit head: {err}")))?;
        if changed == 0 {
            return Err(PortError::Cancelled(run_id.to_owned()));
        }
        transaction
            .commit()
            .map_err(|err| PortError::Store(format!("commit event: {err}")))?;
        Ok(event)
    }

    fn events(&self, run_id: &str) -> PortResult<Vec<AuditEvent>> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(
                "SELECT run_id, seq, ts, kind, step, data_json, prev_hash, hash
                 FROM events WHERE run_id = ?1 ORDER BY seq",
            )
            .map_err(|err| PortError::Store(format!("prepare events: {err}")))?;
        let rows = statement
            .query_map(params![run_id], event_from_row)
            .map_err(|err| PortError::Store(format!("list events: {err}")))?;
        let mut events = Vec::new();
        for row in rows {
            events.push(row.map_err(|err| PortError::Store(format!("map event: {err}")))?);
        }
        Ok(events)
    }

    fn record_trial(&self, trial: &NewTrial) -> PortResult<i64> {
        let connection = self.lock()?;
        let input = serde_json::to_string(&trial.input)
            .map_err(|err| PortError::Store(format!("serialize trial input: {err}")))?;
        let output = match &trial.output {
            Some(value) => Some(
                serde_json::to_string(value)
                    .map_err(|err| PortError::Store(format!("serialize trial output: {err}")))?,
            ),
            None => None,
        };
        connection
            .execute(
                "INSERT INTO trials
                 (run_id, node_index, step, round, attempt, status, score, duration_ms,
                  input_hash, output_hash, input_json, output_json, error, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    trial.run_id,
                    trial.node_index,
                    trial.step,
                    trial.round,
                    trial.attempt,
                    trial.status.as_str(),
                    trial.score,
                    i64::try_from(trial.duration_ms).unwrap_or(i64::MAX),
                    trial.input_hash,
                    trial.output_hash,
                    input,
                    output,
                    trial.error,
                    trial.created_at,
                ],
            )
            .map_err(|err| PortError::Store(format!("insert trial: {err}")))?;
        Ok(connection.last_insert_rowid())
    }

    fn trials(&self, run_id: &str) -> PortResult<Vec<Trial>> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(
                "SELECT id, run_id, node_index, step, round, attempt, status, score,
                        duration_ms, input_hash, output_hash, input_json, output_json,
                        error, created_at
                 FROM trials WHERE run_id = ?1 ORDER BY id",
            )
            .map_err(|err| PortError::Store(format!("prepare trials: {err}")))?;
        let rows = statement
            .query_map(params![run_id], trial_from_row)
            .map_err(|err| PortError::Store(format!("list trials: {err}")))?;
        let mut trials = Vec::new();
        for row in rows {
            trials.push(row.map_err(|err| PortError::Store(format!("map trial: {err}")))?);
        }
        Ok(trials)
    }

    fn record_improvement(&self, improvement: &NewImprovement) -> PortResult<i64> {
        let connection = self.lock()?;
        let proposal = serde_json::to_string(&improvement.proposal)
            .map_err(|err| PortError::Store(format!("serialize improvement proposal: {err}")))?;
        connection
            .execute(
                "INSERT INTO improvements
                 (run_id, parent_id, round, diagnosis, proposal_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    improvement.run_id,
                    improvement.parent_id,
                    improvement.round,
                    improvement.diagnosis,
                    proposal,
                    improvement.created_at,
                ],
            )
            .map_err(|err| PortError::Store(format!("insert improvement: {err}")))?;
        Ok(connection.last_insert_rowid())
    }

    fn improvements(&self, run_id: &str) -> PortResult<Vec<Improvement>> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(
                "SELECT id, run_id, parent_id, round, diagnosis, proposal_json, created_at
                 FROM improvements WHERE run_id = ?1 ORDER BY id",
            )
            .map_err(|err| PortError::Store(format!("prepare improvements: {err}")))?;
        let rows = statement
            .query_map(params![run_id], improvement_from_row)
            .map_err(|err| PortError::Store(format!("list improvements: {err}")))?;
        let mut improvements = Vec::new();
        for row in rows {
            improvements
                .push(row.map_err(|err| PortError::Store(format!("map improvement: {err}")))?);
        }
        Ok(improvements)
    }

    fn cancel(&self, run_id: &str, updated_at: &str) -> PortResult<AuditEvent> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|err| PortError::Store(format!("begin cancel: {err}")))?;
        let current: Option<String> = transaction
            .query_row(
                "SELECT status FROM runs WHERE id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| PortError::Store(format!("read run status: {err}")))?;
        let Some(current) = current else {
            return Err(PortError::Config(format!("run not found: {run_id}")));
        };
        if matches!(current.as_str(), "succeeded" | "failed" | "cancelled") {
            return Err(PortError::Config(format!(
                "run {run_id} is already {current} and cannot be cancelled"
            )));
        }
        let changed = transaction
            .execute(
                "UPDATE runs SET status = 'cancelled', updated_at = ?2
                 WHERE id = ?1 AND status IN ('pending','running','interrupted')",
                params![run_id, updated_at],
            )
            .map_err(|err| PortError::Store(format!("cancel run: {err}")))?;
        if changed == 0 {
            return Err(PortError::Config(format!(
                "run {run_id} changed state and cannot be cancelled"
            )));
        }
        let event = append_event_tx(
            &transaction,
            run_id,
            &NewEvent::new("run.cancelled", None, Json::Null),
            updated_at,
        )?;
        transaction
            .execute(
                "UPDATE runs SET audit_head = ?2 WHERE id = ?1",
                params![run_id, event.hash],
            )
            .map_err(|err| PortError::Store(format!("cancel audit head: {err}")))?;
        transaction
            .commit()
            .map_err(|err| PortError::Store(format!("commit cancel: {err}")))?;
        Ok(event)
    }

    fn metrics(&self, run_id: Option<&str>) -> PortResult<MetricsSummary> {
        let connection = self.lock()?;
        let runs: (u64, u64, u64, u64) = connection
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(CASE WHEN status = 'succeeded' THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END), 0)
                 FROM runs WHERE (?1 IS NULL OR id = ?1)",
                params![run_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?.max(0) as u64,
                        row.get::<_, i64>(1)?.max(0) as u64,
                        row.get::<_, i64>(2)?.max(0) as u64,
                        row.get::<_, i64>(3)?.max(0) as u64,
                    ))
                },
            )
            .map_err(|err| PortError::Store(format!("metrics runs: {err}")))?;
        let steps: (u64, u64) = connection
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0)
                 FROM steps WHERE (?1 IS NULL OR run_id = ?1)",
                params![run_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?.max(0) as u64,
                        row.get::<_, i64>(1)?.max(0) as u64,
                    ))
                },
            )
            .map_err(|err| PortError::Store(format!("metrics steps: {err}")))?;
        let attempts: (u64, u64, f64) = connection
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(CASE WHEN attempt > 0 THEN 1 ELSE 0 END), 0),
                        COALESCE(AVG(duration_ms), 0.0)
                 FROM trials WHERE (?1 IS NULL OR run_id = ?1)",
                params![run_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?.max(0) as u64,
                        row.get::<_, i64>(1)?.max(0) as u64,
                        row.get::<_, f64>(2)?,
                    ))
                },
            )
            .map_err(|err| PortError::Store(format!("metrics trials: {err}")))?;
        Ok(MetricsSummary {
            runs_total: runs.0,
            runs_succeeded: runs.1,
            runs_failed: runs.2,
            runs_running: runs.3,
            steps_total: steps.0,
            steps_failed: steps.1,
            attempts_total: attempts.0,
            retries_total: attempts.1,
            avg_attempt_ms: attempts.2,
        })
    }
}

fn table_exists(connection: &Connection, table: &str) -> PortResult<bool> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![table],
            |row| row.get(0),
        )
        .map_err(|err| PortError::Store(format!("inspect schema: {err}")))?;
    Ok(count > 0)
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> PortResult<bool> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|err| PortError::Store(format!("inspect schema: {err}")))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| PortError::Store(format!("inspect schema: {err}")))?;
    for row in rows {
        if row.map_err(|err| PortError::Store(format!("inspect schema: {err}")))? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> PortResult<()> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|err| PortError::Store(format!("inspect schema: {err}")))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| PortError::Store(format!("inspect schema: {err}")))?;
    let mut exists = false;
    for row in rows {
        if row.map_err(|err| PortError::Store(format!("inspect schema: {err}")))? == column {
            exists = true;
            break;
        }
    }
    drop(statement);
    if !exists {
        connection
            .execute(
                &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
                [],
            )
            .map_err(|err| PortError::Store(format!("add column {table}.{column}: {err}")))?;
    }
    Ok(())
}

fn backfill_audit_head(connection: &Connection) -> PortResult<()> {
    let missing: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM runs WHERE audit_head = '' OR audit_head IS NULL",
            [],
            |row| row.get(0),
        )
        .map_err(|err| PortError::Store(format!("check audit heads: {err}")))?;
    if missing == 0 {
        return Ok(());
    }
    connection
        .execute(
            "UPDATE runs SET audit_head = COALESCE(
                 (SELECT hash FROM events WHERE events.run_id = runs.id
                  ORDER BY seq DESC LIMIT 1), ?1)
             WHERE audit_head = '' OR audit_head IS NULL OR (
                 audit_head = ?1 AND EXISTS (
                     SELECT 1 FROM events WHERE events.run_id = runs.id
                 )
             )",
            params![audit::GENESIS_HASH],
        )
        .map_err(|err| PortError::Store(format!("backfill audit head: {err}")))?;
    Ok(())
}

fn backfill_state_hash(connection: &Connection) -> PortResult<()> {
    let candidates: Vec<(String, String, String, i64, String)> = {
        let mut statement = connection
            .prepare(
                "SELECT id, spec_json, params_json, resume_index, source_file FROM runs
                 WHERE state_hash = '' OR state_hash IS NULL",
            )
            .map_err(|err| PortError::Store(format!("find state hashes: {err}")))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .map_err(|err| PortError::Store(format!("find state hashes: {err}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|err| PortError::Store(format!("find state hashes: {err}")))?);
        }
        out
    };
    for (run_id, spec_json, params_json, resume_index, source_file) in candidates {
        let spec: WorkflowSpec = serde_json::from_str(&spec_json)
            .map_err(|err| PortError::Store(format!("parse spec for state hash: {err}")))?;
        let params: Json = serde_json::from_str(&params_json)
            .map_err(|err| PortError::Store(format!("parse params for state hash: {err}")))?;
        let mut statement = connection
            .prepare(
                "SELECT run_id, name, status, visits, attempts, output_json, error, updated_at
                 FROM steps WHERE run_id = ?1 ORDER BY name",
            )
            .map_err(|err| PortError::Store(format!("load steps for state hash: {err}")))?;
        let rows = statement
            .query_map(params![run_id], step_from_row)
            .map_err(|err| PortError::Store(format!("load steps for state hash: {err}")))?;
        let mut steps = Vec::new();
        for row in rows {
            steps.push(
                row.map_err(|err| PortError::Store(format!("load steps for state hash: {err}")))?,
            );
        }
        drop(statement);
        let digest = audit::state_digest(&spec, &params, &source_file, resume_index, &steps);
        connection
            .execute(
                "UPDATE runs SET state_hash = ?2 WHERE id = ?1",
                params![run_id, digest],
            )
            .map_err(|err| PortError::Store(format!("backfill state hash: {err}")))?;
    }
    Ok(())
}

fn state_hash_tx(
    transaction: &rusqlite::Transaction<'_>,
    run_id: &str,
    resume_index: i64,
) -> PortResult<String> {
    let (spec_json, params_json, source_file): (String, String, String) = transaction
        .query_row(
            "SELECT spec_json, params_json, source_file FROM runs WHERE id = ?1",
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|err| PortError::Store(format!("read run for state hash: {err}")))?;
    let spec: WorkflowSpec = serde_json::from_str(&spec_json)
        .map_err(|err| PortError::Store(format!("parse spec for state hash: {err}")))?;
    let params: Json = serde_json::from_str(&params_json)
        .map_err(|err| PortError::Store(format!("parse params for state hash: {err}")))?;
    let mut statement = transaction
        .prepare(
            "SELECT run_id, name, status, visits, attempts, output_json, error, updated_at
             FROM steps WHERE run_id = ?1 ORDER BY name",
        )
        .map_err(|err| PortError::Store(format!("load steps for state hash: {err}")))?;
    let rows = statement
        .query_map(params![run_id], step_from_row)
        .map_err(|err| PortError::Store(format!("load steps for state hash: {err}")))?;
    let mut steps = Vec::new();
    for row in rows {
        steps.push(
            row.map_err(|err| PortError::Store(format!("load steps for state hash: {err}")))?,
        );
    }
    Ok(audit::state_digest(
        &spec,
        &params,
        &source_file,
        resume_index,
        &steps,
    ))
}

fn current_head_tx(transaction: &rusqlite::Transaction<'_>, run_id: &str) -> PortResult<String> {
    transaction
        .query_row(
            "SELECT audit_head FROM runs WHERE id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(|err| PortError::Store(format!("read audit head: {err}")))
}

fn append_event_tx(
    transaction: &rusqlite::Transaction<'_>,
    run_id: &str,
    event: &NewEvent,
    ts: &str,
) -> PortResult<AuditEvent> {
    let previous: Option<(i64, String)> = transaction
        .query_row(
            "SELECT seq, hash FROM events WHERE run_id = ?1 ORDER BY seq DESC LIMIT 1",
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|err| PortError::Store(format!("read audit head: {err}")))?;
    let expected_head = current_head_tx(transaction, run_id)?;
    let (seq, prev_hash) = match previous {
        Some((seq, hash)) => {
            if hash != expected_head {
                return Err(PortError::Store(format!(
                    "audit chain for run {run_id} is truncated or corrupt; refusing to append"
                )));
            }
            (u64::try_from(seq).unwrap_or(0) + 1, hash)
        }
        None => {
            if expected_head != audit::GENESIS_HASH {
                return Err(PortError::Store(format!(
                    "audit chain for run {run_id} is missing events; refusing to append"
                )));
            }
            (0, audit::GENESIS_HASH.to_owned())
        }
    };
    let sealed = audit::seal(
        run_id,
        seq,
        ts,
        &event.kind,
        event.step.as_deref(),
        &event.data,
        &prev_hash,
    )
    .map_err(|err| PortError::Store(format!("seal audit event: {err}")))?;
    let data = serde_json::to_string(&sealed.data)
        .map_err(|err| PortError::Store(format!("serialize audit data: {err}")))?;
    transaction
        .execute(
            "INSERT INTO events (run_id, seq, ts, kind, step, data_json, prev_hash, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                sealed.run_id,
                i64::try_from(sealed.seq).unwrap_or(i64::MAX),
                sealed.ts,
                sealed.kind,
                sealed.step,
                data,
                sealed.prev_hash,
                sealed.hash,
            ],
        )
        .map_err(|err| PortError::Store(format!("insert audit event: {err}")))?;
    transaction
        .execute(
            "UPDATE runs SET audit_head = ?2 WHERE id = ?1",
            params![run_id, sealed.hash],
        )
        .map_err(|err| PortError::Store(format!("update audit head: {err}")))?;
    Ok(sealed)
}

fn run_from_row(row: &Row<'_>) -> rusqlite::Result<RunRecord> {
    let status: String = row.get(5)?;
    let spec_json: String = row.get(3)?;
    let params_json: String = row.get(4)?;
    let spec: WorkflowSpec = serde_json::from_str(&spec_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(err))
    })?;
    let params: Json = serde_json::from_str(&params_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(err))
    })?;
    Ok(RunRecord {
        id: row.get(0)?,
        workflow_name: row.get(1)?,
        source_file: row.get(2)?,
        spec,
        params,
        status: parse_run_status(&status)?,
        resume_index: row.get(6)?,
        parent_run_id: row.get(7)?,
        error: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        audit_head: row.get(11)?,
        state_hash: row.get(12)?,
    })
}

fn step_from_row(row: &Row<'_>) -> rusqlite::Result<StepRecord> {
    let status: String = row.get(2)?;
    let output_json: Option<String> = row.get(5)?;
    let output = match output_json {
        Some(json) => Some(serde_json::from_str(&json).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(err))
        })?),
        None => None,
    };
    Ok(StepRecord {
        run_id: row.get(0)?,
        name: row.get(1)?,
        status: parse_step_status(&status)?,
        visits: row.get::<_, i64>(3)?.max(0) as u32,
        attempts: row.get::<_, i64>(4)?.max(0) as u32,
        output,
        error: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn event_from_row(row: &Row<'_>) -> rusqlite::Result<AuditEvent> {
    let data_json: String = row.get(5)?;
    let data = serde_json::from_str(&data_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(err))
    })?;
    Ok(AuditEvent {
        run_id: row.get(0)?,
        seq: row.get::<_, i64>(1)?.max(0) as u64,
        ts: row.get(2)?,
        kind: row.get(3)?,
        step: row.get(4)?,
        data,
        prev_hash: row.get(6)?,
        hash: row.get(7)?,
    })
}

fn trial_from_row(row: &Row<'_>) -> rusqlite::Result<Trial> {
    let status: String = row.get(6)?;
    let input_json: String = row.get(11)?;
    let output_json: Option<String> = row.get(12)?;
    Ok(Trial {
        id: row.get(0)?,
        run_id: row.get(1)?,
        node_index: row.get(2)?,
        step: row.get(3)?,
        round: row.get::<_, i64>(4)?.max(0) as u32,
        attempt: row.get::<_, i64>(5)?.max(0) as u32,
        status: parse_trial_status(&status)?,
        score: row.get(7)?,
        duration_ms: row.get::<_, i64>(8)?.max(0) as u64,
        input_hash: row.get(9)?,
        output_hash: row.get(10)?,
        input: serde_json::from_str(&input_json).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                11,
                rusqlite::types::Type::Text,
                Box::new(err),
            )
        })?,
        output: match output_json {
            Some(json) => Some(serde_json::from_str(&json).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    12,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?),
            None => None,
        },
        error: row.get(13)?,
        created_at: row.get(14)?,
    })
}

fn improvement_from_row(row: &Row<'_>) -> rusqlite::Result<Improvement> {
    let proposal_json: String = row.get(5)?;
    Ok(Improvement {
        id: row.get(0)?,
        run_id: row.get(1)?,
        parent_id: row.get(2)?,
        round: row.get::<_, i64>(3)?.max(0) as u32,
        diagnosis: row.get(4)?,
        proposal: serde_json::from_str(&proposal_json).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(err))
        })?,
        created_at: row.get(6)?,
    })
}

fn parse_run_status(value: &str) -> rusqlite::Result<RunStatus> {
    match value {
        "pending" => Ok(RunStatus::Pending),
        "running" => Ok(RunStatus::Running),
        "succeeded" => Ok(RunStatus::Succeeded),
        "failed" => Ok(RunStatus::Failed),
        "interrupted" => Ok(RunStatus::Interrupted),
        "cancelled" => Ok(RunStatus::Cancelled),
        other => Err(rusqlite::Error::InvalidColumnType(
            5,
            format!("unknown run status `{other}`"),
            rusqlite::types::Type::Text,
        )),
    }
}

fn parse_step_status(value: &str) -> rusqlite::Result<StepStatus> {
    match value {
        "pending" => Ok(StepStatus::Pending),
        "running" => Ok(StepStatus::Running),
        "succeeded" => Ok(StepStatus::Succeeded),
        "failed" => Ok(StepStatus::Failed),
        "skipped" => Ok(StepStatus::Skipped),
        other => Err(rusqlite::Error::InvalidColumnType(
            2,
            format!("unknown step status `{other}`"),
            rusqlite::types::Type::Text,
        )),
    }
}

fn parse_trial_status(value: &str) -> rusqlite::Result<ostinato_domain::TrialStatus> {
    match value {
        "succeeded" => Ok(ostinato_domain::TrialStatus::Succeeded),
        "failed" => Ok(ostinato_domain::TrialStatus::Failed),
        "timed_out" => Ok(ostinato_domain::TrialStatus::TimedOut),
        "skipped" => Ok(ostinato_domain::TrialStatus::Skipped),
        other => Err(rusqlite::Error::InvalidColumnType(
            6,
            format!("unknown trial status `{other}`"),
            rusqlite::types::Type::Text,
        )),
    }
}
