#![allow(clippy::collapsible_if)]
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ostinato"))
}

fn cue_command() -> Option<String> {
    let cue = std::env::var("CUE_BIN").unwrap_or_else(|_| "cue".to_owned());
    Command::new(&cue)
        .arg("version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|_| cue)
}

fn ost(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("run ostinato")
}

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write fixture");
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("valid json output")
}

fn run_id(db: &Path) -> String {
    let db = db.to_str().expect("utf8 db");
    let output = ost(&["--db", db, "--json", "runs"]);
    let runs = json(&output);
    runs[0]["id"].as_str().expect("run id").to_owned()
}

#[test]
fn official_cue_runs_and_audit_verifies() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        r#"
package demo
params: { name: string | *"world" @tag(name) }
workflow: {
    name: "official"
    steps: {
        a: { kind: "exec", exec: { cmd: ["sh", "-c", "echo hello \(params.name)"] } }
        b: { kind: "exec", needs: ["a"], exec: { cmd: ["sh", "-c", "echo bye"] } }
    }
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--cue",
        &cue,
        "--evaluator",
        "cue",
        "--json",
        "run",
        workflow.to_str().expect("workflow"),
        "-p",
        "name=ostinato",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["status"], "succeeded");
    let run = run_id(&db);
    let verified = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(verified.status.success());
    assert!(String::from_utf8_lossy(&verified.stdout).contains("OK"));
}

#[test]
fn retries_are_attempted_and_logged_as_trials() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let counter = directory.path().join("counter");
    let workflow = directory.path().join("retry.cue");
    write(
        &workflow,
        &format!(
            r#"
package demo
workflow: {{
    name: "retry"
    steps: {{
        flaky: {{
            kind: "exec"
            retry: {{max: 3, baseMs: 1, factor: 1.0, jitter: false}}
            exec: {{
                cmd: ["sh", "-c", "n=$(cat {counter} 2>/dev/null || echo 0); n=$((n+1)); echo $n > {counter}; test $n -ge 3"]
            }}
        }}
    }}
}}
"#,
            counter = counter.display()
        ),
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "--json",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = run_id(&db);
    let trials = ost(&["--db", db.to_str().expect("db"), "--json", "trials", &run]);
    assert!(trials.status.success());
    let trials = json(&trials);
    assert_eq!(trials.as_array().expect("array").len(), 3);
    assert_eq!(trials[0]["status"], "failed");
    assert_eq!(trials[1]["status"], "failed");
    assert_eq!(trials[2]["status"], "succeeded");
}

#[test]
fn resume_skips_completed_steps_and_finishes() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let flag = directory.path().join("go");
    let trace = directory.path().join("trace");
    let workflow = directory.path().join("resume.cue");
    write(
        &workflow,
        &format!(
            r#"
package demo
workflow: {{
    name: "resume"
    steps: {{
        a: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo a >> {trace}"] }} }}
        b: {{
            kind: "exec"
            needs: ["a"]
            exec: {{ cmd: ["sh", "-c", "test -f {flag} || exit 7; echo b >> {trace}"] }}
        }}
        c: {{ kind: "exec", needs: ["b"], exec: {{ cmd: ["sh", "-c", "echo c >> {trace}"] }} }}
    }}
}}
"#,
            trace = trace.display(),
            flag = flag.display()
        ),
    );
    let first = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert_eq!(first.status.code(), Some(1));
    let run = run_id(&db);
    std::fs::write(&flag, b"go").expect("flag");
    let resumed = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "resume",
        &run,
    ]);
    assert!(
        resumed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    assert_eq!(std::fs::read_to_string(&trace).expect("trace"), "a\nb\nc\n");
    let trials = ost(&["--db", db.to_str().expect("db"), "--json", "trials", &run]);
    let trials = json(&trials);
    let a_trials = trials
        .as_array()
        .expect("array")
        .iter()
        .filter(|trial| trial["step"] == "a")
        .count();
    assert_eq!(a_trials, 1, "completed step must not re-execute on resume");
}

#[test]
fn cycles_stop_when_until_becomes_true() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let counter = directory.path().join("counter");
    let workflow = directory.path().join("cycle.cue");
    write(
        &workflow,
        &format!(
            r#"
package demo
workflow: {{
    name: "cycle"
    steps: {{
        review: {{
            kind: "exec"
            needs: ["refine"]
            cycle: {{max: 5, until: "steps.review.score >= 0.8"}}
            exec: {{
                cmd: ["sh", "-c", "n=$(cat {counter} 2>/dev/null || echo 0); n=$((n+1)); echo $n > {counter}; case $n in 1) s=0.3;; 2) s=0.6;; *) s=0.9;; esac; printf '{{\"score\": %s}}' \"$s\""]
            }}
        }}
        refine: {{ kind: "exec", needs: ["review"], exec: {{ cmd: ["sh", "-c", "echo refine"] }} }}
    }}
}}
"#,
            counter = counter.display()
        ),
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "--json",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["status"], "succeeded");
    let run = run_id(&db);
    let logs = ost(&["--db", db.to_str().expect("db"), "logs", &run]);
    let logs = String::from_utf8_lossy(&logs.stdout);
    assert!(logs.contains("cycle.completed"), "logs: {logs}");
    let status = ost(&["--db", db.to_str().expect("db"), "--json", "status", &run]);
    let status = json(&status);
    assert_eq!(status["run"]["resume_index"], 10);
}

#[test]
fn audit_verification_detects_tampering() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        r#"
package demo
workflow: {
    name: "audit"
    steps: {
        a: { kind: "exec", exec: { cmd: ["sh", "-c", "echo hi"] } }
    }
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--cue",
        &cue,
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(output.status.success());
    let run = run_id(&db);
    let connection = rusqlite::Connection::open(&db).expect("open db");
    connection
        .execute(
            "UPDATE events SET data_json = '{\"tampered\":true}' WHERE run_id = ?1 AND seq = 1",
            [&run],
        )
        .expect("tamper");
    let verified = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(!verified.status.success());
    assert!(String::from_utf8_lossy(&verified.stderr).contains("AUDIT FAILED"));
}

#[test]
fn recursive_improvement_creates_child_runs() {
    let Some(_cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let agent = directory.path().join("agent.sh");
    write(
        &agent,
        "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{\"score\":0.95,\"diagnosis\":\"ok\",\"params_patch\":{\"name\":\"v2\"},\"confidence\":0.9}'\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&agent).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&agent, permissions).expect("chmod");
    }
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        r#"
package demo
params: { name: string | *"v1" @tag(name) }
workflow: {
    name: "improve"
    steps: {
        assess: { kind: "agent", agent: { prompt: "score {{ params.name }}" } }
        report: {
            kind: "exec"
            needs: ["assess"]
            exec: {cmd: ["sh", "-c", "echo \(params.name) > param.txt"]}
        }
    }
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--cue",
        &_cue,
        "run",
        workflow.to_str().expect("workflow"),
        "--agent-cmd",
        agent.to_str().expect("agent"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = run_id(&db);
    let improved = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--cue",
        &_cue,
        "improve",
        &run,
        "--rounds",
        "2",
        "--agent-cmd",
        agent.to_str().expect("agent"),
    ]);
    assert!(
        improved.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&improved.stderr)
    );
    let runs = ost(&["--db", db.to_str().expect("db"), "--json", "runs"]);
    let runs = json(&runs);
    let runs = runs.as_array().expect("array");
    assert!(runs.len() >= 3, "expected parent + 2 children: {runs:?}");
    let child = runs
        .iter()
        .find(|entry| entry["parent_run_id"].is_string())
        .expect("child run");
    assert!(child["parent_run_id"].as_str().is_some());
    // Recursive improvement re-evaluates CUE, not just runtime templates.
    let cmd = child["spec"]["steps"]["report"]["exec"]["cmd"][2]
        .as_str()
        .expect("cmd");
    assert!(
        cmd.contains("v2"),
        "child spec still has baked params: {cmd}"
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("param.txt"))
            .expect("param.txt")
            .trim(),
        "v2"
    );
}

#[test]
fn native_evaluator_supports_the_offline_subset() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        r#"
package demo
params: { name: string | *"native" }
workflow: {
    name: "native"
    steps: {
        a: { kind: "exec", exec: { cmd: ["sh", "-c", "echo \(params.name)"] } }
    }
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "--json",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json(&output)["status"], "succeeded");
}

#[test]
fn audit_detects_tail_truncation_via_persisted_head() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        r#"
package demo
workflow: {
    name: "tail"
    steps: {
        a: { kind: "exec", exec: { cmd: ["sh", "-c", "echo hi"] } }
    }
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--cue",
        &cue,
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(output.status.success());
    let run = run_id(&db);
    let connection = rusqlite::Connection::open(&db).expect("open db");
    connection
        .execute("DELETE FROM events WHERE run_id = ?1 AND seq = 2", [&run])
        .expect("truncate tail");
    let verified = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(!verified.status.success());
    assert!(String::from_utf8_lossy(&verified.stderr).contains("AUDIT FAILED"));
}

#[test]
fn cancel_stops_a_running_run_and_blocks_resume() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        r#"
package demo
workflow: {
    name: "cancel"
    steps: {
        slow: { kind: "exec", exec: { cmd: ["sh", "-c", "sleep 4; echo done"] } }
        after: { kind: "exec", needs: ["slow"], exec: { cmd: ["sh", "-c", "echo after"] } }
    }
}
"#,
    );
    let mut child = std::process::Command::new(bin())
        .args([
            "--db",
            db.to_str().expect("db"),
            "--evaluator",
            "native",
            "run",
            workflow.to_str().expect("workflow"),
        ])
        .spawn()
        .expect("spawn run");
    let mut run = None;
    for _ in 0..80 {
        if let Ok(output) = std::process::Command::new(bin())
            .args(["--db", db.to_str().expect("db"), "--json", "runs"])
            .output()
        {
            if let Ok(runs) = serde_json::from_slice::<Value>(&output.stdout) {
                if let Some(first) = runs.as_array().and_then(|runs| runs.first()) {
                    run = first["id"].as_str().map(ToOwned::to_owned);
                    break;
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let run = run.expect("run id");
    std::thread::sleep(std::time::Duration::from_millis(150));
    let cancelled = ost(&["--db", db.to_str().expect("db"), "cancel", &run]);
    assert!(
        cancelled.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cancelled.stderr)
    );
    let _ = child.wait();
    let status = ost(&["--db", db.to_str().expect("db"), "--json", "status", &run]);
    assert_eq!(json(&status)["run"]["status"], "cancelled");
    let resumed = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "resume",
        &run,
    ]);
    assert!(!resumed.status.success());
    let verified = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(verified.status.success());

    // Cancelling a terminal run is rejected and must not rewrite its outcome.
    let again = ost(&["--db", db.to_str().expect("db"), "cancel", &run]);
    assert!(!again.status.success());
}

#[test]
fn bad_template_is_a_durable_failure() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        r#"
package demo
workflow: {
    name: "bad-template"
    steps: {
        a: { kind: "exec", exec: { cmd: ["sh", "-c", "echo a"] } }
        b: { kind: "exec", needs: ["a"], exec: { cmd: ["sh", "-c", "echo {{ steps.missing.stdout }}"] } }
    }
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert_eq!(output.status.code(), Some(1));
    let run = run_id(&db);
    let status = ost(&["--db", db.to_str().expect("db"), "--json", "status", &run]);
    let status = json(&status);
    assert_eq!(status["run"]["status"], "failed");
    assert!(
        status["run"]["error"]
            .as_str()
            .unwrap_or("")
            .contains("unknown template reference")
    );
    let logs = ost(&["--db", db.to_str().expect("db"), "logs", &run]);
    assert!(String::from_utf8_lossy(&logs.stdout).contains("run.failed"));
    let verified = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(verified.status.success());
}

#[test]
fn state_tampering_is_detected_and_resume_refuses_it() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        r#"
package demo
workflow: {
    name: "state"
    steps: {
        a: { kind: "exec", exec: { cmd: ["sh", "-c", "exit 5"] } }
    }
}
"#,
    );
    let first = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert_eq!(first.status.code(), Some(1));
    let run = run_id(&db);
    let connection = rusqlite::Connection::open(&db).expect("open db");
    connection
        .execute("UPDATE runs SET resume_index = 999 WHERE id = ?1", [&run])
        .expect("tamper checkpoint");
    let verified = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(!verified.status.success());
    let resumed = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "resume",
        &run,
    ]);
    assert!(!resumed.status.success());
    assert!(String::from_utf8_lossy(&resumed.stderr).contains("state digest mismatch"));
}

#[test]
fn read_only_commands_do_not_require_cue() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        r#"
package demo
workflow: {
    name: "read-only"
    steps: { a: { kind: "exec", exec: { cmd: ["sh", "-c", "echo hi"] } } }
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(output.status.success());
    let run = run_id(&db);
    let status = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--cue",
        "/nonexistent/cue",
        "--json",
        "status",
        &run,
    ]);
    assert!(
        status.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let runs = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--cue",
        "/nonexistent/cue",
        "runs",
    ]);
    assert!(runs.status.success());
}

#[test]
fn split_package_and_sibling_conflicts_follow_cue_semantics() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(directory.path().join("pkg")).expect("mkdir");
    let pkg = directory.path().join("pkg");
    write(
        &pkg.join("schema.cue"),
        r#"
package split
#W: {
    name: string
    steps: { a: { kind: "noop" } }
}
"#,
    );
    write(
        &pkg.join("workflow.cue"),
        r#"
package split
workflow: #W & { name: "split" }
"#,
    );
    let validated = ost(&[
        "--db",
        directory.path().join("db.sqlite").to_str().expect("db"),
        "--cue",
        &cue,
        "validate",
        pkg.join("workflow.cue").to_str().expect("workflow"),
    ]);
    assert!(
        validated.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&validated.stderr)
    );

    // Directory input works too (package mode).
    let directory_validated = ost(&[
        "--db",
        directory.path().join("db.sqlite").to_str().expect("db"),
        "--cue",
        &cue,
        "validate",
        pkg.to_str().expect("pkg"),
    ]);
    assert!(directory_validated.status.success());

    // A conflicting sibling must invalidate the package, not be ignored.
    write(
        &pkg.join("conflict.cue"),
        r#"
package split
workflow: { name: "other", steps: { a: { kind: "noop" } } }
"#,
    );
    let conflicted = ost(&[
        "--db",
        directory.path().join("db.sqlite").to_str().expect("db"),
        "--cue",
        &cue,
        "validate",
        pkg.join("workflow.cue").to_str().expect("workflow"),
    ]);
    assert!(!conflicted.status.success());
}

#[test]
fn unknown_fields_are_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let workflow = directory.path().join("typo.cue");
    write(
        &workflow,
        r#"
package demo
workflow: {
    maxparallel: 2
    steps: {
        a: { kind: "exec", exec: { cmd: ["true"], timeoutMS: 1 } }
    }
}
"#,
    );
    let output = ost(&[
        "--db",
        directory.path().join("db.sqlite").to_str().expect("db"),
        "--evaluator",
        "native",
        "validate",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown field"), "stderr: {stderr}");
}

#[test]
fn exec_timeout_kills_the_process_group() {
    let directory = tempfile::tempdir().expect("tempdir");
    let workflow = directory.path().join("timeout.cue");
    write(
        &workflow,
        r#"
package demo
workflow: {
    name: "timeout"
    steps: {
        slow: {
            kind: "exec"
            timeoutMs: 500
            exec: { cmd: ["sh", "-c", "sleep 30 & wait"] }
        }
    }
}
"#,
    );
    let started = std::time::Instant::now();
    let output = ost(&[
        "--db",
        directory.path().join("db.sqlite").to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    let elapsed = started.elapsed();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "timeout took {elapsed:?}"
    );
    let run = run_id(&directory.path().join("db.sqlite"));
    let trials = ost(&[
        "--db",
        directory.path().join("db.sqlite").to_str().expect("db"),
        "--json",
        "trials",
        &run,
    ]);
    let trials = json(&trials);
    assert_eq!(trials[0]["status"], "timed_out");
}

#[test]
fn truncation_blocks_resume_and_appends() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        r#"
package demo
workflow: {
    name: "launder"
    steps: { a: { kind: "exec", exec: { cmd: ["sh", "-c", "exit 3"] } } }
}
"#,
    );
    let first = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert_eq!(first.status.code(), Some(1));
    let run = run_id(&db);
    let connection = rusqlite::Connection::open(&db).expect("open db");
    let before: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM events WHERE run_id = ?1",
            [&run],
            |row| row.get(0),
        )
        .expect("count");
    connection
        .execute(
            "DELETE FROM events WHERE run_id = ?1 AND seq = ?2",
            rusqlite::params![run, before - 1],
        )
        .expect("truncate");
    let resumed = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "resume",
        &run,
    ]);
    assert!(!resumed.status.success());
    let after: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM events WHERE run_id = ?1",
            [&run],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(after, before - 1, "resume must not append after truncation");
}

#[test]
fn package_less_cue_file_is_loaded_by_filename() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let workflow = directory.path().join("anonymous.cue");
    write(
        &workflow,
        r#"
workflow: {
    name: "anonymous"
    steps: { a: { kind: "exec", exec: { cmd: ["sh", "-c", "echo hi"] } } }
}
"#,
    );
    // A sibling package must not be selected instead of the anonymous file.
    write(
        &directory.path().join("sibling.cue"),
        r#"
package sibling
workflow: { name: "wrong", steps: { a: { kind: "noop" } } }
"#,
    );
    let output = ost(&[
        "--db",
        directory.path().join("db.sqlite").to_str().expect("db"),
        "--cue",
        &cue,
        "validate",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("anonymous"));
}

#[test]
fn background_grandchild_does_not_extend_a_successful_step() {
    let directory = tempfile::tempdir().expect("tempdir");
    let workflow = directory.path().join("bg.cue");
    write(
        &workflow,
        r#"
package demo
workflow: {
    name: "background"
    steps: {
        bg: {
            kind: "exec"
            timeoutMs: 1000
            exec: { cmd: ["sh", "-c", "sleep 30 & echo started"] }
        }
    }
}
"#,
    );
    let started = std::time::Instant::now();
    let output = ost(&[
        "--db",
        directory.path().join("db.sqlite").to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    let elapsed = started.elapsed();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "background grandchild held the pipes for {elapsed:?}"
    );
}

#[test]
fn cleared_integrity_hashes_are_not_silently_repaired() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        r#"
package demo
workflow: {
    name: "no-repair"
    steps: { a: { kind: "exec", exec: { cmd: ["sh", "-c", "echo hi"] } } }
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(output.status.success());
    let run = run_id(&db);
    let connection = rusqlite::Connection::open(&db).expect("open db");
    connection
        .execute("UPDATE runs SET state_hash = '' WHERE id = ?1", [&run])
        .expect("clear state hash");
    let verified = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(
        !verified.status.success(),
        "cleared state_hash must not be repaired"
    );

    connection
        .execute("UPDATE runs SET audit_head = '' WHERE id = ?1", [&run])
        .expect("clear head");
    connection
        .execute(
            "DELETE FROM events WHERE run_id = ?1 AND seq = (SELECT MAX(seq) FROM events WHERE run_id = ?1)",
            [&run],
        )
        .expect("truncate");
    let verified = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(
        !verified.status.success(),
        "cleared audit_head must not be repaired"
    );
}

#[test]
fn status_row_tampering_is_detected_by_the_lifecycle_invariant() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        r#"
package demo
workflow: {
    name: "status"
    steps: { a: { kind: "exec", exec: { cmd: ["sh", "-c", "exit 9"] } } }
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert_eq!(output.status.code(), Some(1));
    let run = run_id(&db);
    let connection = rusqlite::Connection::open(&db).expect("open db");
    connection
        .execute(
            "UPDATE runs SET status = 'succeeded', error = NULL WHERE id = ?1",
            [&run],
        )
        .expect("flip status");
    let verified = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(!verified.status.success());
    assert!(String::from_utf8_lossy(&verified.stderr).contains("audit chain ends in"));
}

#[test]
fn recomputed_state_hash_is_still_rejected_by_the_chain_anchor() {
    use ostinato_domain::{StepRecord, StepStatus, WorkflowSpec, state_digest};

    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        r#"
package demo
workflow: {
    name: "anchor"
    steps: { a: { kind: "exec", exec: { cmd: ["sh", "-c", "echo hi"] } } }
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(output.status.success());
    let run = run_id(&db);

    let connection = rusqlite::Connection::open(&db).expect("open db");
    let (spec_json, params_json, source_file, resume_index): (String, String, String, i64) =
        connection
            .query_row(
                "SELECT spec_json, params_json, source_file, resume_index FROM runs WHERE id = ?1",
                [&run],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("run row");
    let mut spec: WorkflowSpec = serde_json::from_str(&spec_json).expect("spec");
    let step = spec.steps.get_mut("a").expect("step a");
    step.exec.as_mut().expect("exec").cmd = vec!["sh".into(), "-c".into(), "echo forged".into()];
    let params: Value = serde_json::from_str(&params_json).expect("params");

    let mut statement = connection
        .prepare(
            "SELECT run_id, name, status, visits, attempts, output_json, error, updated_at
             FROM steps WHERE run_id = ?1",
        )
        .expect("prepare steps");
    let rows = statement
        .query_map([&run], |row| {
            let status: String = row.get(2)?;
            let output: Option<String> = row.get(5)?;
            Ok(StepRecord {
                run_id: row.get(0)?,
                name: row.get(1)?,
                status: match status.as_str() {
                    "succeeded" => StepStatus::Succeeded,
                    "failed" => StepStatus::Failed,
                    "skipped" => StepStatus::Skipped,
                    "running" => StepStatus::Running,
                    _ => StepStatus::Pending,
                },
                visits: row.get::<_, i64>(3)? as u32,
                attempts: row.get::<_, i64>(4)? as u32,
                output: output.map(|text| serde_json::from_str(&text).expect("output")),
                error: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .expect("query steps");
    let steps: Vec<StepRecord> = rows.map(|row| row.expect("step")).collect();
    drop(statement);

    let digest = state_digest(&spec, &params, &source_file, resume_index, &steps);
    connection
        .execute(
            "UPDATE runs SET spec_json = ?2, state_hash = ?3 WHERE id = ?1",
            rusqlite::params![
                run,
                serde_json::to_string(&spec).expect("serialize"),
                digest
            ],
        )
        .expect("tamper spec and recompute digest");

    let verified = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(!verified.status.success());
    assert!(String::from_utf8_lossy(&verified.stderr).contains("not anchored"));
}

#[test]
fn package_underscore_files_are_file_local() {
    let directory = tempfile::tempdir().expect("tempdir");
    let a = directory.path().join("a.cue");
    let b = directory.path().join("b.cue");
    write(
        &a,
        r#"
package _
params: {x: string | *"one"}
workflow: {name: "a", steps: {s: {kind: "noop"}}}
"#,
    );
    write(
        &b,
        r#"
package _
params: {y: string | *"two"}
extra: "ignored"
"#,
    );
    let output = ost(&[
        "--db",
        directory.path().join("db.sqlite").to_str().expect("db"),
        "--json",
        "validate",
        a.to_str().expect("a"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json(&output);
    assert_eq!(value["params"]["x"], "one");
    assert!(value["params"].get("y").is_none());
}

#[test]
fn non_struct_params_are_rejected() {
    let directory = tempfile::tempdir().expect("tempdir");
    let workflow = directory.path().join("params.cue");
    write(
        &workflow,
        r#"
package demo
params: "not-a-struct"
workflow: {name: "p", steps: {a: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        directory.path().join("db.sqlite").to_str().expect("db"),
        "validate",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("must be a struct"));
}

#[test]
fn quoted_labels_do_not_leak_evaluator_state_to_stderr() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("quoted.cue");
    write(
        &workflow,
        r#"
package demo
a: {"len": 1, b: len("ab")}
workflow: {name: "n", steps: {s: {kind: "noop"}}}
"#,
    );
    let db = db.to_str().expect("utf8 db");
    let workflow = workflow.to_str().expect("utf8 workflow");
    let output = ost(&["--db", db, "validate", workflow]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn tag_attributes_drive_parameter_injection_types() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("tagged.cue");
    write(
        &workflow,
        r#"
package demo
params: {
    s: string @tag(s, type=string)
    i: int @tag(i, type=int)
    b: bool @tag(b, type=bool)
    f: float @tag(f, type=number)
}
workflow: {
    name: "\(params.s)-\(params.i)-\(params.b)-\(params.f)"
    steps: {s: {kind: "noop"}}
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "s=5",
        "-p",
        "i=5",
        "-p",
        "b=true",
        "-p",
        "f=1.5",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["params"]["s"], "5");
    assert_eq!(report["params"]["i"], 5);
    assert_eq!(report["params"]["b"], true);
    assert_eq!(report["params"]["f"], 1.5);
    assert_eq!(report["workflow"]["name"], "5-5-true-1.5");
}

#[test]
fn tag_names_and_value_grammar_follow_cue() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("tagged-names.cue");
    write(
        &workflow,
        r#"
package demo
params: {
    renamed: string @tag(other, type=string)
    hex: int @tag(hex, type=int)
    flag: bool @tag(flag, type=bool)
}
workflow: {
    name: "\(params.renamed)-\(params.hex)-\(params.flag)"
    steps: {s: {kind: "noop"}}
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "other=5",
        "-p",
        "hex=0x10",
        "-p",
        "flag=TRUE",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["params"]["renamed"], "5");
    assert_eq!(report["params"]["hex"], 16);
    assert_eq!(report["params"]["flag"], true);
    assert_eq!(report["workflow"]["name"], "5-16-true");
}

#[test]
fn duplicate_and_nested_tags_follow_cue() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("tagged-duplicates.cue");
    write(
        &workflow,
        r#"
package demo
params: {
    a: string @tag(x)
    b: int @tag(x, type=int)
    inner: {
        a: int | *0 @tag(nestedOnly, type=int)
    }
}
workflow: {
    name: "\(params.a)-\(params.b)-\(params.inner.a)"
    steps: {s: {kind: "noop"}}
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "x=5",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["params"]["a"], "5");
    assert_eq!(report["params"]["b"], 5);
}

#[test]
fn tag_literal_grammar_and_strict_types() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("tagged-literals.cue");
    write(
        &workflow,
        r#"
package demo
params: {
    o: int @tag(o, type=int)
    b: int @tag(b, type=int)
    h: int @tag(h, type=int)
    n: number @tag(n, type=number)
    dot: number @tag(dot, type=number)
}
workflow: {name: "\(params.o)-\(params.b)-\(params.h)-\(params.n)-\(params.dot)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "o=0o10",
        "-p",
        "b=0b101",
        "-p",
        "h=-0x10",
        "-p",
        "n=.5",
        "-p",
        "dot=5.",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["params"]["o"], 8);
    assert_eq!(report["params"]["b"], 5);
    assert_eq!(report["params"]["h"], -16);
    assert_eq!(report["params"]["n"], 0.5);
    assert_eq!(report["params"]["dot"], 5.0);

    for (name, value) in [("o", "010"), ("b", "1__0"), ("n", "1_e1"), ("o", " 1 ")] {
        let output = ost(&[
            "--db",
            db.to_str().expect("db"),
            "validate",
            workflow.to_str().expect("workflow"),
            "-p",
            &format!("{name}={value}"),
            "-p",
            "h=1",
            "-p",
            "n=1",
            "-p",
            "dot=1",
        ]);
        assert!(!output.status.success(), "{name}={value}");
    }
}

#[test]
fn definition_backed_tags_work_in_both_evaluators() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("tagged-definition.cue");
    write(
        &workflow,
        r#"
package demo
#P: {
    name: string @tag(other)
    size: int @tag(size, type=int)
}
params: #P
workflow: {name: "\(params.name)-\(params.size)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "other=Alice",
        "-p",
        "size=0x10",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["params"]["name"], "Alice");
    assert_eq!(report["params"]["size"], 16);

    let Some(cue) = cue_command() else {
        return;
    };
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--cue",
        &cue,
        "--evaluator",
        "cue",
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "other=Alice",
        "-p",
        "size=0x10",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn tag_number_grammar_and_parenthesized_params() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("tagged-grammar.cue");
    write(
        &workflow,
        r#"
package demo
params: ({a: string @tag(x)}) & ({b: number @tag(y, type=number)})
workflow: {name: "\(params.a)-\(params.b)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "x=5",
        "-p",
        "y=00.5",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["params"]["a"], "5");
    assert_eq!(report["params"]["b"], 0.5);
}

#[test]
fn split_and_duplicate_definition_tags_work() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    write(
        &directory.path().join("a.cue"),
        r#"
package demo
#P: { name: string @tag(other), ... }
"#,
    );
    let workflow = directory.path().join("b.cue");
    write(
        &workflow,
        r#"
package demo
#Q: { size: *1 | int @tag(sz, type=int), ... }
params: (#P & #Q)
workflow: {name: "\(params.name)-\(params.size)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "other=Alice",
        "-p",
        "sz=0x10",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["params"]["name"], "Alice");
    assert_eq!(report["params"]["size"], 16);

    let duplicate = directory.path().join("duplicate.cue");
    write(
        &duplicate,
        r#"
package demo
#P: { name: *"Bob" | string @tag(other), ... }
#P: { size: *1 | int @tag(sz, type=int), ... }
params: #P
workflow: {name: "\(params.name)-\(params.size)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        duplicate.to_str().expect("workflow"),
        "-p",
        "other=Alice",
        "-p",
        "sz=16",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["params"]["name"], "Alice");
    assert_eq!(report["params"]["size"], 16);
}

#[test]
fn let_and_alias_backed_params_tags_work() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("tagged-let.cue");
    write(
        &workflow,
        r#"
package demo
#P: { name: *"a" | string @tag(other), ... }
let extra = { size: *1 | int @tag(sz, type=int), ... }
X = P2: { note: string @tag(note), ... }
params: (#P & extra & X)
workflow: {name: "\(params.name)-\(params.size)-\(params.note)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "other=Alice",
        "-p",
        "sz=0x10",
        "-p",
        "note=hello",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["params"]["name"], "Alice");
    assert_eq!(report["params"]["size"], 16);
    assert_eq!(report["params"]["note"], "hello");
}

#[test]
fn params_siblings_resolve_and_true_cycles_fail_cleanly() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("params-sibling.cue");
    write(
        &workflow,
        r#"
package demo
params: {a: int @tag(a, type=int), b: params.a}
workflow: {name: "\(params.b)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "a=2",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json(&output)["params"]["b"], 2);

    let cycle = directory.path().join("params-cycle.cue");
    write(
        &cycle,
        r#"
package demo
params: {a: params.a}
workflow: {name: "x", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "validate",
        cycle.to_str().expect("workflow"),
    ]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cyclic"), "stderr: {stderr}");
}

#[test]
fn bare_open_list_needs_and_params_compile() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("open.cue");
    write(
        &workflow,
        r#"
package demo
params: {tags: [...string]}
workflow: {
    name: "open"
    steps: {a: {kind: "noop", needs: [...string]}}
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["params"]["tags"], serde_json::json!([]));
}

#[test]
fn hidden_metadata_is_allowed_throughout_the_workflow_schema() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("hidden-metadata.cue");
    write(
        &workflow,
        r#"
package demo
params: {a: 1}
workflow: {
    name: "x"
    _h: 1
    steps: {
        s: {
            kind: "noop"
            _sh: 2
            retry: {_r: 1, max: 2}
            exec: {_e: 1, cmd: ["true"], env: {_env: "x", A: "1"}}
        }
    }
}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["workflow"]["steps"]["s"]["exec"]["env"]["A"], "1");
    assert!(
        report["workflow"]["steps"]["s"]["exec"]["env"]
            .get("\u{3}_env")
            .is_none()
    );
}

#[test]
fn nested_tags_inject_nested_parameters() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("nested-tags.cue");
    write(
        &workflow,
        r#"
package demo
params: {
    db: {
        host: string @tag(host)
        port: int @tag(port, type=int)
    }
    name: string @tag(name)
}
workflow: {
    name: "\(params.db.host):\(params.db.port):\(params.name)"
    steps: {s: {kind: "noop"}}
}
"#,
    );
    let arguments = [
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "host=db.example",
        "-p",
        "port=5432",
        "-p",
        "name=svc",
    ];
    let output = ost(&arguments);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["params"]["db"]["host"], "db.example");
    assert_eq!(report["params"]["db"]["port"], 5432);
    assert_eq!(report["params"]["name"], "svc");
    assert_eq!(report["workflow"]["name"], "db.example:5432:svc");

    let Some(cue) = cue_command() else {
        return;
    };
    let mut cue_arguments = vec![
        "--db",
        db.to_str().expect("db"),
        "--cue",
        &cue,
        "--evaluator",
        "cue",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "host=db.example",
        "-p",
        "port=5432",
        "-p",
        "name=svc",
    ];
    cue_arguments.shrink_to_fit();
    let output = ost(&cue_arguments);
    assert!(
        output.status.success(),
        "cue stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn nested_tags_work_through_definitions_and_reject_bad_types() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("nested-tag-definition.cue");
    write(
        &workflow,
        r#"
package demo
#P: {db: {host: string @tag(host), port: int @tag(port, type=int)}}
params: #P
workflow: {name: "\(params.db.host):\(params.db.port)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "host=x",
        "-p",
        "port=0x10",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["params"]["db"]["port"], 16);

    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "host=x",
        "-p",
        "port=nope",
    ]);
    assert!(!output.status.success());
}

#[test]
fn nested_tags_cover_definitions_hidden_quoted_and_shorthand() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");

    let definition_dir = directory.path().join("definition");
    std::fs::create_dir_all(&definition_dir).expect("definition dir");
    let definition = definition_dir.join("nested-tag-copy.cue");
    write(
        &definition,
        r#"
package demo
#P: {host: string | *"d" @tag(host), copy: host}
params: {nested: #P}
workflow: {name: "\(params.nested.host)|\(params.nested.copy)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        definition.to_str().expect("definition"),
        "-p",
        "host=x",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json(&output)["workflow"]["name"], "x|x");

    let hidden_dir = directory.path().join("hidden");
    std::fs::create_dir_all(&hidden_dir).expect("hidden dir");
    let hidden = hidden_dir.join("nested-tag-hidden.cue");
    write(
        &hidden,
        r#"
package demo
params: {_db: {host: string @tag(host)}, "_quoted": {host: string @tag(qhost)}}
workflow: {name: "\(params."_quoted".host)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        hidden.to_str().expect("hidden"),
        "-p",
        "host=x",
        "-p",
        "qhost=y",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json(&output)["params"]["_quoted"]["host"], "y");

    let shorthand_dir = directory.path().join("shorthand");
    std::fs::create_dir_all(&shorthand_dir).expect("shorthand dir");
    let shorthand = shorthand_dir.join("nested-tag-shorthand.cue");
    write(
        &shorthand,
        r#"
package demo
params: {db: host: string @tag(host)}
workflow: {name: "\(params.db.host)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        shorthand.to_str().expect("shorthand"),
        "-p",
        "host=x",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json(&output)["params"]["db"]["host"], "x");
}

#[test]
fn recursive_definitions_do_not_overflow_tag_collection() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("recursive-tag.cue");
    write(
        &workflow,
        r#"
package demo
#Node: {next?: #Node, name: string | *"n" @tag(name)}
params: {nested: #Node}
workflow: {name: "\(params.nested.name)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "name=x",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json(&output)["params"]["nested"]["name"], "x");
}

#[test]
fn nested_definition_tags_inject_their_declaration() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("nested-definition-tag.cue");
    write(
        &workflow,
        r#"
package demo
#Outer: {#Inner: {host: string | *"d" @tag(host), copy: host}, inner: #Inner}
params: {n: #Outer}
workflow: {name: "\(params.n.inner.host)|\(params.n.inner.copy)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "host=x",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json(&output);
    assert_eq!(report["workflow"]["name"], "x|x");
    assert_eq!(report["params"]["n"]["inner"]["copy"], "x");
    assert!(report["params"]["n"].get("#Inner").is_none());
}

#[test]
fn leaf_tag_injection_preserves_lexical_scopes() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let cases = [
        (
            "quoted",
            r#"
package demo
params: {"_q": {host: string | *"d" @tag(host), copy: host}}
workflow: {name: "\(params."_q".host)|\(params."_q".copy)", steps: {s: {kind: "noop"}}}
"#,
            "x|x",
        ),
        (
            "group-definition",
            r#"
package demo
#Outer: {grp: {#Inner: {host: string | *"d" @tag(host), copy: host}}, inner: grp.#Inner}
params: {n: #Outer}
workflow: {name: "\(params.n.inner.host)|\(params.n.inner.copy)", steps: {s: {kind: "noop"}}}
"#,
            "x|x",
        ),
        (
            "inner-definition",
            r#"
package demo
params: {#Inner: {host: string | *"d" @tag(host)}, inner: #Inner}
workflow: {name: "\(params.inner.host)", steps: {s: {kind: "noop"}}}
"#,
            "x",
        ),
        (
            "alias",
            r#"
package demo
A=#P: {host: string | *"d" @tag(host), copy: host}
params: {n: A}
workflow: {name: "\(params.n.host)|\(params.n.copy)", steps: {s: {kind: "noop"}}}
"#,
            "x|x",
        ),
        (
            "outer-let",
            r#"
package demo
let extra = {host: string | *"d" @tag(host), copy: host}
params: {e: extra}
workflow: {name: "\(params.e.host)|\(params.e.copy)", steps: {s: {kind: "noop"}}}
"#,
            "x|x",
        ),
        (
            "local-let",
            r#"
package demo
params: {let extra = {host: string | *"d" @tag(host), copy: host}, e: extra}
workflow: {name: "\(params.e.host)|\(params.e.copy)", steps: {s: {kind: "noop"}}}
"#,
            "x|x",
        ),
    ];
    for (name, source, expected) in cases {
        let case_dir = directory.path().join(name);
        std::fs::create_dir_all(&case_dir).expect("case dir");
        let workflow = case_dir.join("main.cue");
        write(&workflow, source);
        let output = ost(&[
            "--db",
            db.to_str().expect("db"),
            "--json",
            "validate",
            workflow.to_str().expect("workflow"),
            "-p",
            "host=x",
        ]);
        assert!(
            output.status.success(),
            "{name} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(json(&output)["workflow"]["name"], expected, "{name}");
    }

    let unknown_dir = directory.path().join("unknown");
    std::fs::create_dir_all(&unknown_dir).expect("unknown dir");
    let unknown = unknown_dir.join("main.cue");
    write(
        &unknown,
        r#"
package demo
params: {db: {host: string | *"localhost" @tag(host)}}
workflow: {name: "\(params.db.host)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "validate",
        unknown.to_str().expect("unknown"),
        "-p",
        "zzz=9",
    ]);
    assert!(
        !output.status.success(),
        "unknown parameter must be rejected"
    );
}
