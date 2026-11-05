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
    let mut command = Command::new(bin());
    command.args(args);
    if args.first() == Some(&"cmd")
        && let Some(path) = args.last()
        && Path::new(path).is_dir()
    {
        command.current_dir(path);
    }
    command.output().expect("run ostinato")
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
            "SELECT run_id, name, status, visits, attempts, output_json, error, updated_at, round
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
                round: row.get::<_, i64>(8)? as u32,
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

#[test]
fn tag_identity_handles_shadowing_selectors_and_disjunctions() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("tag-identity.cue");
    write(
        &workflow,
        r#"
package demo
config: database: {host: string | *"d" @tag(dhost), copy: host}
#P: {host: string | *"d" @tag(phost), copy: host}
params: {
    a: #P
    b: {#P: {host: string | *"d"}, v: #P}
    db: config.database
    g: *{host: string | *"d"} | {host: string | *"e" @tag(ghost)}
}
workflow: {
    name: "\(params.a.host)|\(params.a.copy)|\(params.b.v.host)|\(params.db.host)|\(params.db.copy)|\(params.g.host)"
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
        "dhost=x",
        "-p",
        "phost=y",
        "-p",
        "ghost=z",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json(&output)["workflow"]["name"], "y|y|d|x|x|d");
}

#[test]
fn selector_tags_cover_leaves_duplicates_and_shadowed_definitions() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("selector-tags.cue");
    write(
        &workflow,
        r#"
package demo
config: database: string | *"d" @tag(leaf)
config2: database: {host: string | *"d" @tag(h)}
config2: database: {port: int | *1 @tag(p, type=int)}
#P: {host: int | *1 @tag(n, type=int)}
helper: {#P: {port: int | *1 @tag(n, type=int)}, p: #P}
params: {
    leaf: config.database
    db: config2.database
    a: #P
    b: helper.p
}
workflow: {
    name: "\(params.leaf)|\(params.db.host):\(params.db.port)|\(params.a.host)|\(params.b.port)"
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
        "leaf=Z",
        "-p",
        "h=H",
        "-p",
        "p=9",
        "-p",
        "n=5",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json(&output)["workflow"]["name"], "Z|H:9|5|5");
}

#[test]
fn selector_tags_follow_definitions_duplicates_and_scope() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("selector-scope.cue");
    write(
        &workflow,
        r#"
package demo
#Config: {database: {host: string | *"d" @tag(h), copy: host}}
config: #Config
A: {host: string | *"d" @tag(a)}
B: {port: int | *1 @tag(b, type=int)}
local: database: {def: A, def: B, db: def}
#P: {port: int | *1 @tag(n, type=int)}
helper: {#P: {port: int | *1 @tag(n, type=int)}, direct: #P.port}
params: {
    db: config.database
    x: local.database.db
    b: helper.direct
    other: {port: int | *1 @tag(n, type=int)}
}
workflow: {
    name: "\(params.db.host)|\(params.db.copy)|\(params.x.port)|\(params.b)|\(params.other.port)"
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
        "h=x",
        "-p",
        "b=2",
        "-p",
        "n=5",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json(&output)["workflow"]["name"], "x|x|2|5|5");
}

#[test]
fn nested_tags_follow_label_identity_depth_and_expression_bases() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let cases = [
        (
            "quoted-identity",
            r#"
package demo
config: database: {host: string | *"d" @tag(host), copy: host}
quoted: {"database": {port: int | *1 @tag(port, type=int)}}
params: {
    db: config["database"]["host"]
    full: config["database"]
    q: quoted.database.port
}
workflow: {name: "\(params.db)|\(params.full.copy)|\(params.q)", steps: {s: {kind: "noop"}}}
"#,
            &["-p", "host=x", "-p", "port=9"][..],
            "x|x|9",
        ),
        (
            "depth-shadow",
            r#"
package demo
#P: {port: int | *1 @tag(n, type=int)}
helper: {#P: {port: int | *1 @tag(n, type=int)}, wrap: {direct: #P.port}}
params: {b: helper.wrap.direct, other: {port: int | *1 @tag(n, type=int)}}
workflow: {name: "\(params.b):\(params.other.port)", steps: {s: {kind: "noop"}}}
"#,
            &["-p", "n=5"][..],
            "5:5",
        ),
        (
            "expression-bases",
            r#"
package demo
config: database: {host: string | *"d" @tag(host), copy: host}
h: config.database
params: {
    x: h.host
    c: h.copy
    inline: ({port: int | *1 @tag(port, type=int)}).port
    unified: ({a: int} & {b: int | *2 @tag(b, type=int)}).b
    chosen: (*{a: int | *3} | {b: int | *4 @tag(unused, type=int)}).a
}
workflow: {name: "\(params.x)|\(params.c)|\(params.inline)|\(params.unified)|\(params.chosen)", steps: {s: {kind: "noop"}}}
"#,
            &[
                "-p", "host=x", "-p", "port=7", "-p", "b=8", "-p", "unused=9",
            ][..],
            "x|x|7|8|3",
        ),
    ];
    for (name, source, overrides, expected) in cases {
        let case_dir = directory.path().join(name);
        std::fs::create_dir_all(&case_dir).expect("case dir");
        let workflow = case_dir.join("main.cue");
        write(&workflow, source);
        let mut arguments = vec![
            "--db",
            db.to_str().expect("db"),
            "--json",
            "validate",
            workflow.to_str().expect("workflow"),
        ];
        arguments.extend_from_slice(overrides);
        let output = ost(&arguments);
        assert!(
            output.status.success(),
            "{name} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(json(&output)["workflow"]["name"], expected, "{name}");
    }
}

#[test]
fn nested_tag_aliases_resolve_at_their_written_scope() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("alias-scope.cue");
    write(
        &workflow,
        r#"
package demo
A=#Q: {port: int | *1 @tag(glob, type=int)}
outer: {
    #Q: {port: int | *2 @tag(loc, type=int)}
    v: A.port
    w: #Q.port
}
C=#S: {port: int | *1 @tag(n, type=int)}
outer2: {
    #S: {port: int | *2 @tag(n, type=int)}
    v: C.port
    w: #S.port
}
params: {x: outer.v, y: outer.w, m: outer2.v, k: outer2.w}
workflow: {name: "\(params.x)|\(params.y)|\(params.m)|\(params.k)", steps: {s: {kind: "noop"}}}
"#,
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        "glob=5",
        "-p",
        "loc=6",
        "-p",
        "n=7",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json(&output)["workflow"]["name"], "5|6|7|7");
}

#[test]
fn recursive_definitions_materialize_in_both_evaluators() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("recursive.cue");
    write(
        &workflow,
        r#"
package demo
#Tree: {value: int, children?: [...#Tree]}
#Node: {value: int, next?: #Node}
#MutualA: {name: string, b?: #MutualB}
#MutualB: {name: string, a?: #MutualA}
tree: #Tree & {value: 1, children: [{value: 2}, {value: 3, children: [{value: 4}]}]}
node: #Node & {value: 1, next: {value: 2, next: {value: 3}}}
mutual: #MutualA & {name: "a", b: {name: "b", a: {name: "c"}}}
params: {t: tree, n: node, m: mutual}
workflow: {name: "recursive", steps: {s: {kind: "noop"}}}
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
    assert_eq!(
        report["params"]["t"]["children"][1]["children"][0]["value"],
        4
    );
    assert_eq!(report["params"]["n"]["next"]["next"]["value"], 3);
    assert_eq!(report["params"]["m"]["b"]["a"]["name"], "c");

    let Some(cue) = cue_command() else {
        return;
    };
    let oracle = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--cue",
        &cue,
        "--evaluator",
        "cue",
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        oracle.status.success(),
        "cue stderr: {}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    assert_eq!(json(&oracle)["params"], report["params"]);
}

#[test]
fn deep_recursive_values_do_not_exhaust_the_stack() {
    fn tree(depth: usize) -> String {
        let mut value = format!("{{value: {depth}}}");
        for index in (1..depth).rev() {
            value = format!("{{value: {index}, children: [{value}]}}");
        }
        value
    }
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = directory.path().join("deep.cue");
    write(
        &workflow,
        &format!(
            "package demo\n#Tree: {{value: int, children?: [...#Tree]}}\ntree: #Tree & {}\nparams: {{out: tree}}\nworkflow: {{name: \"deep\", steps: {{s: {{kind: \"noop\"}}}}}}\n",
            tree(800)
        ),
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "validate",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn multiline_and_interpolated_bytes_match_both_evaluators() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let workflow = directory.path().join("bytes.cue");
    write(
        &workflow,
        r#"
package demo
params: {
	plain: '''
line1
line2
'''
	interp: 'x\(1+2)y'
	scalars: 's=\(params.s) n=\(params.n) b=\(params.b)'
	s: "text"
	n: 42
	b: 'raw'
	indented: '''
    value=\(1+2)
      nested
    '''
	text: """
line1
line2
"""
	text_interp: """
  value=\(1+2)
  """
	triple: """
a\"""b
"""
}
workflow: {name: "bytes", steps: {s: {kind: "noop"}}}
"#,
    );
    let db_rust = directory.path().join("rust.sqlite");
    let rust = ost(&[
        "--db",
        db_rust.to_str().expect("db"),
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        rust.status.success(),
        "rust stderr: {}",
        String::from_utf8_lossy(&rust.stderr)
    );

    let db_cue = directory.path().join("cue.sqlite");
    let oracle = ost(&[
        "--db",
        db_cue.to_str().expect("db"),
        "--cue",
        &cue,
        "--evaluator",
        "cue",
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        oracle.status.success(),
        "cue stderr: {}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    assert_eq!(json(&rust)["params"], json(&oracle)["params"]);
}

fn write_gateway_workflow(directory: &Path, workflow: &str) -> PathBuf {
    let path = directory.join("gateway.cue");
    write(&path, workflow);
    path
}

fn run_workflow(db: &Path, workflow: &Path) -> Output {
    ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ])
}

fn status_text(db: &Path, run: &str) -> String {
    let output = ost(&["--db", db.to_str().expect("db"), "status", run]);
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn logs_json(db: &Path, run: &str) -> Vec<Value> {
    let output = ost(&["--db", db.to_str().expect("db"), "--json", "logs", run]);
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("event json"))
        .collect()
}

fn metrics_json(db: &Path, run: &str) -> Value {
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "metrics",
        "--run",
        run,
        "--format",
        "json",
    ]);
    assert!(output.status.success());
    json(&output)
}

fn trials_for(db: &Path, run: &str) -> Vec<Value> {
    let output = ost(&["--db", db.to_str().expect("db"), "--json", "trials", run]);
    assert!(output.status.success());
    json(&output).as_array().cloned().unwrap_or_default()
}

#[test]
fn gateway_xor_routes_one_branch_and_skips_the_other() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let trace = directory.path().join("trace");
    let workflow = write_gateway_workflow(
        directory.path(),
        &format!(
            r#"
package demo
workflow: {{
    name: "xor"
    steps: {{
        test: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo '{{\"score\":0.95}}'"] }} }}
        route: {{
            kind: "gateway"
            needs: ["test"]
            gateway: {{
                split: "xor"
                branches: {{
                    deployFast: "steps.test.score >= 0.9"
                    deploySlow: true
                }}
                default: "deploySlow"
            }}
        }}
        deployFast: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo fast >> {trace}"] }} }}
        deploySlow: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo slow >> {trace}"] }} }}
        merge: {{ kind: "gateway", needs: ["deployFast", "deploySlow"], gateway: {{ join: "or" }} }}
        notify: {{ kind: "exec", needs: ["merge"], exec: {{ cmd: ["sh", "-c", "echo notify >> {trace}"] }} }}
    }}
}}
"#,
            trace = trace.display()
        ),
    );
    let output = run_workflow(&db, &workflow);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = run_id(&db);
    let trace = std::fs::read_to_string(&trace).expect("trace");
    assert_eq!(trace.lines().collect::<Vec<_>>(), vec!["fast", "notify"]);

    let status = status_text(&db, &run);
    assert!(
        status
            .lines()
            .any(|line| line.starts_with("deployFast ") && line.contains("succeeded"))
    );
    assert!(
        status
            .lines()
            .any(|line| line.starts_with("deploySlow ") && line.contains("skipped"))
    );

    let trials = trials_for(&db, &run);
    let names: Vec<&str> = trials
        .iter()
        .filter_map(|trial| trial["step"].as_str())
        .collect();
    assert!(!names.contains(&"route"), "gateways must not record trials");
    assert!(!names.contains(&"merge"), "gateways must not record trials");
    assert!(
        !names.contains(&"deploySlow"),
        "skipped steps must not record trials"
    );

    let metrics = metrics_json(&db, &run);
    assert_eq!(metrics["stepsSkipped"], 1);
    assert_eq!(metrics["gatewaysEvaluated"], 2);

    let text = ost(&["--db", db.to_str().expect("db"), "metrics", "--run", &run]);
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(text.contains("steps_skipped     1"), "{text}");
    assert!(text.contains("gateways_evaluated 2"), "{text}");
    let prometheus = ost(&[
        "--db",
        db.to_str().expect("db"),
        "metrics",
        "--run",
        &run,
        "--format",
        "prometheus",
    ]);
    let prometheus = String::from_utf8_lossy(&prometheus.stdout);
    assert!(
        prometheus.contains("ostinato_steps_skipped_total 1"),
        "{prometheus}"
    );
    assert!(
        prometheus.contains("ostinato_gateways_evaluated_total 2"),
        "{prometheus}"
    );
}

#[test]
fn gateway_xor_default_branch_runs_when_no_condition_matches() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let trace = directory.path().join("trace");
    let workflow = write_gateway_workflow(
        directory.path(),
        &format!(
            r#"
package demo
workflow: {{
    name: "default"
    steps: {{
        test: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo '{{\"score\":0.1}}'"] }} }}
        route: {{
            kind: "gateway"
            needs: ["test"]
            gateway: {{
                split: "xor"
                branches: {{
                    deployFast: "steps.test.score >= 0.9"
                    deploySlow: true
                }}
                default: "deploySlow"
            }}
        }}
        deployFast: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo fast >> {trace}"] }} }}
        deploySlow: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo slow >> {trace}"] }} }}
        merge: {{ kind: "gateway", needs: ["deployFast", "deploySlow"], gateway: {{ join: "or" }} }}
    }}
}}
"#,
            trace = trace.display()
        ),
    );
    let output = run_workflow(&db, &workflow);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let trace = std::fs::read_to_string(&trace).expect("trace");
    assert_eq!(trace.lines().collect::<Vec<_>>(), vec!["slow"]);
}

#[test]
fn gateway_or_runs_every_true_branch() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let trace = directory.path().join("trace");
    let workflow = write_gateway_workflow(
        directory.path(),
        &format!(
            r#"
package demo
workflow: {{
    name: "or"
    steps: {{
        route: {{
            kind: "gateway"
            gateway: {{ split: "or", branches: {{
                left: "true"
                right: "true"
            }} }}
        }}
        left: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo left >> {trace}"] }} }}
        right: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo right >> {trace}"] }} }}
        merge: {{ kind: "gateway", needs: ["left", "right"], gateway: {{ join: "or" }} }}
    }}
}}
"#,
            trace = trace.display()
        ),
    );
    let output = run_workflow(&db, &workflow);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = run_id(&db);
    let trace = std::fs::read_to_string(&trace).expect("trace");
    let lines: Vec<&str> = trace.lines().collect();
    assert!(lines.contains(&"left") && lines.contains(&"right"));
    let metrics = metrics_json(&db, &run);
    assert_eq!(metrics["stepsSkipped"], 0);
    assert_eq!(metrics["gatewaysEvaluated"], 2);
}

#[test]
fn gateway_and_fan_out_and_fan_in_wait_for_both_branches() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let trace = directory.path().join("trace");
    let workflow = write_gateway_workflow(
        directory.path(),
        &format!(
            r#"
package demo
workflow: {{
    name: "and"
    steps: {{
        fanout: {{ kind: "gateway", gateway: {{ split: "and", branches: {{ unit: true, integration: true }} }} }}
        unit: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo unit >> {trace}"] }} }}
        integration: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo integration >> {trace}"] }} }}
        fanin: {{ kind: "gateway", needs: ["unit", "integration"], gateway: {{ join: "and" }} }}
        done: {{ kind: "exec", needs: ["fanin"], exec: {{ cmd: ["sh", "-c", "echo done >> {trace}"] }} }}
    }}
}}
"#,
            trace = trace.display()
        ),
    );
    let output = run_workflow(&db, &workflow);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let trace = std::fs::read_to_string(&trace).expect("trace");
    let lines: Vec<&str> = trace.lines().collect();
    assert!(lines.contains(&"unit") && lines.contains(&"integration"));
    assert_eq!(lines.last().copied(), Some("done"));
    let run = run_id(&db);
    let metrics = metrics_json(&db, &run);
    assert_eq!(metrics["stepsSkipped"], 0);
    assert_eq!(metrics["gatewaysEvaluated"], 2);
}

#[test]
fn gateway_dead_path_propagates_and_records_skips() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = write_gateway_workflow(
        directory.path(),
        r#"
package demo
workflow: {
    name: "dead"
    steps: {
        route: {
            kind: "gateway"
            gateway: { split: "xor", branches: {
                live: "true"
                dead: true
            }, default: "dead" }
        }
        live: { kind: "noop" }
        dead: { kind: "noop" }
        dead2: { kind: "noop", needs: ["dead"] }
        dead3: { kind: "noop", needs: ["dead2"] }
        merge: { kind: "gateway", needs: ["live", "dead3"], gateway: { join: "or" } }
    }
}
"#,
    );
    let output = run_workflow(&db, &workflow);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = run_id(&db);
    let status = status_text(&db, &run);
    for name in ["dead", "dead2", "dead3"] {
        assert!(
            status
                .lines()
                .any(|line| line.starts_with(name) && line.contains("skipped")),
            "{name} must be skipped:\n{status}"
        );
    }
    let trials = trials_for(&db, &run);
    assert!(
        trials.iter().all(
            |trial| !["dead", "dead2", "dead3"].contains(&trial["step"].as_str().unwrap_or(""))
        ),
        "skipped steps must not record trials"
    );
    let skipped_events = logs_json(&db, &run)
        .into_iter()
        .filter(|event| event["kind"] == "step.skipped")
        .count();
    assert_eq!(skipped_events, 3);
}

#[test]
fn gateway_routes_on_continue_on_error_status() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let trace = directory.path().join("trace");
    let workflow = write_gateway_workflow(
        directory.path(),
        &format!(
            r#"
package demo
workflow: {{
    name: "failure-routing"
    steps: {{
        check: {{ kind: "exec", continueOnError: true, exec: {{ cmd: ["sh", "-c", "exit 3"] }} }}
        route: {{
            kind: "gateway"
            needs: ["check"]
            gateway: {{
                split: "xor"
                branches: {{
                    failure: "stepStatus.check == \"failed\""
                    success: true
                }}
                default: "success"
            }}
        }}
        failure: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo failure >> {trace}"] }} }}
        success: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo success >> {trace}"] }} }}
        merge: {{ kind: "gateway", needs: ["failure", "success"], gateway: {{ join: "or" }} }}
    }}
}}
"#,
            trace = trace.display()
        ),
    );
    let output = run_workflow(&db, &workflow);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let trace = std::fs::read_to_string(&trace).expect("trace");
    assert_eq!(trace.lines().collect::<Vec<_>>(), vec!["failure"]);
    let run = run_id(&db);
    let status = status_text(&db, &run);
    assert!(
        status
            .lines()
            .any(|line| line.starts_with("success ") && line.contains("skipped"))
    );
}

#[test]
fn gateway_failure_is_recorded_and_keeps_resume_index_at_the_gateway_group() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = write_gateway_workflow(
        directory.path(),
        r#"
package demo
params: { never: bool | *false }
workflow: {
    name: "gateway-failure"
    steps: {
        test: { kind: "noop" }
        route: {
            kind: "gateway"
            needs: ["test"]
            gateway: { split: "xor", branches: {
                a: "params.never == true"
            } }
        }
        a: { kind: "noop" }
        merge: { kind: "gateway", needs: ["a"], gateway: { join: "or" } }
    }
}
"#,
    );
    let output = run_workflow(&db, &workflow);
    assert_eq!(output.status.code(), Some(1));
    let events = logs_json(&db, &run_id(&db));
    assert!(events.iter().any(|event| event["kind"] == "gateway.failed"));
    assert!(events.iter().any(|event| event["kind"] == "run.failed"));
    let connection = rusqlite::Connection::open(&db).expect("open db");
    let resume_index: i64 = connection
        .query_row("SELECT resume_index FROM runs LIMIT 1", [], |row| {
            row.get(0)
        })
        .expect("resume index");
    assert_eq!(resume_index, 1, "gateway is the second plan node");
}

#[test]
fn gateway_resume_after_split_does_not_re_evaluate_or_run_dead_branch() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let flag = directory.path().join("go");
    let trace = directory.path().join("trace");
    let workflow = write_gateway_workflow(
        directory.path(),
        &format!(
            r#"
package demo
workflow: {{
    name: "gateway-resume"
    steps: {{
        route: {{
            kind: "gateway"
            gateway: {{ split: "xor", branches: {{
                active: "true"
                dead: true
            }}, default: "dead" }}
        }}
        active: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "test -f {flag} || exit 7; echo active >> {trace}"] }} }}
        dead: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo dead >> {trace}"] }} }}
        merge: {{ kind: "gateway", needs: ["active", "dead"], gateway: {{ join: "or" }} }}
        done: {{ kind: "exec", needs: ["merge"], exec: {{ cmd: ["sh", "-c", "echo done >> {trace}"] }} }}
    }}
}}
"#,
            flag = flag.display(),
            trace = trace.display()
        ),
    );
    let first = run_workflow(&db, &workflow);
    assert!(!first.status.success());
    let run = run_id(&db);
    std::fs::write(&flag, b"go").expect("flag");
    let resumed = ost(&["--db", db.to_str().expect("db"), "resume", &run]);
    assert!(
        resumed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&resumed.stderr)
    );

    let trace = std::fs::read_to_string(&trace).expect("trace");
    assert_eq!(trace.lines().collect::<Vec<_>>(), vec!["active", "done"]);
    let events = logs_json(&db, &run);
    let evaluated = events
        .iter()
        .filter(|event| event["kind"] == "gateway.evaluated")
        .count();
    assert_eq!(evaluated, 1, "the split must not be re-evaluated on resume");
    let dead_skips = events
        .iter()
        .filter(|event| event["kind"] == "step.skipped" && event["step"] == "dead")
        .count();
    assert_eq!(
        dead_skips, 1,
        "a committed skip must not be re-emitted on resume"
    );
}

#[test]
fn gateway_audit_verifies_and_detects_tampered_gateway_output() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = write_gateway_workflow(
        directory.path(),
        r#"
package demo
workflow: {
    name: "audit"
    steps: {
        route: {
            kind: "gateway"
            gateway: { split: "xor", branches: {
                a: "true"
                b: true
            }, default: "b" }
        }
        a: { kind: "noop" }
        b: { kind: "noop" }
        merge: { kind: "gateway", needs: ["a", "b"], gateway: { join: "or" } }
    }
}
"#,
    );
    let output = run_workflow(&db, &workflow);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = run_id(&db);
    let verified = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(verified.status.success());

    let connection = rusqlite::Connection::open(&db).expect("open db");
    connection
        .execute(
            "UPDATE steps SET output_json = '{\"chosen\":[\"b\"],\"kind\":\"gateway\"}' WHERE run_id = ?1 AND name = 'route'",
            rusqlite::params![run],
        )
        .expect("tamper gateway output");
    let tampered = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(
        !tampered.status.success(),
        "tampered state must fail verification"
    );
    let stderr = String::from_utf8_lossy(&tampered.stderr);
    assert!(
        stderr.contains("AUDIT FAILED") || stderr.contains("digest"),
        "{stderr}"
    );
}

#[test]
fn gateway_graph_dot_and_json_expose_routing_metadata() {
    let directory = tempfile::tempdir().expect("tempdir");
    let workflow = write_gateway_workflow(
        directory.path(),
        r#"
package demo
workflow: {
    name: "graph"
    steps: {
        test: { kind: "noop" }
        route: {
            kind: "gateway"
            needs: ["test"]
            gateway: { split: "xor", branches: {
                fast: "steps.test.stdout == \"ok\""
                slow: true
            }, default: "slow" }
        }
        fast: { kind: "noop" }
        slow: { kind: "noop" }
        merge: { kind: "gateway", needs: ["fast", "slow"], gateway: { join: "or" } }
    }
}
"#,
    );
    let dot = ost(&[
        "--evaluator",
        "native",
        "graph",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(dot.status.success());
    let dot = String::from_utf8_lossy(&dot.stdout);
    assert_eq!(dot.matches("shape=diamond").count(), 2, "{dot}");
    assert!(dot.contains("steps.test.stdout == \\\"ok\\\""), "{dot}");
    assert!(dot.contains("default: true"), "{dot}");
    assert!(dot.contains("join: or"), "{dot}");

    let plan = ost(&[
        "--evaluator",
        "native",
        "graph",
        "--format",
        "json",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(plan.status.success());
    let plan = json(&plan);
    assert_eq!(plan["has_gateways"], true);
    let nodes = plan["nodes"].as_array().expect("nodes");
    let route = nodes
        .iter()
        .find(|node| node["step"] == "route")
        .expect("route node");
    assert_eq!(route["gateway"]["split"], "xor");
    let fast = nodes
        .iter()
        .find(|node| node["step"] == "fast")
        .expect("fast node");
    assert_eq!(fast["incoming"][0]["origin"]["branch"]["gateway"], "route");
    let merge = nodes
        .iter()
        .find(|node| node["step"] == "merge")
        .expect("merge node");
    assert_eq!(merge["gateway"]["join"], "or");
    assert_eq!(merge["incoming"].as_array().expect("incoming").len(), 2);
}

#[test]
fn gateway_audit_detects_a_tampered_skipped_row() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = write_gateway_workflow(
        directory.path(),
        r#"
package demo
workflow: {
    name: "skip-tamper"
    steps: {
        route: {
            kind: "gateway"
            gateway: { split: "xor", branches: { a: "true", b: true }, default: "b" }
        }
        a: { kind: "noop" }
        b: { kind: "noop" }
        merge: { kind: "gateway", needs: ["a", "b"], gateway: { join: "or" } }
    }
}
"#,
    );
    let output = run_workflow(&db, &workflow);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = run_id(&db);
    let connection = rusqlite::Connection::open(&db).expect("open db");
    connection
        .execute(
            "UPDATE steps SET status = 'succeeded' WHERE run_id = ?1 AND name = 'b'",
            rusqlite::params![run],
        )
        .expect("tamper skipped row");
    let tampered = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(
        !tampered.status.success(),
        "tampered skip must fail verification"
    );
    let stderr = String::from_utf8_lossy(&tampered.stderr);
    assert!(
        stderr.contains("AUDIT FAILED") || stderr.contains("digest"),
        "{stderr}"
    );
}

#[test]
fn legacy_run_without_gateways_still_resumes_and_verifies() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let flag = directory.path().join("go");
    let workflow = directory.path().join("legacy.cue");
    write(
        &workflow,
        &format!(
            r#"
package demo
workflow: {{
    name: "legacy"
    steps: {{
        a: {{ kind: "exec", exec: {{ cmd: ["sh", "-c", "echo a"] }} }}
        b: {{ kind: "exec", needs: ["a"], exec: {{ cmd: ["sh", "-c", "test -f {flag} || exit 7"] }} }}
    }}
}}
"#,
            flag = flag.display()
        ),
    );
    let first = run_workflow(&db, &workflow);
    assert!(!first.status.success());
    let run = run_id(&db);
    let connection = rusqlite::Connection::open(&db).expect("open db");
    let spec_json: String = connection
        .query_row(
            "SELECT spec_json FROM runs WHERE id = ?1",
            rusqlite::params![run],
            |row| row.get(0),
        )
        .expect("spec json");
    assert!(
        !spec_json.contains("gateway"),
        "legacy spec must not gain a gateway key"
    );
    assert!(
        ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run])
            .status
            .success()
    );
    std::fs::write(&flag, b"go").expect("flag");
    let resumed = ost(&["--db", db.to_str().expect("db"), "resume", &run]);
    assert!(
        resumed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    assert!(
        ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run])
            .status
            .success()
    );
}

#[test]
fn gateway_n_of_join_runs_when_quorum_is_met() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = write_gateway_workflow(
        directory.path(),
        r#"
package demo
workflow: {
    name: "nof"
    steps: {
        a: {kind: "noop"}
        b: {kind: "noop"}
        c: {kind: "noop"}
        q: {kind: "gateway", needs: ["a", "b", "c"], gateway: {join: "n_of", n: 2}}
    }
}
"#,
    );
    let run = run_workflow(&db, &workflow);
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let id = run_id(&db);
    let status = status_text(&db, &id);
    assert!(
        status.contains("q") && status.contains("succeeded"),
        "{status}"
    );
    assert!(
        ost(&["--db", db.to_str().expect("db"), "audit", "verify", &id])
            .status
            .success()
    );
}

#[test]
fn gateway_n_of_partial_quorum_fails_the_run() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = write_gateway_workflow(
        directory.path(),
        r#"
package demo
workflow: {
    name: "nof-partial"
    steps: {
        route: {kind: "gateway", gateway: {split: "xor", branches: {a: "true", b: true}, default: "b"}}
        a: {kind: "noop"}
        b: {kind: "noop"}
        q: {kind: "gateway", needs: ["a", "b"], gateway: {join: "n_of", n: 2}}
    }
}
"#,
    );
    let run = run_workflow(&db, &workflow);
    assert!(!run.status.success());
    let stdout = String::from_utf8_lossy(&run.stdout);
    let id = stdout
        .split("run=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .expect("run id in output")
        .to_owned();
    let events = logs_json(&db, &id);
    assert!(events.iter().any(|event| event["kind"] == "gateway.failed"));
    let metrics = metrics_json(&db, &id);
    assert!(
        metrics["gatewayFailures"].as_u64().unwrap_or_default() >= 1,
        "{metrics}"
    );
    assert!(
        metrics["gatewayQuorumShort"].as_u64().unwrap_or_default() >= 1,
        "{metrics}"
    );
}

#[test]
fn gateway_expression_join_runs_when_the_expression_is_true() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = write_gateway_workflow(
        directory.path(),
        r#"
package demo
workflow: {
    name: "expression"
    steps: {
        a: {kind: "noop"}
        b: {kind: "noop"}
        m: {kind: "gateway", needs: ["a", "b"], gateway: {join: "expression", expression: "edges.a && edges.b"}}
    }
}
"#,
    );
    let run = run_workflow(&db, &workflow);
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let id = run_id(&db);
    assert!(status_text(&db, &id).contains("m"));
}

#[test]
fn gateway_cycle_runs_rounds_and_audits() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = write_gateway_workflow(
        directory.path(),
        r#"
package demo
params: { go: bool | *false }
workflow: {
    name: "cycle-gateway"
    maxParallel: 1
    steps: {
        loop: {kind: "noop", needs: ["tail"], cycle: {max: 3, until: "round >= 1"}}
        split: {kind: "gateway", needs: ["loop"], gateway: {
            split: "xor"
            branches: {work: "params.go", idle: true}
            default: "idle"
        }}
        work: {kind: "noop"}
        idle: {kind: "noop"}
        merge: {kind: "gateway", needs: ["work", "idle"], gateway: {join: "or"}}
        tail: {kind: "noop", needs: ["merge"]}
    }
}
"#,
    );
    let run = run_workflow(&db, &workflow);
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let id = run_id(&db);
    let events = logs_json(&db, &id);
    assert!(
        events
            .iter()
            .any(|event| event["kind"] == "cycle.completed")
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["kind"] == "gateway.evaluated")
            .count(),
        2
    );
    assert!(
        ost(&["--db", db.to_str().expect("db"), "audit", "verify", &id])
            .status
            .success()
    );
}

#[test]
fn gateway_static_analysis_warns_and_strict_fails() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = write_gateway_workflow(
        directory.path(),
        r#"
package demo
workflow: {
    name: "analysis"
    steps: {
        left: {kind: "gateway", gateway: {split: "and", branches: {c: true}}}
        right: {kind: "gateway", gateway: {split: "and", branches: {c: true}}}
        c: {kind: "gateway", gateway: {split: "and", branches: {d: true}}}
        d: {kind: "noop"}
    }
}
"#,
    );
    let relaxed = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "validate",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(relaxed.status.success());
    let text = String::from_utf8_lossy(&relaxed.stdout);
    assert!(text.contains("GatewaySharedBranchTarget"), "{text}");
    let strict = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "validate",
        workflow.to_str().expect("workflow"),
        "--strict",
    ]);
    assert_eq!(strict.status.code(), Some(2));
}

#[test]
fn gateway_cycle_with_external_entry_survives_round_one() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = write_gateway_workflow(
        directory.path(),
        r#"
package demo
workflow: {
    name: "external-cycle"
    maxParallel: 1
    steps: {
        x: {kind: "noop"}
        loop: {kind: "noop", needs: ["x", "tail"]}
        split: {kind: "gateway", needs: ["loop"], gateway: {split: "and", branches: {work: true}}}
        work: {kind: "noop"}
        merge: {kind: "gateway", needs: ["work"], gateway: {join: "and"}}
        tail: {kind: "noop", needs: ["merge"], cycle: {max: 2}}
    }
}
"#,
    );
    let run = run_workflow(&db, &workflow);
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let id = run_id(&db);
    let status = status_text(&db, &id);
    assert!(
        status.contains("loop") && status.contains("succeeded"),
        "{status}"
    );
    let connection = rusqlite::Connection::open(&db).expect("open db");
    let (round, visits): (i64, i64) = connection
        .query_row(
            "SELECT round, visits FROM steps WHERE run_id = ?1 AND name = 'loop'",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("loop row");
    assert!(round >= 1, "loop did not run a second round: round={round}");
    assert!(
        visits >= 2,
        "loop did not record two visits: visits={visits}"
    );
    let events = logs_json(&db, &id);
    assert_eq!(
        events
            .iter()
            .filter(|event| event["kind"] == "cycle.exhausted")
            .count(),
        1,
        "{events:?}"
    );
    assert!(
        ost(&["--db", db.to_str().expect("db"), "audit", "verify", &id])
            .status
            .success()
    );
}

#[test]
fn resume_refuses_a_recomputed_digest_that_is_not_anchored() {
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
    step.exec.as_mut().expect("exec").cmd = vec!["sh".into(), "-c".into(), "echo pwned".into()];
    let params: Value = serde_json::from_str(&params_json).expect("params");
    let mut statement = connection
        .prepare(
            "SELECT run_id, name, status, visits, attempts, output_json, error, updated_at, round
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
                round: row.get::<_, i64>(8)? as u32,
            })
        })
        .expect("query steps");
    let steps: Vec<StepRecord> = rows.map(|row| row.expect("step")).collect();
    drop(statement);
    let digest = state_digest(&spec, &params, &source_file, resume_index, &steps);
    let forged_spec = serde_json::to_string(&spec).expect("spec json");
    connection
        .execute(
            "UPDATE runs SET spec_json = ?2, state_hash = ?3 WHERE id = ?1",
            rusqlite::params![run, forged_spec, digest],
        )
        .expect("tamper spec and digest");
    let resumed = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "resume",
        &run,
    ]);
    assert!(!resumed.status.success());
    assert!(
        String::from_utf8_lossy(&resumed.stderr).contains("not anchored"),
        "stderr: {}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    assert!(
        !ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run])
            .status
            .success()
    );
}

#[test]
fn gateway_cycle_round_tampering_fails_audit_verify() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = write_gateway_workflow(
        directory.path(),
        r#"
package demo
params: { go: bool | *false }
workflow: {
    name: "cycle-tamper"
    maxParallel: 1
    steps: {
        loop: {kind: "noop", needs: ["tail"], cycle: {max: 2, until: "round >= 1"}}
        split: {kind: "gateway", needs: ["loop"], gateway: {
            split: "xor"
            branches: {work: "params.go", idle: true}
            default: "idle"
        }}
        work: {kind: "noop"}
        idle: {kind: "noop"}
        merge: {kind: "gateway", needs: ["work", "idle"], gateway: {join: "or"}}
        tail: {kind: "noop", needs: ["merge"]}
    }
}
"#,
    );
    let run = run_workflow(&db, &workflow);
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let id = run_id(&db);
    assert!(
        ost(&["--db", db.to_str().expect("db"), "audit", "verify", &id])
            .status
            .success()
    );
    let connection = rusqlite::Connection::open(&db).expect("open db");
    connection
        .execute(
            "UPDATE steps SET round = 9 WHERE run_id = ?1 AND name = 'split'",
            [&id],
        )
        .expect("tamper round");
    assert!(
        !ost(&["--db", db.to_str().expect("db"), "audit", "verify", &id])
            .status
            .success()
    );
}

#[test]
fn acyclic_runs_do_not_emit_cycle_events() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = write_gateway_workflow(
        directory.path(),
        r#"
package demo
workflow: {
    name: "acyclic"
    steps: {a: {kind: "noop"}}
}
"#,
    );
    let run = run_workflow(&db, &workflow);
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let id = run_id(&db);
    let events = logs_json(&db, &id);
    assert!(
        !events
            .iter()
            .any(|event| event["kind"] == "cycle.exhausted")
    );
    assert!(!events.iter().any(|event| event["kind"] == "cycle.exited"));
}

#[test]
fn gateway_cycle_dead_carry_exits_and_skips_the_tail() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let workflow = write_gateway_workflow(
        directory.path(),
        r#"
package demo
workflow: {
    name: "dead-carry"
    maxParallel: 1
    steps: {
        loop: {kind: "noop", needs: ["tail"], cycle: {max: 3}}
        split: {kind: "gateway", needs: ["loop"], gateway: {
            split: "xor"
            branches: {a: "true", b: true}
            default: "b"
        }}
        a: {kind: "noop"}
        b: {kind: "noop"}
        merge: {kind: "gateway", needs: ["a", "b"], gateway: {join: "and"}}
        tail: {kind: "noop", needs: ["merge"]}
    }
}
"#,
    );
    let run = run_workflow(&db, &workflow);
    assert!(
        run.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let id = run_id(&db);
    let connection = rusqlite::Connection::open(&db).expect("open db");
    let (status, round): (String, i64) = connection
        .query_row(
            "SELECT status, round FROM steps WHERE run_id = ?1 AND name = 'tail'",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("tail row");
    assert_eq!(status, "skipped", "tail must be skipped");
    assert_eq!(round, 0, "tail should be skipped in the first round");
    let events = logs_json(&db, &id);
    assert!(
        events
            .iter()
            .any(|event| event["kind"] == "cycle.exited" && event["data"]["reason"] == "dead_path"),
        "{events:?}"
    );
    assert!(
        ost(&["--db", db.to_str().expect("db"), "audit", "verify", &id])
            .status
            .success()
    );
}

#[test]
fn tag_placement_violations_match_both_evaluators() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let cases = [
        (
            "multi-bytes.cue",
            r#"
package demo
params: {x: {'ab': string | *"v" @tag(ab)}}
workflow: {name: "t", steps: {s: {kind: "noop"}}}
"#,
        ),
        (
            "in-list.cue",
            r#"
package demo
params: {l: [{a: string | *"v" @tag(a)}]}
workflow: {name: "t", steps: {s: {kind: "noop"}}}
"#,
        ),
        (
            "in-listcomp.cue",
            r#"
package demo
params: {l: [for i in [1] {a: string | *"v" @tag(a)}]}
workflow: {name: "t", steps: {s: {kind: "noop"}}}
"#,
        ),
        (
            "struct-for.cue",
            r#"
package demo
params: {for i in [1] {"k": string | *"v" @tag(k)}}
workflow: {name: "t", steps: {s: {kind: "noop"}}}
"#,
        ),
        (
            "pattern.cue",
            r#"
package demo
params: {[string]: {p: string | *"v" @tag(p)}}
workflow: {name: "t", steps: {s: {kind: "noop"}}}
"#,
        ),
        (
            "dynamic-label.cue",
            r#"
package demo
params: {k: "a", "\(k)": {x: string | *"v" @tag(t)}}
workflow: {name: "t", steps: {s: {kind: "noop"}}}
"#,
        ),
        (
            "pattern-value.cue",
            r#"
package demo
params: {x: {[string]: string | *"v" @tag(p)}}
workflow: {name: "t", steps: {s: {kind: "noop"}}}
"#,
        ),
        (
            "same-line-prefix.cue",
            r#"
package demo
params: {@tag(x) a: string | *"v"}
workflow: {name: "t", steps: {s: {kind: "noop"}}}
"#,
        ),
        (
            "combined-hex.cue",
            r#"
package demo
params: {x: {'\xc3\xa9': string | *"v" @tag(e)}}
workflow: {name: "t", steps: {s: {kind: "noop"}}}
"#,
        ),
    ];
    for (name, body) in cases {
        let workflow = directory.path().join(name);
        write(&workflow, body);
        let db_rust = directory.path().join(format!("{name}.rust.sqlite"));
        let rust = ost(&[
            "--db",
            db_rust.to_str().expect("db"),
            "--json",
            "validate",
            workflow.to_str().expect("workflow"),
        ]);
        assert!(
            !rust.status.success(),
            "rust must reject {name}: {}",
            String::from_utf8_lossy(&rust.stdout)
        );
        let db_cue = directory.path().join(format!("{name}.cue.sqlite"));
        let oracle = ost(&[
            "--db",
            db_cue.to_str().expect("db"),
            "--cue",
            &cue,
            "--evaluator",
            "cue",
            "--json",
            "validate",
            workflow.to_str().expect("workflow"),
        ]);
        assert!(
            !oracle.status.success(),
            "cue must reject {name}: {}",
            String::from_utf8_lossy(&oracle.stdout)
        );
    }
}

#[test]
fn tool_commands_match_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let fixtures = [
        (
            "hello",
            r#"
package foo
import "tool/exec"
city: "Amsterdam"
who: *"World" | string @tag(who)
command: hello: {
	print: exec.Run & {
		cmd: "echo Hello \(who)! Welcome to \(city)."
	}
}
"#,
            true,
        ),
        (
            "argv",
            r#"
package foo
import "tool/exec"
command: argv: {
	run: exec.Run & {cmd: ["printf", "%s-%s", "a", "b"]}
}
"#,
            false,
        ),
        (
            "env",
            r#"
package foo
import "tool/exec"
command: env: {
	run: exec.Run & {
		cmd: ["sh", "-c", "printf '%s' \"$FOO\""]
		env: FOO: "bar"
	}
}
"#,
            false,
        ),
        (
            "fail",
            r#"
package foo
import "tool/exec"
command: fail: {
	run: exec.Run & {cmd: ["sh", "-c", "echo bad >&2; exit 3"]}
}
"#,
            false,
        ),
        (
            "after",
            r#"
package foo
import "tool/exec"
command: after: {
	two: exec.Run & {
		$after: one
		cmd: ["sh", "-c", "echo two"]
	}
	one: exec.Run & {cmd: ["sh", "-c", "echo one"]}
}
"#,
            false,
        ),
        (
            "helper",
            r#"
package foo
import "tool/exec"
command: helper: {
	message: "hi there"
	run: exec.Run & {cmd: ["echo", message]}
}
"#,
            false,
        ),
        (
            "strmeta",
            r#"
package foo
import "tool/exec"
command: strmeta: {run: exec.Run & {cmd: "echo $HOME; echo meta >&2"}}
"#,
            false,
        ),
        (
            "envreplace",
            r#"
package foo
import "tool/exec"
command: envreplace: {
	run: exec.Run & {
		cmd: ["sh", "-c", "printf '<%s>' \"$HOME\""]
		env: FOO: "bar"
	}
}
"#,
            false,
        ),
        (
            "envlist",
            r#"
package foo
import "tool/exec"
command: envlist: {
	run: exec.Run & {
		cmd: ["sh", "-c", "printf '%s-%s' \"$FOO\" \"$BAZ\""]
		env: ["FOO=bar", "BAZ=qux"]
	}
}
"#,
            false,
        ),
        (
            "nullcap",
            r#"
package foo
import "tool/exec"
command: nullcap: {
	run: exec.Run & {
		cmd: ["sh", "-c", "echo OUT; echo ERR >&2"]
		stdout: null
		stderr: null
	}
}
"#,
            false,
        ),
        (
            "failstop",
            r#"
package foo
import "tool/exec"
command: failstop: {
	a: exec.Run & {cmd: ["sh", "-c", "exit 3"]}
	b: exec.Run & {$after: a, cmd: ["sh", "-c", "echo B"]}
}
"#,
            false,
        ),
        (
            "mustfalse",
            r#"
package foo
import "tool/exec"
command: mustfalse: {run: exec.Run & {cmd: ["sh", "-c", "exit 7"], mustSucceed: false}}
"#,
            false,
        ),
        (
            "bytesstdin",
            r#"
package foo
import "tool/exec"
command: bytesstdin: {run: exec.Run & {cmd: ["cat"], stdin: 'bytes-in'}}
"#,
            false,
        ),
        (
            "g",
            r#"
package foo
import "tool/exec"
command: f: {run: exec.Run & {cmd: ["echo", "F"]}}
command: g: {run: exec.Run & {cmd: ["echo", "G"]}}
"#,
            false,
        ),
        (
            "helperdef",
            r#"
package foo
import "tool/exec"
command: helperdef: {
	#args: ["echo", "DEF"]
	run: exec.Run & {#note: "helper", cmd: #args}
}
"#,
            false,
        ),
        (
            "defaults",
            r#"
package foo
import "tool/exec"
who: *"World" | string @tag(who)
command: defaults: {run: exec.Run & {cmd: ["echo", who]}}
"#,
            false,
        ),
        (
            "envlistreplace",
            r#"
package foo
import "tool/exec"
command: envlistreplace: {
	run: exec.Run & {
		cmd: ["sh", "-c", "printf '<%s>|<%s>' \"$FOO\" \"$HOME\""]
		env: ["FOO=bar"]
	}
}
"#,
            false,
        ),
        (
            "mustfalsespawn",
            r#"
package foo
import "tool/exec"
command: mustfalsespawn: {run: exec.Run & {cmd: ["definitely-not-a-real-binary-xyz"], mustSucceed: false}}
"#,
            false,
        ),
        (
            "single",
            r#"
package foo
import "tool/exec"
command: single: exec.Run & {cmd: ["echo", "DIRECT"]}
"#,
            false,
        ),
        (
            "grp",
            r#"
package foo
import "tool/exec"
command: grp: {g: {run: exec.Run & {cmd: ["echo", "G"]}}}
"#,
            false,
        ),
        (
            "ref",
            r#"
package foo
import "tool/exec"
base: {run: exec.Run & {cmd: ["echo", "BASE"]}}
command: ref: base
"#,
            false,
        ),
        (
            "envwhole",
            r#"
package foo
import "tool/exec"
command: envwhole: {
	run: exec.Run & {
		cmd: ["sh", "-c", "printf '<%s>' \"$FOO\""]
		env: *{FOO: "bar"} | {[string]: string}
	}
}
"#,
            false,
        ),
        (
            "outdefault",
            r#"
package foo
import "tool/exec"
command: outdefault: {run: exec.Run & {cmd: ["sh", "-c", "echo O"], stdout: *null | string}}
"#,
            false,
        ),
        (
            "mustdefault",
            r#"
package foo
import "tool/exec"
command: mustdefault: {run: exec.Run & {cmd: ["true"], mustSucceed: *true | bool}}
"#,
            false,
        ),
        (
            "stdindefault",
            r#"
package foo
import "tool/exec"
command: stdindefault: {run: exec.Run & {cmd: ["cat"], stdin: *"y" | string}}
"#,
            false,
        ),
        (
            "musttruefalse",
            r#"
package foo
import "tool/exec"
command: musttruefalse: {run: exec.Run & {cmd: ["true"], mustSucceed: *true | false}}
"#,
            false,
        ),
        (
            "stderrdefault",
            r#"
package foo
import "tool/exec"
command: stderrdefault: {run: exec.Run & {cmd: ["sh", "-c", "echo hi"], stderr: *"x" | "y"}}
"#,
            false,
        ),
        (
            "cap",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: cap: {
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc\n'"], stdout: string}
	print: cli.Print & {text: run.stdout}
}
"#,
            false,
        ),
        (
            "printfirst",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: printfirst: {
	print: cli.Print & {text: run.stdout}
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
}
"#,
            false,
        ),
        (
            "envpending",
            r#"
package foo
import "tool/exec"
command: envpending: {
	a: exec.Run & {cmd: ["sh", "-c", "printf '%s' \"$FOO\""], env: {FOO: z.stdout}}
	z: exec.Run & {cmd: ["sh", "-c", "printf bar"], stdout: string}
}
"#,
            false,
        ),
        (
            "stdinpending",
            r#"
package foo
import "tool/exec"
command: stdinpending: {
	a: exec.Run & {cmd: ["cat"], stdin: *z.stdout | string}
	z: exec.Run & {cmd: ["sh", "-c", "printf payload"], stdout: string}
}
"#,
            false,
        ),
        (
            "envlistpending",
            r#"
package foo
import "tool/exec"
command: envlistpending: {
	a: exec.Run & {cmd: ["sh", "-c", "printf '%s' \"$FOO\""], env: [z.stdout]}
	z: exec.Run & {cmd: ["sh", "-c", "printf 'FOO=bar'"], stdout: string}
}
"#,
            false,
        ),
        (
            "cmddisj",
            r#"
package foo
import "tool/exec"
command: cmddisj: {
	a: exec.Run & {$after: z, cmd: [z.stdout | "echo"]}
	z: exec.Run & {cmd: ["sh", "-c", "printf echo"], stdout: string}
}
"#,
            false,
        ),
        (
            "envdef",
            r#"
package foo
import "tool/exec"
#E: {FOO: "bar"}
command: envdef: {run: exec.Run & {cmd: ["sh", "-c", "printf '%s' \"$FOO\""], env: #E}}
"#,
            false,
        ),
        (
            "interp",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: interp: {
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc\n'"], stdout: string}
	p: cli.Print & {text: "got: \(run.stdout)"}
}
"#,
            false,
        ),
        (
            "success",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: success: {
	run: exec.Run & {cmd: ["false"], mustSucceed: false, success: bool}
	p: cli.Print & {text: "success=\(run.success)"}
}
"#,
            false,
        ),
        (
            "cmdinterp",
            r#"
package foo
import "tool/exec"
command: cmdinterp: {
	run: exec.Run & {cmd: ["sh", "-c", "printf 'X'"], stdout: string}
	use: exec.Run & {cmd: ["sh", "-c", "printf 'v=\(run.stdout)'"]}
}
"#,
            false,
        ),
        (
            "interpfirst",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: interpfirst: {
	p: cli.Print & {text: "got: \(run.stdout)"}
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
}
"#,
            false,
        ),
        (
            "defaultstdout",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: defaultstdout: {
	p: cli.Print & {text: "ok=\(run.stdout)"}
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: *"x" | string}
}
"#,
            false,
        ),
        (
            "defaultsuccess",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: defaultsuccess: {
	p: cli.Print & {text: "ok=\(run.success)"}
	run: exec.Run & {cmd: ["true"], success: *false | bool}
}
"#,
            false,
        ),
        (
            "b",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: a: {
	run: exec.Run & {cmd: ["sh", "-c", "printf 'AAA'"], stdout: string}
	p: cli.Print & {text: "\(run.stdout)"}
}
command: b: {
	run: exec.Run & {cmd: ["sh", "-c", "printf 'BBB'"], stdout: string}
	p: cli.Print & {text: "\(run.stdout)"}
}
"#,
            false,
        ),
        (
            "ref",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
base: {
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
	p: cli.Print & {text: "R=\(run.stdout)"}
}
command: ref: base
"#,
            false,
        ),
        (
            "refunify",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
base: {
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
	p: cli.Print & {text: "R=\(run.stdout)"}
}
command: refunify: base & {extra: 1}
"#,
            false,
        ),
        (
            "helper",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: helper: {
	msg: "v=\(run.stdout)"
	p: cli.Print & {text: msg}
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
}
"#,
            false,
        ),
        (
            "helperlet",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: helperlet: {
	let msg = "v=\(run.stdout)"
	p: cli.Print & {text: msg}
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
}
"#,
            false,
        ),
        (
            "paren",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: paren: {
	p: cli.Print & {text: "v=\((run).stdout)"}
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
}
"#,
            false,
        ),
        (
            "helperinterp",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: helperinterp: {
	msg: run.stdout
	p: cli.Print & {text: "x=\(msg)"}
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
}
"#,
            false,
        ),
        (
            "helperletinterp",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: helperletinterp: {
	let msg = run.stdout
	p: cli.Print & {text: "x=\(msg)"}
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
}
"#,
            false,
        ),
        (
            "helperchaininterp",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: helperchaininterp: {
	a: "v=\(run.stdout)"
	b: a
	p: cli.Print & {text: "x=\(b)"}
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
}
"#,
            false,
        ),
        (
            "helpernestedinterp",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: helpernestedinterp: {
	cfg: {msg: run.stdout}
	p: cli.Print & {text: "x=\(cfg.msg)"}
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
}
"#,
            false,
        ),
        (
            "helpersuccessinterp",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: helpersuccessinterp: {
	ok: run.success
	p: cli.Print & {text: "ok=\(ok)"}
	run: exec.Run & {cmd: ["false"], mustSucceed: false, success: bool}
}
"#,
            false,
        ),
        (
            "selbase",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
lib: {
	base: {
		run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
		p: cli.Print & {text: "R=\(run.stdout)"}
	}
}
command: selbase: lib.base
"#,
            false,
        ),
        (
            "selnested",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
lib: {
	inner: {
		base: {
			run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
			p: cli.Print & {text: "R=\(run.stdout)"}
		}
	}
}
command: selnested: lib.inner.base
"#,
            false,
        ),
        (
            "selquoted",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
lib: {
	"b-a": {
		run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
		p: cli.Print & {text: "R=\(run.stdout)"}
	}
}
command: selquoted: lib."b-a"
"#,
            false,
        ),
        (
            "selindex",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
lib: {
	"b-a": {
		run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
		p: cli.Print & {text: "R=\(run.stdout)"}
	}
}
command: selindex: lib["b-a"]
"#,
            false,
        ),
        (
            "selparen",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
lib: {
	base: {
		run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
		p: cli.Print & {text: "R=\(run.stdout)"}
	}
}
command: selparen: (lib).base
"#,
            false,
        ),
        (
            "seldef",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
#Lib: {
	base: {
		run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
		p: cli.Print & {text: "R=\(run.stdout)"}
	}
}
command: seldef: #Lib.base
"#,
            false,
        ),
        (
            "selunify",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
lib: {
	base: {
		run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
		p: cli.Print & {text: "R=\(run.stdout)"}
	}
}
command: selunify: lib.base & {extra: 1}
"#,
            false,
        ),
        (
            "selalias",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
other: {
	inner: {
		run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
		p: cli.Print & {text: "R=\(run.stdout)"}
	}
}
lib: {base: other.inner}
command: selalias: lib.base
"#,
            false,
        ),
        (
            "selmid",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
m: {
	base: {
		run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
		p: cli.Print & {text: "R=\(run.stdout)"}
	}
}
lib: {mid: m}
command: selmid: lib.mid.base
"#,
            false,
        ),
        (
            "selhelper",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
lib: {
	base: {
		msg: run.stdout
		p: cli.Print & {text: "R=\(msg)"}
		run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
	}
}
command: selhelper: lib.base
"#,
            false,
        ),
        (
            "selwhole",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
other: {
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
	p: cli.Print & {text: "R=\(run.stdout)"}
}
lib: other
command: selwhole: lib
"#,
            false,
        ),
        (
            "selmix",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
lib: {
	base: {
		run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
		p: cli.Print & {text: "R=\(run.stdout)"}
	}
}
let group = {inner: {z: exec.Run & {cmd: ["true"]}}}
command: selmix: lib.base & group.inner
"#,
            false,
        ),
        (
            "selmixnested",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
lib: {
	base: {
		producer: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
		run:      cli.Print & {text: "R=\(producer.stdout)"}
	}
}
let group = {inner: {
	z: exec.Run & {cmd: ["true"]}
	meta: {run: 1}
}}
command: selmixnested: lib.base & group.inner
"#,
            false,
        ),
        (
            "taskbase",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
base: {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
command: taskbase: {
	run: exec.Run & base
	p: cli.Print & {text: "R=\(run.stdout)"}
}
"#,
            false,
        ),
        (
            "taskdef",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
#B: {cmd: ["sh", "-c", "printf 'abc'"], stdout: string, ...}
command: taskdef: {
	run: exec.Run & #B
	p: cli.Print & {text: "R=\(run.stdout)"}
}
"#,
            false,
        ),
        (
            "tasktwo",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
b1: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
b2: {extra: 1}
command: tasktwo: {
	run: b1 & b2
	p: cli.Print & {text: "R=\(run.stdout)"}
}
"#,
            false,
        ),
        (
            "taskparen",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
base: {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
command: taskparen: {
	run: (exec.Run & base)
	p: cli.Print & {text: "R=\(run.stdout)"}
}
"#,
            false,
        ),
        (
            "tasksuccess",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
base: {cmd: ["false"], success: bool, mustSucceed: false}
command: tasksuccess: {
	run: exec.Run & base
	p: cli.Print & {text: "ok=\(run.success)"}
}
"#,
            false,
        ),
        (
            "taskafterbase",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: taskafterbase: {
	base: {cmd: ["sh", "-c", "test -f ready && printf 'R' || printf 'X'"], stdout: string, $after: z}
	run:  exec.Run & base
	p:    cli.Print & {text: "P=\(run.stdout)"}
	z:    exec.Run & {cmd: ["sh", "-c", "touch ready"]}
}
"#,
            false,
        ),
        (
            "taskafterdef",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: taskafterdef: {
	#base: {cmd: ["sh", "-c", "test -f ready && printf 'R' || printf 'X'"], stdout: string, $after: z, ...}
	run:   exec.Run & #base
	p:     cli.Print & {text: "P=\(run.stdout)"}
	z:     exec.Run & {cmd: ["sh", "-c", "touch ready"]}
}
"#,
            false,
        ),
        (
            "taskafterlist",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: taskafterlist: {
	base: {cmd: ["sh", "-c", "test -f a && test -f b && printf 'R' || printf 'X'"], stdout: string, $after: [z, m]}
	run:  exec.Run & base
	p:    cli.Print & {text: "P=\(run.stdout)"}
	z:    exec.Run & {cmd: ["sh", "-c", "touch a"]}
	m:    exec.Run & {cmd: ["sh", "-c", "touch b"]}
}
"#,
            false,
        ),
        (
            "taskaftersplit",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: taskaftersplit: {
	base: {cmd: ["sh", "-c", "test -f ready && printf 'R' || printf 'X'"], stdout: string}
	base: {$after: z}
	run:  exec.Run & base
	p:    cli.Print & {text: "P=\(run.stdout)"}
	z:    exec.Run & {cmd: ["sh", "-c", "touch ready"]}
}
"#,
            false,
        ),
        (
            "taskaftersel",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: taskaftersel: {
	bases: {base: {cmd: ["sh", "-c", "test -f ready && printf 'R' || printf 'X'"], stdout: string}}
	bases: {base: {$after: z}}
	run:   exec.Run & bases.base
	p:     cli.Print & {text: "P=\(run.stdout)"}
	z:     exec.Run & {cmd: ["sh", "-c", "touch ready"]}
}
"#,
            false,
        ),
        (
            "taskafterhop",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: taskafterhop: {
	bases: {inner: {base: {cmd: ["sh", "-c", "test -f ready && printf 'R' || printf 'X'"], stdout: string, $after: z}}}
	alias: bases.inner
	run:   exec.Run & alias.base
	p:     cli.Print & {text: "P=\(run.stdout)"}
	z:     exec.Run & {cmd: ["sh", "-c", "touch ready"]}
}
"#,
            false,
        ),
    ];
    for (name, source, injected) in fixtures {
        let directory = tempfile::tempdir().expect("tempdir");
        write(&directory.path().join("fixture_tool.cue"), source);
        let cue_args = if injected {
            vec!["cmd", "-t", "who=Jan", name]
        } else {
            vec!["cmd", name]
        };
        let expected = Command::new(&cue)
            .args(&cue_args)
            .current_dir(directory.path())
            .output()
            .expect("run cue cmd");
        let mut rust_args = vec!["cmd"];
        if injected {
            rust_args.extend(["-p", "who=Jan"]);
        }
        rust_args.push(name);
        rust_args.push(directory.path().to_str().expect("path"));
        let actual = ost(&rust_args);
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{name}: exit codes differ; rust stderr: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "{name}: stdout differs");
        assert_eq!(actual.stderr, expected.stderr, "{name}: stderr differs");
    }
}

#[test]
fn tool_files_stay_excluded_from_normal_workflow_loading() {
    let directory = tempfile::tempdir().expect("tempdir");
    write(
        &directory.path().join("workflow.cue"),
        r#"
package demo
workflow: {name: "plain", steps: {s: {kind: "noop"}}}
"#,
    );
    write(
        &directory.path().join("ignored_tool.cue"),
        r#"
package demo
command: nope: {run: {cmd: ["false"]}}
"#,
    );
    let db = directory.path().join("db.sqlite");
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "validate",
        directory
            .path()
            .join("workflow.cue")
            .to_str()
            .expect("workflow"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json(&output)["workflow"]["name"], "plain");
}

#[test]
fn tool_command_handles_large_stdin_and_capture() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let big = "x".repeat(200_000);
    let source = format!(
        r#"
package foo
import "tool/exec"
command: bigstdin: {{
	run: exec.Run & {{
		cmd: ["wc", "-c"]
		stdin: "{big}"
		stdout: string
	}}
}}
"#
    );
    write(&directory.path().join("big_tool.cue"), &source);
    let expected = Command::new(&cue)
        .args(["cmd", "bigstdin"])
        .current_dir(directory.path())
        .output()
        .expect("run cue cmd");
    let actual = ost(&["cmd", "bigstdin", directory.path().to_str().expect("path")]);
    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}

#[test]
fn tool_command_unknown_tag_matches_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    write(
        &directory.path().join("tag_tool.cue"),
        r#"
package foo
import "tool/exec"
command: tagged: {run: exec.Run & {cmd: ["echo", "x"]}}
"#,
    );
    let expected = Command::new(&cue)
        .args(["cmd", "-t", "nope=1", "tagged"])
        .current_dir(directory.path())
        .output()
        .expect("run cue cmd");
    let actual = ost(&[
        "cmd",
        "-p",
        "nope=1",
        "tagged",
        directory.path().to_str().expect("path"),
    ]);
    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}

#[test]
fn tool_command_resolves_relative_dir_from_package() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let package = directory.path().join("pkg");
    std::fs::create_dir_all(package.join("sub")).expect("subdir");
    std::fs::write(package.join("sub/data.txt"), "relative-ok").expect("data");
    write(
        &package.join("rel_tool.cue"),
        r#"
package foo
import "tool/exec"
command: rel: {run: exec.Run & {cmd: ["cat", "sub/data.txt"], dir: *"sub" | string}}
"#,
    );
    let expected = Command::new(&cue)
        .args(["cmd", "rel"])
        .current_dir(&package)
        .output()
        .expect("run cue cmd");
    let actual = ost(&["cmd", "rel", package.to_str().expect("path")]);
    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}

#[test]
fn tool_command_rejects_conflicting_defaults() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let fixtures = [
        (
            "mustfalse",
            r#"
package foo
import "tool/exec"
command: mustfalse: {run: exec.Run & {cmd: ["sh", "-c", "echo RAN"], mustSucceed: *false | bool}}
"#,
            false,
        ),
        (
            "stdinconflict",
            r#"
package foo
import "tool/exec"
command: stdinconflict: {run: exec.Run & {cmd: ["cat"], stdin: *"y" | null}}
"#,
            false,
        ),
        (
            "stdoutconflict",
            r#"
package foo
import "tool/exec"
command: stdoutconflict: {run: exec.Run & {cmd: ["sh", "-c", "echo O"], stdout: *"x" | null}}
"#,
            false,
        ),
        (
            "baddir",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: baddir: {
	print: cli.Print & {text: "SIBLING"}
	run: exec.Run & {$after: print, cmd: ["echo", "R"], dir: 5}
}
"#,
            false,
        ),
        (
            "baddir-d",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
	"tool/file"
)
command: baddir-d: {
	print: cli.Print & {text: "SIBLING"}
	write: file.Create & {filename: "side.txt", contents: "S"}
	run: exec.Run & {$after: [print, write], cmd: ["sh", "-c", "echo RAN"], dir: 5 | 6}
}
"#,
            true,
        ),
        (
            "badenv",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
	"tool/file"
)
command: badenv: {
	write: file.Create & {filename: "side.txt", contents: "S"}
	run: exec.Run & {$after: write, cmd: ["sh", "-c", "echo RAN"], env: 5 | 6}
}
"#,
            true,
        ),
        (
            "badenvvalue",
            r#"
package foo
import (
	"tool/exec"
	"tool/file"
)
command: badenvvalue: {
	write: file.Create & {filename: "side.txt", contents: "S"}
	run: exec.Run & {$after: write, cmd: ["sh", "-c", "echo RAN"], env: {FOO: 5 | 6}}
}
"#,
            true,
        ),
        (
            "badenvlist",
            r#"
package foo
import (
	"tool/exec"
	"tool/file"
)
command: badenvlist: {
	write: file.Create & {filename: "side.txt", contents: "S"}
	run: exec.Run & {$after: write, cmd: ["sh", "-c", "echo RAN"], env: [5 | 6]}
}
"#,
            true,
        ),
        (
            "badstdin",
            r#"
package foo
import (
	"tool/exec"
	"tool/file"
)
command: badstdin: {
	write: file.Create & {filename: "side.txt", contents: "S"}
	run: exec.Run & {$after: write, cmd: ["cat"], stdin: 5 | 6}
}
"#,
            true,
        ),
        (
            "badmust",
            r#"
package foo
import (
	"tool/exec"
	"tool/file"
)
command: badmust: {
	write: file.Create & {filename: "side.txt", contents: "S"}
	run: exec.Run & {$after: write, cmd: ["true"], mustSucceed: 5 | 6}
}
"#,
            true,
        ),
        (
            "badcmd",
            r#"
package foo
import (
	"tool/exec"
	"tool/file"
)
command: badcmd: {
	write: file.Create & {filename: "side.txt", contents: "S"}
	run: exec.Run & {$after: write, cmd: 5 | 6}
}
"#,
            true,
        ),
        (
            "badcmdlist",
            r#"
package foo
import (
	"tool/exec"
	"tool/file"
)
command: badcmdlist: {
	write: file.Create & {filename: "side.txt", contents: "S"}
	run: exec.Run & {$after: write, cmd: [5 | 6]}
}
"#,
            true,
        ),
        (
            "badprint",
            r#"
package foo
import (
	"tool/cli"
	"tool/file"
)
command: badprint: {
	write: file.Create & {filename: "side.txt", contents: "S"}
	p: cli.Print & {$after: write, text: 5 | 6}
}
"#,
            true,
        ),
        (
            "nontaskinterp",
            r#"
package foo
import (
	"tool/cli"
	"tool/file"
)
command: nontaskinterp: {
	write: file.Create & {filename: "side.txt", contents: "S"}
	x: string
	p: cli.Print & {$after: write, text: "v=\(x)" & "v=abc"}
}
"#,
            false,
        ),
    ];
    for (name, source, file_task) in fixtures {
        let directory = tempfile::tempdir().expect("tempdir");
        write(&directory.path().join("fixture_tool.cue"), source);
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(directory.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, directory.path().to_str().expect("path")]);
        assert_eq!(actual.status.code(), expected.status.code(), "{name}");
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        if file_task {
            assert!(
                !directory.path().join("side.txt").exists(),
                "{name}: sibling file task ran"
            );
        }
    }
}

#[test]
fn tool_command_spawn_error_messages_match_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let cases = [
        (
            "missdir",
            r#"
package foo
import "tool/exec"
command: missdir: {run: exec.Run & {cmd: ["echo", "X"], dir: "nope"}}
"#,
        ),
        (
            "missboth",
            r#"
package foo
import "tool/exec"
command: missboth: {run: exec.Run & {cmd: ["no-such-bin-xyz"], dir: "nope"}}
"#,
        ),
        (
            "dirfile",
            r#"
package foo
import "tool/exec"
command: dirfile: {run: exec.Run & {cmd: ["echo", "X"], dir: "file.txt"}}
"#,
        ),
        (
            "absbin",
            r#"
package foo
import "tool/exec"
command: absbin: {run: exec.Run & {cmd: ["/no/such/bin-xyz"]}}
"#,
        ),
        (
            "relbin",
            r#"
package foo
import "tool/exec"
command: relbin: {run: exec.Run & {cmd: ["./prog"], dir: "sub"}}
"#,
        ),
    ];
    for (name, source) in cases {
        let directory = tempfile::tempdir().expect("tempdir");
        if name == "dirfile" {
            std::fs::write(directory.path().join("file.txt"), "x").expect("file");
        }
        if name == "relbin" {
            std::fs::create_dir_all(directory.path().join("sub")).expect("dir");
        }
        write(&directory.path().join("fixture_tool.cue"), source);
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(directory.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, directory.path().to_str().expect("path")]);
        assert_eq!(actual.status.code(), expected.status.code(), "{name}");
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        assert_eq!(actual.stderr, expected.stderr, "{name}");
    }
}

#[test]
fn tool_command_print_and_file_tasks_match_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let cases = [
        (
            "append",
            r#"
package foo
import (
	"tool/exec"
	"tool/file"
)
command: append: {
	run: exec.Run & {cmd: ["sh", "-c", "printf 'line1\\n'"], stdout: string}
	write: file.Append & {$after: run, filename: "out.txt", contents: run.stdout}
}
"#,
            Some("out.txt"),
        ),
        (
            "create",
            r#"
package foo
import "tool/file"
command: create: {write: file.Create & {filename: "created.txt", contents: "c"}}
"#,
            Some("created.txt"),
        ),
        (
            "missing",
            r#"
package foo
import "tool/file"
command: missing: {write: file.Append & {filename: "sub/out.txt", contents: "x"}}
"#,
            None,
        ),
        (
            "interpfile",
            r#"
package foo
import (
	"tool/exec"
	"tool/file"
)
command: interpfile: {
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc\n'"], stdout: string}
	w: file.Create & {filename: "out.txt", contents: "pre-\(run.stdout)post"}
}
"#,
            Some("out.txt"),
        ),
        (
            "defaultcapfile",
            r#"
package foo
import (
	"tool/exec"
	"tool/file"
)
command: defaultcapfile: {
	w: file.Create & {filename: "out.txt", contents: "pre-\(run.stdout)post"}
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: *"x" | string}
}
"#,
            Some("out.txt"),
        ),
    ];
    for (name, source, artifact) in cases {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        write(&cue_dir.path().join("fixture_tool.cue"), source);
        write(&rust_dir.path().join("fixture_tool.cue"), source);
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        assert_eq!(actual.status.code(), expected.status.code(), "{name}");
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        assert_eq!(actual.stderr, expected.stderr, "{name}");
        if let Some(artifact) = artifact {
            assert_eq!(
                std::fs::read(cue_dir.path().join(artifact)).expect("cue artifact"),
                std::fs::read(rust_dir.path().join(artifact)).expect("rust artifact"),
                "{name}: file contents differ"
            );
        }
    }
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod");
    }
}

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn agent_definitions_produce_verifiable_traces_and_artifacts() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let agent = directory.path().join("agent.sh");
    write(
        &agent,
        "#!/bin/sh\ncat > prompt.txt\nmkdir -p out\nprintf 'report body\\n' > out/report.txt\nprintf '{\"score\":0.91,\"summary\":\"ok\"}\\n'\n",
    );
    make_executable(&agent);
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        &format!(
            r#"
package demo
agents: {{
    reviewer: {{
        provider: "claude"
        command: ["{agent}"]
        model: "fake-model"
        system: "review strictly"
        artifacts: ["out/*.txt"]
    }}
}}
workflow: {{
    name: "agents"
    steps: {{
        review: {{
            kind: "agent"
            agent: {{use: "reviewer", prompt: "Review the change"}}
        }}
    }}
}}
"#,
            agent = agent.display()
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

    let validated = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "--json",
        "validate",
        workflow.to_str().expect("workflow"),
    ]);
    let validated = json(&validated);
    assert_eq!(
        validated["workflow"]["steps"]["review"]["agent"]["definition"],
        "reviewer"
    );
    assert_eq!(
        validated["workflow"]["steps"]["review"]["agent"]["provider"],
        "claude"
    );
    assert_eq!(
        validated["workflow"]["steps"]["review"]["agent"]["model"],
        "fake-model"
    );

    let traces = ost(&["--db", db.to_str().expect("db"), "--json", "agents", &run]);
    assert!(traces.status.success());
    let traces = json(&traces);
    let entries = traces["traces"].as_array().expect("traces");
    assert_eq!(entries.len(), 1, "{traces:?}");
    let trace = &entries[0];
    assert_eq!(trace["trace"]["step"], "review");
    assert_eq!(trace["trace"]["definition"], "reviewer");
    assert_eq!(trace["trace"]["provider"], "claude");
    assert_eq!(trace["trace"]["model"], "fake-model");
    assert_eq!(trace["trace"]["success"], true);
    assert_eq!(trace["trace"]["argv"][0], agent.to_str().expect("agent"));
    assert_eq!(trace["trace"]["artifacts"][0]["name"], "out/report.txt");
    assert_eq!(
        trace["trace"]["stdout"],
        "{\"score\":0.91,\"summary\":\"ok\"}\n"
    );

    let verified = ost(&["--db", db.to_str().expect("db"), "agents", &run, "--verify"]);
    assert!(
        verified.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&verified.stderr)
    );
    assert!(String::from_utf8_lossy(&verified.stdout).contains("OK trace="));

    let artifacts = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "artifacts",
        &run,
    ]);
    let artifacts = json(&artifacts);
    let items = artifacts["artifacts"].as_array().expect("artifacts");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], "out/report.txt");
    assert_eq!(items[0]["kind"], "declared");

    let extract = directory.path().join("extract");
    let extracted = ost(&[
        "--db",
        db.to_str().expect("db"),
        "artifacts",
        &run,
        "--extract",
        extract.to_str().expect("extract"),
    ]);
    assert!(
        extracted.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&extracted.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(extract.join("out/report.txt")).expect("artifact"),
        "report body\n"
    );

    let audit = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(
        audit.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&audit.stderr)
    );
}

#[test]
fn agent_retries_keep_one_trace_per_attempt() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let counter = directory.path().join("counter");
    let agent = directory.path().join("agent.sh");
    write(
        &agent,
        &format!(
            "#!/bin/sh\ncat >/dev/null\nn=$(cat {counter} 2>/dev/null || echo 0)\nn=$((n+1))\necho $n > {counter}\nif [ $n -lt 2 ]; then echo first failed >&2; exit 7; fi\necho '{{\"score\":0.5}}'\n",
            counter = counter.display()
        ),
    );
    make_executable(&agent);
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        &format!(
            r#"
package demo
agents: {{writer: {{command: ["{agent}"]}}}}
workflow: {{
    name: "retry"
    steps: {{
        work: {{
            kind: "agent"
            retry: {{max: 2, baseMs: 1, factor: 1.0, jitter: false}}
            agent: {{use: "writer", prompt: "try"}}
        }}
    }}
}}
"#,
            agent = agent.display()
        ),
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = run_id(&db);
    let traces = ost(&["--db", db.to_str().expect("db"), "--json", "agents", &run]);
    let traces = json(&traces);
    let entries = traces["traces"].as_array().expect("traces");
    assert_eq!(entries.len(), 2, "{traces:?}");
    assert_eq!(entries[0]["trace"]["attempt"], 0);
    assert_eq!(entries[0]["trace"]["success"], false);
    assert!(
        entries[0]["trace"]["error"]
            .as_str()
            .expect("error")
            .contains("exited with 7")
    );
    assert_eq!(entries[1]["trace"]["attempt"], 1);
    assert_eq!(entries[1]["trace"]["success"], true);
    let verified = ost(&["--db", db.to_str().expect("db"), "agents", &run, "--verify"]);
    assert!(
        verified.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&verified.stderr)
    );
}

#[test]
fn agent_trace_tampering_is_detected_by_audit_verify() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let agent = directory.path().join("agent.sh");
    write(&agent, "#!/bin/sh\ncat >/dev/null\necho '{\"score\":1}'\n");
    make_executable(&agent);
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        &format!(
            r#"
package demo
agents: {{a: {{command: ["{agent}"]}}}}
workflow: {{name: "tamper", steps: {{s: {{kind: "agent", agent: {{use: "a", prompt: "x"}}}}}}}}
"#,
            agent = agent.display()
        ),
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
        .execute(
            "UPDATE agent_traces SET trace_json = '{\"tampered\":true}' WHERE run_id = ?1",
            [&run],
        )
        .expect("tamper");
    let verified = ost(&["--db", db.to_str().expect("db"), "agents", &run, "--verify"]);
    assert!(!verified.status.success());
    assert!(
        String::from_utf8_lossy(&verified.stderr).contains("TRACE FAILED"),
        "stderr: {}",
        String::from_utf8_lossy(&verified.stderr)
    );
    let audit = ost(&["--db", db.to_str().expect("db"), "audit", "verify", &run]);
    assert!(!audit.status.success());
    assert!(
        String::from_utf8_lossy(&audit.stderr).contains("AUDIT FAILED"),
        "stderr: {}",
        String::from_utf8_lossy(&audit.stderr)
    );
}

#[test]
fn worktree_isolation_keeps_the_checkout_clean_and_captures_changes() {
    if !git_available() {
        eprintln!("skipping: git not installed");
        return;
    }
    let directory = tempfile::tempdir().expect("tempdir");
    let repo = directory.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo");
    let run_git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?} failed");
    };
    run_git(&["init", "-q"]);
    write(&repo.join("tracked.txt"), "base\n");
    let agent = repo.join("agent.sh");
    write(
        &agent,
        "#!/bin/sh\ncat >/dev/null\necho agent >> tracked.txt\necho new > created.txt\nmkdir -p reports\necho report > reports/review.md\necho '{\"score\":0.8}'\n",
    );
    make_executable(&agent);
    let workflow = repo.join("workflow.cue");
    write(
        &workflow,
        r#"
package demo
agents: {
    worker: {
        command: ["./agent.sh"]
        isolation: {mode: "worktree", keep: false}
        artifacts: ["reports/*.md"]
    }
}
workflow: {
    name: "worktree"
    steps: {
        work: {kind: "agent", agent: {use: "worker", prompt: "edit"}}
    }
}
"#,
    );
    run_git(&["add", "."]);
    run_git(&["commit", "-qm", "init"]);
    let db = directory.path().join("db.sqlite");
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("tracked.txt")).expect("tracked"),
        "base\n"
    );
    assert!(!repo.join("created.txt").exists());
    let run = run_id(&db);
    let traces = ost(&["--db", db.to_str().expect("db"), "--json", "agents", &run]);
    let traces = json(&traces);
    let trace = &traces["traces"][0]["trace"];
    assert_eq!(trace["isolation"], "worktree");
    assert!(trace["isolation_path"].is_string());
    assert!(trace["isolation_base"].is_string());
    let artifacts = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "artifacts",
        &run,
    ]);
    let artifacts = json(&artifacts);
    let names: Vec<&str> = artifacts["artifacts"]
        .as_array()
        .expect("artifacts")
        .iter()
        .map(|item| item["name"].as_str().expect("name"))
        .collect();
    assert!(names.contains(&"_isolation/changes.patch"), "{names:?}");
    assert!(names.contains(&"created.txt"), "{names:?}");
    assert!(names.contains(&"tracked.txt"), "{names:?}");
    assert!(names.contains(&"reports/review.md"), "{names:?}");
    let extract = directory.path().join("extract");
    let extracted = ost(&[
        "--db",
        db.to_str().expect("db"),
        "artifacts",
        &run,
        "--extract",
        extract.to_str().expect("extract"),
    ]);
    assert!(extracted.status.success());
    let patch = std::fs::read_to_string(extract.join("_isolation/changes.patch")).expect("patch");
    assert!(patch.contains("+agent"), "{patch}");
    assert!(patch.contains("created.txt"), "{patch}");
}

#[test]
fn official_cue_evaluator_resolves_agent_definitions() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let agent = directory.path().join("agent.sh");
    write(
        &agent,
        "#!/bin/sh\ncat >/dev/null\necho '{\"score\":0.7}'\n",
    );
    make_executable(&agent);
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        &format!(
            r#"
package demo
agents: {{reviewer: {{provider: "codex", command: ["{agent}"], model: "m"}}}}
workflow: {{
    name: "cue-agents"
    steps: {{review: {{kind: "agent", agent: {{use: "reviewer", prompt: "hello"}}}}}}
}}
"#,
            agent = agent.display()
        ),
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--cue",
        &cue,
        "--evaluator",
        "cue",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = run_id(&db);
    let traces = ost(&["--db", db.to_str().expect("db"), "--json", "agents", &run]);
    let traces = json(&traces);
    assert_eq!(traces["traces"][0]["trace"]["definition"], "reviewer");
    assert_eq!(traces["traces"][0]["trace"]["provider"], "codex");
}

#[test]
fn resume_replays_a_group_without_trace_key_collisions() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let flag = directory.path().join("go");
    let agent = directory.path().join("agent.sh");
    write(&agent, "#!/bin/sh\ncat >/dev/null\necho '{\"score\":1}'\n");
    make_executable(&agent);
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        &format!(
            r#"
package demo
agents: {{a: {{command: ["{agent}"]}}}}
workflow: {{
    name: "resume-traces"
    steps: {{
        a: {{kind: "agent", agent: {{use: "a", prompt: "x"}}}}
        b: {{kind: "exec", exec: {{cmd: ["sh", "-c", "test -f {flag} || exit 7"]}}}}
    }}
}}
"#,
            agent = agent.display(),
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
    assert!(!first.status.success());
    let run = run_id(&db);
    let before = ost(&["--db", db.to_str().expect("db"), "--json", "agents", &run]);
    assert_eq!(json(&before)["traces"].as_array().expect("traces").len(), 1);
    std::fs::write(&flag, "go").expect("flag");
    let resumed = ost(&["--db", db.to_str().expect("db"), "resume", &run]);
    assert!(
        resumed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let after = ost(&["--db", db.to_str().expect("db"), "--json", "agents", &run]);
    let traces = json(&after);
    let traces = traces["traces"].as_array().expect("traces");
    assert_eq!(traces.len(), 2, "{traces:?}");
    let verified = ost(&["--db", db.to_str().expect("db"), "agents", &run, "--verify"]);
    assert!(
        verified.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&verified.stderr)
    );
}

#[test]
fn cycle_rounds_each_get_a_trace() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let counter = directory.path().join("counter");
    let agent = directory.path().join("agent.sh");
    write(
        &agent,
        &format!(
            "#!/bin/sh\ncat >/dev/null\nn=$(cat {counter} 2>/dev/null || echo 0)\nn=$((n+1))\necho $n > {counter}\ncase $n in 1) s=0.1;; 2) s=0.5;; *) s=0.95;; esac\necho \"{{\\\"score\\\":$s}}\"\n",
            counter = counter.display()
        ),
    );
    make_executable(&agent);
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        &format!(
            r#"
package demo
agents: {{a: {{command: ["{agent}"]}}}}
workflow: {{
    name: "cycle-traces"
    steps: {{
        review: {{
            kind: "agent"
            needs: ["review"]
            cycle: {{max: 3, until: "steps.review.score >= 0.9"}}
            agent: {{use: "a", prompt: "x"}}
        }}
    }}
}}
"#,
            agent = agent.display()
        ),
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = run_id(&db);
    let traces = ost(&["--db", db.to_str().expect("db"), "--json", "agents", &run]);
    let traces = json(&traces);
    let traces = traces["traces"].as_array().expect("traces");
    assert_eq!(traces.len(), 3, "{traces:?}");
    let rounds: Vec<u64> = traces
        .iter()
        .map(|item| item["trace"]["round"].as_u64().expect("round"))
        .collect();
    assert_eq!(rounds, vec![0, 1, 2]);
    let verified = ost(&["--db", db.to_str().expect("db"), "agents", &run, "--verify"]);
    assert!(verified.status.success());
}

#[test]
fn jsonl_provider_output_drives_until_conditions() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let agent = directory.path().join("agent.sh");
    write(
        &agent,
        "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{\"type\":\"assistant\",\"text\":\"working\"}' '{\"type\":\"result\",\"score\":0.95}'\n",
    );
    make_executable(&agent);
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        &format!(
            r#"
package demo
agents: {{a: {{provider: "codex", command: ["{agent}"], promptDelivery: "stdin"}}}}
workflow: {{
    name: "jsonl"
    steps: {{
        review: {{
            kind: "agent"
            needs: ["review"]
            cycle: {{max: 3, until: "steps.review.score >= 0.9"}}
            agent: {{use: "a", prompt: "x"}}
        }}
    }}
}}
"#,
            agent = agent.display()
        ),
    );
    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--evaluator",
        "native",
        "run",
        workflow.to_str().expect("workflow"),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = run_id(&db);
    let traces = ost(&["--db", db.to_str().expect("db"), "--json", "agents", &run]);
    let traces = json(&traces);
    let traces = traces["traces"].as_array().expect("traces");
    assert_eq!(
        traces.len(),
        1,
        "score from JSONL must stop the cycle: {traces:?}"
    );
    assert_eq!(traces[0]["trace"]["round"], 0);
    assert!(traces[0]["trace"]["transcript"].is_array());
}

#[test]
fn agents_and_artifacts_error_on_unknown_runs() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let agents = ost(&["--db", db.to_str().expect("db"), "agents", "nope"]);
    assert!(!agents.status.success());
    assert!(String::from_utf8_lossy(&agents.stderr).contains("run not found"));
    let artifacts = ost(&["--db", db.to_str().expect("db"), "artifacts", "nope"]);
    assert!(!artifacts.status.success());
    assert!(String::from_utf8_lossy(&artifacts.stderr).contains("run not found"));
}

#[test]
fn resume_extraction_namespaces_replayed_traces_by_id() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let flag = directory.path().join("go");
    let agent = directory.path().join("agent.sh");
    write(
        &agent,
        "#!/bin/sh\ncat >/dev/null\nprintf 'report\\n' > report.txt\necho '{\"score\":1}'\n",
    );
    make_executable(&agent);
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        &format!(
            r#"
package demo
agents: {{a: {{command: ["{agent}"], artifacts: ["report.txt"]}}}}
workflow: {{
    name: "resume-artifacts"
    steps: {{
        a: {{kind: "agent", agent: {{use: "a", prompt: "x"}}}}
        b: {{kind: "exec", exec: {{cmd: ["sh", "-c", "test -f {flag} || exit 7"]}}}}
    }}
}}
"#,
            agent = agent.display(),
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
    assert!(!first.status.success());
    let run = run_id(&db);
    std::fs::write(&flag, "go").expect("flag");
    let resumed = ost(&["--db", db.to_str().expect("db"), "resume", &run]);
    assert!(
        resumed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let extract = directory.path().join("out");
    let extracted = ost(&[
        "--db",
        db.to_str().expect("db"),
        "artifacts",
        &run,
        "--extract",
        extract.to_str().expect("extract"),
    ]);
    assert!(
        extracted.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&extracted.stderr)
    );
    let namespaces: Vec<String> = std::fs::read_dir(extract.join("a"))
        .expect("namespaces")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(namespaces.len(), 2, "{namespaces:?}");
    assert!(
        namespaces.iter().all(|name| name.contains("-t")),
        "{namespaces:?}"
    );
}

#[test]
fn deleting_a_trace_row_is_detected_from_step_outputs() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let agent = directory.path().join("agent.sh");
    write(&agent, "#!/bin/sh\ncat >/dev/null\necho '{\"score\":1}'\n");
    make_executable(&agent);
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        &format!(
            r#"
package demo
agents: {{a: {{command: ["{agent}"]}}}}
workflow: {{name: "del", steps: {{s: {{kind: "agent", agent: {{use: "a", prompt: "x"}}}}}}}}
"#,
            agent = agent.display()
        ),
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
    let connection = rusqlite::Connection::open(&db).expect("db");
    connection
        .execute("DELETE FROM agent_traces WHERE run_id = ?1", [&run])
        .expect("delete");
    let verified = ost(&["--db", db.to_str().expect("db"), "agents", &run, "--verify"]);
    assert!(!verified.status.success());
    assert!(
        String::from_utf8_lossy(&verified.stderr).contains("references missing trace"),
        "stderr: {}",
        String::from_utf8_lossy(&verified.stderr)
    );
}

#[test]
fn filtered_agents_verify_still_checks_the_whole_run() {
    let directory = tempfile::tempdir().expect("tempdir");
    let db = directory.path().join("db.sqlite");
    let agent = directory.path().join("agent.sh");
    write(&agent, "#!/bin/sh\ncat >/dev/null\necho '{\"score\":1}'\n");
    make_executable(&agent);
    let workflow = directory.path().join("workflow.cue");
    write(
        &workflow,
        &format!(
            r#"
package demo
agents: {{a: {{command: ["{agent}"]}}}}
workflow: {{
    name: "multi"
    steps: {{
        a: {{kind: "agent", agent: {{use: "a", prompt: "a"}}}}
        b: {{kind: "agent", agent: {{use: "a", prompt: "b"}}}}
    }}
}}
"#,
            agent = agent.display()
        ),
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
    let connection = rusqlite::Connection::open(&db).expect("db");
    connection
        .execute(
            "UPDATE agent_traces SET trace_json = '{}' WHERE run_id = ?1 AND step = 'a'",
            [&run],
        )
        .expect("tamper");
    let verified = ost(&[
        "--db",
        db.to_str().expect("db"),
        "agents",
        &run,
        "--step",
        "b",
        "--verify",
    ]);
    assert!(!verified.status.success());
    assert!(
        String::from_utf8_lossy(&verified.stderr).contains("TRACE FAILED"),
        "stderr: {}",
        String::from_utf8_lossy(&verified.stderr)
    );
}

#[test]
fn tool_command_referenced_and_unified_commands_match_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let cases = [
        (
            "unifyafter",
            r#"
package foo
import "tool/exec"
base: {
	a: exec.Run & {$after: z, cmd: ["sh", "-c", "cat data.txt"]}
	z: exec.Run & {cmd: ["sh", "-c", "printf fresh > data.txt"]}
}
command: unifyafter: base & {extra: 1}
"#,
            Some(("data.txt", "stale")),
            None,
        ),
        (
            "letbaseafter",
            r#"
package foo
import "tool/exec"
let base = {
	a: exec.Run & {$after: z, cmd: ["sh", "-c", "cat data.txt"]}
	z: exec.Run & {cmd: ["sh", "-c", "printf fresh > data.txt"]}
}
command: letbaseafter: base
"#,
            Some(("data.txt", "stale")),
            None,
        ),
        (
            "twobases",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
	"tool/file"
)
base1: {
	run1: exec.Run & {cmd: ["sh", "-c", "printf 'AAA'"], stdout: string}
	p1: cli.Print & {text: "1=\(run1.stdout)"}
}
base2: {
	run2: exec.Run & {cmd: ["sh", "-c", "printf 'BBB'"], stdout: string}
	w2: file.Create & {filename: "out2.txt", contents: "2=\(run2.stdout)"}
}
command: twobases: base1 & base2
"#,
            None,
            Some("out2.txt"),
        ),
        (
            "chainedunify",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
first: {
	run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'"], stdout: string}
	p: cli.Print & {text: "R=\(run.stdout)"}
}
second: first & {extra: 2}
command: chainedunify: second
"#,
            None,
            None,
        ),
    ];
    for (name, source, seed, artifact) in cases {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        write(&cue_dir.path().join("fixture_tool.cue"), source);
        write(&rust_dir.path().join("fixture_tool.cue"), source);
        if let Some((file, contents)) = seed {
            write(&cue_dir.path().join(file), contents);
            write(&rust_dir.path().join(file), contents);
        }
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        assert_eq!(actual.status.code(), expected.status.code(), "{name}");
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        assert_eq!(actual.stderr, expected.stderr, "{name}");
        if let Some(artifact) = artifact {
            assert_eq!(
                std::fs::read(cue_dir.path().join(artifact)).expect("cue artifact"),
                std::fs::read(rust_dir.path().join(artifact)).expect("rust artifact"),
                "{name}: file contents differ"
            );
        }
    }
}

#[test]
fn tool_command_nested_task_outputs_match_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let cases = [
        (
            "nestedoutput",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: nestedoutput: {
	a: producer: exec.Run & {cmd: ["sh", "-c", "printf abc"], stdout: string}
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}
"#,
        ),
        (
            "nestedoutputorder",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: nestedoutputorder: {
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
	a: producer: exec.Run & {cmd: ["sh", "-c", "printf abc"], stdout: string}
}
"#,
        ),
        (
            "nestedoutputdeep",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: nestedoutputdeep: {
	a: b: producer: exec.Run & {cmd: ["sh", "-c", "printf abc"], stdout: string}
	q: cli.Print & {text: "R=\(a.b.producer.stdout)"}
}
"#,
        ),
        (
            "nestedoutputsuccess",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: nestedoutputsuccess: {
	a: producer: exec.Run & {cmd: ["false"], success: bool, mustSucceed: false}
	q: cli.Print & {text: "ok=\(a.producer.success)"}
}
"#,
        ),
        (
            "nestedoutputtwo",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: nestedoutputtwo: {
	a: p1: exec.Run & {cmd: ["sh", "-c", "printf A"], stdout: string}
	b: p2: exec.Run & {cmd: ["sh", "-c", "printf B"], stdout: string}
	q: cli.Print & {text: "R=\(a.p1.stdout)-\(b.p2.stdout)"}
}
"#,
        ),
        (
            "nestedoutputhelper",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: nestedoutputhelper: {
	a: {
		producer: exec.Run & {cmd: ["sh", "-c", "printf abc"], stdout: string}
		msg: producer.stdout
	}
	q: cli.Print & {text: "R=\(a.msg)"}
}
"#,
        ),
        (
            "nestedoutputconcrete",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: nestedoutputconcrete: {
	a: producer: {stdout: "concrete"}
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
	z: t: exec.Run & {cmd: ["true"]}
}
"#,
        ),
    ];
    for (name, source) in cases {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        write(&cue_dir.path().join("fixture_tool.cue"), source);
        write(&rust_dir.path().join("fixture_tool.cue"), source);
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{name}: cue stderr: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        assert_eq!(actual.stderr, expected.stderr, "{name}");
    }
}

#[test]
fn tool_command_nested_output_ignores_import_concrete_values() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let source = r#"
package foo

import (
	"tool/exec"
	"example.com/k/lib"
)

command: c: {
	q: exec.Run & {
		cmd: ["sh", "-c", "printf Q\(a.producer.stdout) >> order.txt"]
		stdout: string
	}
	a: producer: lib.val
	z: producer: exec.Run & {
		cmd: ["sh", "-c", "printf Z\(q.stdout) >> order.txt"]
		stdout: string
	}
}
"#;
    let module = "module: \"example.com/k\"\nlanguage: version: \"v0.17.1\"\n";
    let library = "package lib\nval: {stdout: \"concrete\"}\n";
    let cue_dir = tempfile::tempdir().expect("tempdir");
    let rust_dir = tempfile::tempdir().expect("tempdir");
    for directory in [cue_dir.path(), rust_dir.path()] {
        std::fs::create_dir_all(directory.join("cue.mod")).expect("cue.mod");
        std::fs::create_dir_all(directory.join("lib")).expect("lib");
        write(&directory.join("cue.mod/module.cue"), module);
        write(&directory.join("lib/lib.cue"), library);
        write(&directory.join("x_tool.cue"), source);
    }
    let expected = Command::new(&cue)
        .args(["cmd", "c"])
        .current_dir(cue_dir.path())
        .output()
        .expect("run cue cmd");
    let actual = ost(&["cmd", "c", rust_dir.path().to_str().expect("path")]);
    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
    assert_eq!(
        std::fs::read(cue_dir.path().join("order.txt")).expect("cue order"),
        std::fs::read(rust_dir.path().join("order.txt")).expect("rust order")
    );
}

#[test]
fn tool_command_relative_paths_use_invocation_cwd() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let source = r#"
package foo
import (
	"tool/cli"
	"tool/exec"
	"tool/file"
)
command: c: {
	where: exec.Run & {cmd: ["sh", "-c", "pwd"], stdout: string}
	r: exec.Run & {cmd: ["sh", "-c", "pwd; ls"], dir: "work", stdout: string}
	q: cli.Print & {text: "D=\(where.stdout)R=\(r.stdout)"}
	w: file.Create & {filename: "out.txt", contents: "hello"}
}
"#;
    let root = tempfile::tempdir().expect("tempdir");
    let pkg = root.path().join("pkg");
    std::fs::create_dir_all(pkg.join("work")).expect("pkg work");
    std::fs::create_dir_all(root.path().join("work")).expect("root work");
    write(&pkg.join("a_tool.cue"), source);
    write(&pkg.join("work/PKG"), "");
    write(&root.path().join("work/ROOT"), "");
    let expected = Command::new(&cue)
        .args(["cmd", "c", "./pkg"])
        .current_dir(root.path())
        .output()
        .expect("run cue cmd");
    let actual = Command::new(bin())
        .args(["cmd", "c", "./pkg"])
        .current_dir(root.path())
        .output()
        .expect("run ostinato cmd");
    assert_eq!(actual.status.code(), expected.status.code());
    let normalize = |output: &[u8]| {
        String::from_utf8_lossy(output)
            .replace(&root.path().display().to_string(), "<root>")
            .replace(&pkg.display().to_string(), "<pkg>")
    };
    assert_eq!(normalize(&actual.stdout), normalize(&expected.stdout));
    assert!(
        root.path().join("out.txt").exists(),
        "file task must write relative to the invocation cwd"
    );
    assert!(
        !pkg.join("out.txt").exists(),
        "file task must not write relative to the package dir"
    );
}

#[test]
fn tool_command_package_after_schedule_matches_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let cases: &[(&str, &str, Option<&str>)] = &[
        (
            "pkgafter",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf prep > order.txt"]}
command: pkgafter: {
	a: exec.Run & {$after: prep, cmd: ["sh", "-c", "cat order.txt"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgafterchain",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
one: exec.Run & {cmd: ["sh", "-c", "printf 1 >> order.txt"]}
two: exec.Run & {$after: one, cmd: ["sh", "-c", "printf 2 >> order.txt"]}
command: pkgafterchain: {
	a: exec.Run & {$after: two, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgaftercontainer",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
group: {
	prep: exec.Run & {cmd: ["sh", "-c", "printf prep > order.txt"]}
}
command: pkgaftercontainer: {
	a: exec.Run & {$after: group.prep, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgafterdefinition",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
#prep: exec.Run & {cmd: ["sh", "-c", "printf D >> order.txt"]}
command: pkgafterdefinition: {
	a: exec.Run & {$after: #prep, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgafterread",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf prep"], stdout: string}
command: pkgafterread: {
	a: exec.Run & {$after: prep, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)-\(prep.stdout)"}
}
"#,
            None,
        ),
        (
            "pkgrootread",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P"], stdout: string}
command: pkgrootread: {
	q: cli.Print & {text: "R=\(prep.stdout)"}
}
"#,
            None,
        ),
        (
            "pkgrootnestedread",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
root: p1: exec.Run & {cmd: ["sh", "-c", "printf P"], stdout: string}
command: pkgrootnestedread: {
	q: cli.Print & {text: "R=\(root.p1.stdout)"}
}
"#,
            None,
        ),
        (
            "pkgafterlet",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let p = prep
prep: exec.Run & {cmd: ["sh", "-c", "printf prep > order.txt"]}
command: pkgafterlet: {
	a: exec.Run & {$after: p, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgafteralias",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf prep > order.txt"]}
alias: prep
command: pkgafteralias: {
	a: exec.Run & {$after: alias, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgunreferenced",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf unused > unused.txt"]}
command: pkgunreferenced: {
	a: exec.Run & {cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "pkgshadow",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf TOP >> order.txt"]}
command: pkgshadow: {
	prep: exec.Run & {cmd: ["sh", "-c", "printf CMD >> order.txt"], stdout: string}
	a: exec.Run & {$after: prep, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "R=\(prep.stdout)"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgduplicate",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}
command: pkgduplicate: {
	b: prep
	a: exec.Run & {$after: prep, cmd: ["sh", "-c", "cat order.txt"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgsiblingchain",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
group: {
	one: exec.Run & {cmd: ["sh", "-c", "printf 1 >> order.txt"]}
	two: exec.Run & {$after: one, cmd: ["sh", "-c", "printf 2 >> order.txt"]}
}
command: pkgsiblingchain: {
	a: exec.Run & {$after: group.two, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgopenbase",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}
base: {$after: prep, cmd: ["sh", "-c", "printf 2 >> order.txt"]}
two: exec.Run & base
command: pkgopenbase: {
	a: exec.Run & {$after: two, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgreaddeps",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}
run: exec.Run & {$after: prep, cmd: ["sh", "-c", "cat order.txt"], stdout: string}
command: pkgreaddeps: {
	q: cli.Print & {text: "R=\(run.stdout)"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgcmdalias",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}
command: pkgcmdalias: {
	alias: prep
	a: exec.Run & {$after: alias, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgaliasreadfirst",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P"], stdout: string}
command: pkgaliasreadfirst: {
	q: cli.Print & {text: "R=\(prep.stdout)"}
	r: prep
}
"#,
            None,
        ),
        (
            "pkgaliasreaddecl",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P"], stdout: string}
command: pkgaliasreaddecl: {
	r: prep
	q: cli.Print & {text: "R=\(prep.stdout)"}
}
"#,
            None,
        ),
        (
            "pkghiddenread",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P"], stdout: string}
command: pkghiddenread: {
	_r: prep
	r?: prep
	#r: prep
	x: {_h: prep}
	q: cli.Print & {text: "R=\(prep.stdout)"}
	s: prep
}
"#,
            None,
        ),
        (
            "pkgaliastargetafter",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
base: exec.Run & {cmd: ["sh", "-c", "printf B >> order.txt"]}
prep: exec.Run & {$after: base, cmd: ["sh", "-c", "printf P >> order.txt"]}
command: pkgaliastargetafter: {
	alias: prep
	a: exec.Run & {$after: alias, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgaliastargetonly",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
base: exec.Run & {cmd: ["sh", "-c", "printf B >> order.txt"]}
prep: exec.Run & {$after: base, cmd: ["sh", "-c", "printf P >> order.txt"]}
command: pkgaliastargetonly: {
	alias: prep
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgaliastargetcontainer",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
group: {
	one: exec.Run & {cmd: ["sh", "-c", "printf 1 >> order.txt"]}
	two: exec.Run & {$after: one, cmd: ["sh", "-c", "printf 2 >> order.txt"]}
}
command: pkgaliastargetcontainer: {
	alias: group.two
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgreadselectorshadow",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
group: {prep: exec.Run & {cmd: ["sh", "-c", "printf G"], stdout: string}}
command: pkgreadselectorshadow: {
	prep: exec.Run & {cmd: ["sh", "-c", "printf C"], stdout: string}
	q: cli.Print & {text: "R=\(group.prep.stdout)"}
}
"#,
            None,
        ),
        (
            "pkgaliasreadhidden",
            r#"
package foo
import (
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}
command: pkgaliasreadhidden: {
	r: prep
	_x: prep.stdout
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgaliasreadoptional",
            r#"
package foo
import (
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}
command: pkgaliasreadoptional: {
	r: prep
	x?: prep.stdout
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgaliasreaddef",
            r#"
package foo
import (
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}
command: pkgaliasreaddef: {
	r: prep
	#x: prep.stdout
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgnestedhiddenread",
            r#"
package foo
import (
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}
command: pkgnestedhiddenread: {
	x: {
		r: prep
		_y: prep.stdout
	}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgnestedvisibledecl",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}
command: pkgnestedvisibledecl: {
	x: {
		r: prep
		q: cli.Print & {text: "R=\(prep.stdout)"}
	}
}
"#,
            None,
        ),
        (
            "pkgnestedreadfirst",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}
command: pkgnestedreadfirst: {
	x: {
		q: cli.Print & {text: "R=\(prep.stdout)"}
		r: prep
	}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgdeepnestedhidden",
            r#"
package foo
import (
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}
command: pkgdeepnestedhidden: {
	x: {
		y: {
			r: prep
			_z: prep.stdout
		}
	}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgouternestedhidden",
            r#"
package foo
import (
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}
command: pkgouternestedhidden: {
	r: prep
	x: {_y: prep.stdout}
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgnestedouterhidden",
            r#"
package foo
import (
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}
command: pkgnestedouterhidden: {
	x: {r: prep}
	_y: prep.stdout
}
"#,
            Some("order.txt"),
        ),
        (
            "pkgunknownafter",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: pkgunknownafter: {
	a: exec.Run & {$after: missing, cmd: ["true"]}
	q: cli.Print & {text: "ok"}
}
"#,
            None,
        ),
    ];
    for (name, source, artifact) in cases {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        write(&cue_dir.path().join("fixture_tool.cue"), source);
        write(&rust_dir.path().join("fixture_tool.cue"), source);
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{name}: cue stderr: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        if expected.status.success() {
            assert_eq!(actual.stderr, expected.stderr, "{name}");
        }
        if let Some(artifact) = artifact {
            assert_eq!(
                std::fs::read(cue_dir.path().join(artifact)).ok(),
                std::fs::read(rust_dir.path().join(artifact)).ok(),
                "{name}: artifact {artifact} differs"
            );
        }
        if *name == "pkgunreferenced" {
            assert!(!cue_dir.path().join("unused.txt").exists());
            assert!(!rust_dir.path().join("unused.txt").exists());
        }
    }
}

#[test]
fn tool_command_nontask_after_operands_match_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let cases: &[(&str, &str, Option<&str>)] = &[
        (
            "afternontaskfield",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
x: 5
command: afternontaskfield: {
	a: exec.Run & {$after: x, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "afterint",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: afterint: {
	a: exec.Run & {$after: 3, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "afterstring",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: afterstring: {
	a: exec.Run & {$after: "prep", cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "afternestedconcrete",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
group: {x: 1}
command: afternestedconcrete: {
	a: exec.Run & {$after: group.x, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "aftermissingnested",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
group: {x: 1}
command: aftermissingnested: {
	a: exec.Run & {$after: group.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "aftermixed",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
x: 5
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}
command: aftermixed: {
	a: exec.Run & {$after: [prep, x], cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "afteroutputselector",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}
command: afteroutputselector: {
	a: exec.Run & {$after: prep.stdout, cmd: ["sh", "-c", "printf A >> order.txt"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            Some("order.txt"),
        ),
        (
            "aftertaskmissing",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"], stdout: string}
command: aftertaskmissing: {
	a: exec.Run & {$after: prep.missing, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "aftergroupstdout",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
group: {prep: exec.Run & {cmd: ["sh", "-c", "printf G >> order.txt"], stdout: string}}
command: aftergroupstdout: {
	a: exec.Run & {$after: group.prep.stdout, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "afterlocalstdout",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: afterlocalstdout: {
	local: exec.Run & {cmd: ["sh", "-c", "printf L >> order.txt"], stdout: string}
	a: exec.Run & {$after: local.stdout, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "afterlettask",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let y = 5
command: afterlettask: {
	a: exec.Run & {$after: y, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "afterletselect",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let g = {x: 1}
command: afterletselect: {
	a: exec.Run & {$after: g.x, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "aftermathpi",
            r#"
package foo
import (
	"math"
	"tool/cli"
	"tool/exec"
)
command: aftermathpi: {
	a: exec.Run & {$after: math.Pi, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "aftermathmissing",
            r#"
package foo
import (
	"math"
	"tool/cli"
	"tool/exec"
)
pi: math.Pi
command: aftermathmissing: {
	a: exec.Run & {$after: math.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "aftermathmissingonly",
            r#"
package foo
import (
	"math"
	"tool/cli"
	"tool/exec"
)
command: aftermathmissingonly: {
	a: exec.Run & {$after: math.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "afterletstructmissing",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let g = {x: 1}
v: g.x
command: afterletstructmissing: {
	a: exec.Run & {$after: g.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "afterletstructmissingonly",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let g = {x: 1}
command: afterletstructmissingonly: {
	a: exec.Run & {$after: g.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "afterletconcretemissing",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let g = {x: 1}
v: g.x
command: afterletconcretemissing: {
	a: exec.Run & {$after: g.x.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "aftermathpimissing",
            r#"
package foo
import (
	"math"
	"tool/cli"
	"tool/exec"
)
command: aftermathpimissing: {
	a: exec.Run & {$after: math.Pi.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "afterselfletmissing",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let a = a
command: afterselfletmissing: {
	run: exec.Run & {$after: a.missing, cmd: ["true"]}
	q: cli.Print & {text: "ok"}
}
"#,
            None,
        ),
        (
            "afterfieldconcretemissing",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
x: 5
command: afterfieldconcretemissing: {
	a: exec.Run & {$after: x.missing, cmd: ["true"]}
	q: cli.Print & {text: "ok"}
}
"#,
            None,
        ),
        (
            "afterfieldstructmissing",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
x: {y: 1}
command: afterfieldstructmissing: {
	a: exec.Run & {$after: x.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "afterfieldstructconcretemissing",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
x: {y: 1}
command: afterfieldstructconcretemissing: {
	a: exec.Run & {$after: x.y.missing, cmd: ["true"]}
	q: cli.Print & {text: "ok"}
}
"#,
            None,
        ),
        (
            "afterdefmissing",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
#G: {inner: {x: 1}}
command: afterdefmissing: {
	a: exec.Run & {$after: #G.inner.missing, cmd: ["true"]}
	q: cli.Print & {text: "ok"}
}
"#,
            None,
        ),
        (
            "afterletimportmissing",
            r#"
package foo
import (
	"math"
	"tool/cli"
	"tool/exec"
)
let g = math
command: afterletimportmissing: {
	a: exec.Run & {$after: g.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "afteraliasimportmissing",
            r#"
package foo
import (
	"math"
	"tool/cli"
	"tool/exec"
)
alias: math
pi: alias.Pi
command: afteraliasimportmissing: {
	a: exec.Run & {$after: alias.missing, cmd: ["sh", "-c", "printf A"], stdout: string}
	q: cli.Print & {text: "R=\(a.stdout)"}
}
"#,
            None,
        ),
        (
            "afterletimportconcrete",
            r#"
package foo
import (
	"math"
	"tool/cli"
	"tool/exec"
)
let g = math
command: afterletimportconcrete: {
	a: exec.Run & {$after: g.Pi.missing, cmd: ["true"]}
	q: cli.Print & {text: "ok"}
}
"#,
            None,
        ),
        (
            "afterfieldimportconcrete",
            r#"
package foo
import (
	"math"
	"tool/cli"
	"tool/exec"
)
x: math
p: x.Pi
command: afterfieldimportconcrete: {
	a: exec.Run & {$after: x.Pi.missing, cmd: ["true"]}
	q: cli.Print & {text: "ok"}
}
"#,
            None,
        ),
        (
            "afteraliasimportconcrete",
            r#"
package foo
import (
	"math"
	"tool/cli"
	"tool/exec"
)
alias: math
p: alias.Pi
command: afteraliasimportconcrete: {
	a: exec.Run & {$after: alias.Pi.missing, cmd: ["true"]}
	q: cli.Print & {text: "ok"}
}
"#,
            None,
        ),
        (
            "aftercomposed",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}
command: aftercomposed: {
	a: exec.Run & {$after: prep & {stdout: string}, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "afterauxcomposed",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
aux: {stdout: string}
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}
command: afterauxcomposed: {
	a: exec.Run & {$after: aux & prep, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "aftercomposeddef",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
#prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}
command: aftercomposeddef: {
	a: exec.Run & {$after: #prep & {stdout: string}, cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "aftercomposedlist",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt"]}
other: exec.Run & {$after: prep, cmd: ["sh", "-c", "printf O >> order.txt"]}
command: aftercomposedlist: {
	a: exec.Run & {$after: [prep & {stdout: string}, other], cmd: ["sh", "-c", "printf A >> order.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            Some("order.txt"),
        ),
        (
            "afterundefinedident",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: afterundefinedident: {
	a: exec.Run & {$after: missing, cmd: ["true"]}
	q: cli.Print & {text: "ok"}
}
"#,
            None,
        ),
        (
            "afterundefinedselect",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: afterundefinedselect: {
	a: exec.Run & {$after: nope.x, cmd: ["true"]}
	q: cli.Print & {text: "ok"}
}
"#,
            None,
        ),
        (
            "aftermixedmissing",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["true"]}
command: aftermixedmissing: {
	a: exec.Run & {$after: [prep, missing], cmd: ["true"]}
	q: cli.Print & {text: "ok"}
}
"#,
            None,
        ),
    ];
    for (name, source, artifact) in cases {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        write(&cue_dir.path().join("fixture_tool.cue"), source);
        write(&rust_dir.path().join("fixture_tool.cue"), source);
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{name}: cue stderr: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        if let Some(artifact) = artifact {
            assert_eq!(
                std::fs::read(cue_dir.path().join(artifact)).ok(),
                std::fs::read(rust_dir.path().join(artifact)).ok(),
                "{name}: artifact {artifact} differs"
            );
        }
    }
}

#[test]
fn tool_command_package_after_multi_task_matches_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let cases: &[(&str, &str, &[&str])] = &[
        (
            "pkgshadowdep",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf TOP > top.txt"]}
two: exec.Run & {$after: prep, cmd: ["sh", "-c", "printf TWO > two.txt"]}
command: pkgshadowdep: {
	prep: exec.Run & {cmd: ["sh", "-c", "printf CMD > cmd.txt"]}
	a: exec.Run & {$after: two, cmd: ["sh", "-c", "printf A > a.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            &["top.txt", "two.txt", "cmd.txt", "a.txt"],
        ),
        (
            "pkgselectorshadow",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
group: {prep: exec.Run & {cmd: ["sh", "-c", "printf GROUP > group.txt"]}}
command: pkgselectorshadow: {
	prep: exec.Run & {cmd: ["sh", "-c", "printf CMD > cmd.txt"]}
	a: exec.Run & {$after: group.prep, cmd: ["sh", "-c", "printf A > a.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            &["group.txt", "cmd.txt", "a.txt"],
        ),
        (
            "pkgduplicateleaves",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
g1: {prep: exec.Run & {cmd: ["sh", "-c", "printf 1 > g1.txt"]}}
g2: {prep: exec.Run & {cmd: ["sh", "-c", "printf 2 > g2.txt"]}}
command: pkgduplicateleaves: {
	a: exec.Run & {$after: [g1.prep, g2.prep], cmd: ["sh", "-c", "printf A > a.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            &["g1.txt", "g2.txt", "a.txt"],
        ),
        (
            "pkgduplicatechain",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
g1: {
	one: exec.Run & {cmd: ["sh", "-c", "printf 1 > g1one.txt"]}
	two: exec.Run & {$after: one, cmd: ["sh", "-c", "printf 2 > g1two.txt"]}
}
g2: {
	one: exec.Run & {cmd: ["sh", "-c", "printf 3 > g2one.txt"]}
	two: exec.Run & {$after: one, cmd: ["sh", "-c", "printf 4 > g2two.txt"]}
}
command: pkgduplicatechain: {
	a: exec.Run & {$after: [g1.two, g2.two], cmd: ["sh", "-c", "printf A > a.txt"]}
	q: cli.Print & {text: "ok"}
}
"#,
            &["g1one.txt", "g1two.txt", "g2one.txt", "g2two.txt", "a.txt"],
        ),
    ];
    for (name, source, artifacts) in cases {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        write(&cue_dir.path().join("fixture_tool.cue"), source);
        write(&rust_dir.path().join("fixture_tool.cue"), source);
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{name}: cue stderr: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        for artifact in *artifacts {
            assert_eq!(
                std::fs::read(cue_dir.path().join(artifact)).ok(),
                std::fs::read(rust_dir.path().join(artifact)).ok(),
                "{name}: artifact {artifact} differs"
            );
        }
    }
}

#[test]
fn tool_command_alias_target_plain_reads_match_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let cases: &[(&str, &str, Option<&str>)] = &[
        (
            "aliasplaindecl",
            r#"
package foo
import "tool/exec"
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: aliasplaindecl: {
	r: prep
	q: "\(prep.stdout)"
}
"#,
            Some("order.txt"),
        ),
        (
            "aliasplainreadfirst",
            r#"
package foo
import "tool/exec"
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: aliasplainreadfirst: {
	q: "\(prep.stdout)"
	r: prep
}
"#,
            Some("order.txt"),
        ),
        (
            "aliasplainhidden",
            r#"
package foo
import "tool/exec"
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: aliasplainhidden: {
	r: prep
	_q: "\(prep.stdout)"
}
"#,
            Some("order.txt"),
        ),
        (
            "aliastaskinputdecl",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: aliastaskinputdecl: {
	r: prep
	q: cli.Print & {text: "R=\(prep.stdout)"}
}
"#,
            None,
        ),
        (
            "aliastaskinputreadfirst",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: aliastaskinputreadfirst: {
	q: cli.Print & {text: "R=\(prep.stdout)"}
	r: prep
}
"#,
            Some("order.txt"),
        ),
        (
            "aliasplantaskinputafter",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: aliasplantaskinputafter: {
	r: prep
	t: cli.Print & {text: "R=\(prep.stdout)"}
	q: "\(prep.stdout)"
}
"#,
            None,
        ),
        (
            "aliasplantaskinputbetween",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: aliasplantaskinputbetween: {
	r: prep
	q: "\(prep.stdout)"
	t: cli.Print & {text: "R=\(prep.stdout)"}
}
"#,
            None,
        ),
        (
            "plantaskinputfirst",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: plantaskinputfirst: {
	t: cli.Print & {text: "R=\(prep.stdout)"}
	q: "\(prep.stdout)"
	r: prep
}
"#,
            Some("order.txt"),
        ),
        (
            "nestedaliasplantask",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: nestedaliasplantask: {
	x: {
		r: prep
		t: cli.Print & {text: "R=\(prep.stdout)"}
		q: "\(prep.stdout)"
	}
}
"#,
            None,
        ),
        (
            "composedaliastaskplain",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: composedaliastaskplain: {
	r: prep & {stdout: string}
	t: cli.Print & {text: "R=\(prep.stdout)"}
	q: "\(prep.stdout)"
}
"#,
            None,
        ),
        (
            "composedaliasreadfirst",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: composedaliasreadfirst: {
	q: "\(prep.stdout)"
	r: prep & {stdout: string}
	t: cli.Print & {text: "R=\(prep.stdout)"}
}
"#,
            None,
        ),
        (
            "composedaliastaskfirst",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: composedaliastaskfirst: {
	t: cli.Print & {text: "R=\(prep.stdout)"}
	r: prep & {stdout: string}
	q: "\(prep.stdout)"
}
"#,
            Some("order.txt"),
        ),
        (
            "composedcontainertaskplain",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
group: {prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}}
command: composedcontainertaskplain: {
	r: group.prep & {stdout: string}
	t: cli.Print & {text: "R=\(group.prep.stdout)"}
	q: "\(group.prep.stdout)"
}
"#,
            None,
        ),
        (
            "composeddefinitiontaskplain",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
#G: {prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}}
command: composeddefinitiontaskplain: {
	r: #G.prep & {stdout: string}
	t: cli.Print & {text: "R=\(#G.prep.stdout)"}
}
"#,
            None,
        ),
        (
            "auxfirstaliastask",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
aux: {stdout: string}
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: auxfirstaliastask: {
	r: aux & prep
	t: cli.Print & {text: "R=\(prep.stdout)"}
}
"#,
            None,
        ),
        (
            "auxfirstaliasplain",
            r#"
package foo
import "tool/exec"
aux: {stdout: string}
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: auxfirstaliasplain: {
	r: aux & prep
	q: "\(prep.stdout)"
}
"#,
            Some("order.txt"),
        ),
        (
            "auxfirstaliasreadfirst",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
aux: {stdout: string}
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: auxfirstaliasreadfirst: {
	t: cli.Print & {text: "R=\(prep.stdout)"}
	r: aux & prep
	q: "\(prep.stdout)"
}
"#,
            Some("order.txt"),
        ),
        (
            "groupauxfirstalias",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
group: {
	aux: {stdout: string}
	prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
}
command: groupauxfirstalias: {
	r: group.aux & group.prep
	t: cli.Print & {text: "R=\(group.prep.stdout)"}
}
"#,
            None,
        ),
        (
            "aliascontainerplain",
            r#"
package foo
import "tool/exec"
group: {prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}}
command: aliascontainerplain: {
	r: group.prep
	q: "\(group.prep.stdout)"
}
"#,
            Some("order.txt"),
        ),
        (
            "plainnoalias",
            r#"
package foo
import "tool/exec"
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: plainnoalias: {
	a: exec.Run & {cmd: ["true"]}
	q: "\(prep.stdout)"
}
"#,
            None,
        ),
        (
            "aliastwoplain",
            r#"
package foo
import "tool/exec"
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: aliastwoplain: {
	r1: prep
	r2: prep
	q: "\(prep.stdout)"
}
"#,
            Some("order.txt"),
        ),
        (
            "aliasafterplain",
            r#"
package foo
import "tool/exec"
base: exec.Run & {cmd: ["sh", "-c", "printf B >> order.txt"]}
prep: exec.Run & {$after: base, cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: aliasafterplain: {
	r: prep
	q: "\(prep.stdout)"
}
"#,
            Some("order.txt"),
        ),
        (
            "aliasnestedplain",
            r#"
package foo
import "tool/exec"
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: aliasnestedplain: {
	x: {
		r: prep
		q: "\(prep.stdout)"
	}
}
"#,
            Some("order.txt"),
        ),
        (
            "aliasplaincomposed",
            r#"
package foo
import "tool/exec"
prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: aliasplaincomposed: {
	r: prep & {stdout: string}
	q: "\(prep.stdout)"
}
"#,
            Some("order.txt"),
        ),
        (
            "aliasplaindefinition",
            r#"
package foo
import "tool/exec"
#prep: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
command: aliasplaindefinition: {
	r: #prep
	q: "\(#prep.stdout)"
}
"#,
            Some("order.txt"),
        ),
        (
            "aliasplainlocal",
            r#"
package foo
import "tool/exec"
command: aliasplainlocal: {
	local: exec.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
	r: local
	q: "\(local.stdout)"
}
"#,
            Some("order.txt"),
        ),
        (
            "aliasplainsuccess",
            r#"
package foo
import "tool/exec"
prep: exec.Run & {cmd: ["false"], mustSucceed: false, success: bool}
command: aliasplainsuccess: {
	r: prep
	q: "\(prep.success)"
}
"#,
            None,
        ),
    ];
    for (name, source, artifact) in cases {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        write(&cue_dir.path().join("fixture_tool.cue"), source);
        write(&rust_dir.path().join("fixture_tool.cue"), source);
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{name}: cue stderr: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        if let Some(artifact) = artifact {
            assert_eq!(
                std::fs::read(cue_dir.path().join(artifact)).ok(),
                std::fs::read(rust_dir.path().join(artifact)).ok(),
                "{name}: artifact {artifact} differs"
            );
        } else if expected.status.success() {
            assert!(
                !cue_dir.path().join("order.txt").exists(),
                "{name}: cue side effect"
            );
            assert!(
                !rust_dir.path().join("order.txt").exists(),
                "{name}: rust side effect"
            );
        } else {
            // CUE may or may not leave side effects behind when evaluation
            // rejects; only our own non-execution is deterministic.
            assert!(
                !rust_dir.path().join("order.txt").exists(),
                "{name}: rust side effect"
            );
        }
    }
}

#[test]
fn tool_command_let_and_definition_output_reads_match_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let module = "module: \"example.com/k\"\nlanguage: version: \"v0.17.1\"\n";
    let library = r#"package lib

import e "tool/exec"

run: e.Run & {cmd: ["sh", "-c", "printf abc"], stdout: string}
"#;
    let cases = [
        (
            "letreadfield",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
base: exec.Run & {cmd: ["sh", "-c", "printf B"], stdout: string}
command: letreadfield: {
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            true,
        ),
        (
            "letreadcmd",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: letreadcmd: {
	base: lib.run
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            true,
        ),
        (
            "letreadchain",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: letreadchain: {
	base: lib.run
	let u = base
	let t = u
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            true,
        ),
        (
            "letreadfirst",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
base: exec.Run & {cmd: ["sh", "-c", "printf B"], stdout: string}
command: letreadfirst: {
	q: cli.Print & {text: "R=\(t.stdout)"}
	let t = base
}
"#,
            true,
        ),
        (
            "defreadmodule",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#base: lib.run
command: defreadmodule: {
	q: cli.Print & {text: "R=\(#base.stdout)"}
}
"#,
            true,
        ),
        (
            "defreadinline",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
#t: exec.Run & {cmd: ["sh", "-c", "printf T"], stdout: string}
command: defreadinline: {
	q: cli.Print & {text: "R=\(#t.stdout)"}
}
"#,
            true,
        ),
        (
            "defreadlocal",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: defreadlocal: {
	#base: lib.run
	q: cli.Print & {text: "R=\(#base.stdout)"}
}
"#,
            true,
        ),
        (
            "defreadtaskref",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#base: lib.run
command: defreadtaskref: {
	a: p1: #base
	q: cli.Print & {text: "R=\(a.p1.stdout)"}
}
"#,
            false,
        ),
        (
            "defreadboth",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#base: lib.run
command: defreadboth: {
	a: p1: #base
	q: cli.Print & {text: "R=\(#base.stdout)"}
}
"#,
            false,
        ),
        (
            "defcontainermodule",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defcontainermodule: {
	q: cli.Print & {text: "R=\(#G.run.stdout)"}
}
"#,
            true,
        ),
        (
            "defcontainerinline",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
#G: {run: exec.Run & {cmd: ["sh", "-c", "printf T"], stdout: string}}
command: defcontainerinline: {
	q: cli.Print & {text: "R=\(#G.run.stdout)"}
}
"#,
            true,
        ),
        (
            "defcontainerdeep",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {group: {run: lib.run}}
command: defcontainerdeep: {
	q: cli.Print & {text: "R=\(#G.group.run.stdout)"}
}
"#,
            true,
        ),
        (
            "defcontainercmd",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: defcontainercmd: {
	#G: {run: lib.run}
	q: cli.Print & {text: "R=\(#G.run.stdout)"}
}
"#,
            true,
        ),
        (
            "defalias",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#a: #b
#b: lib.run
command: defalias: {
	q: cli.Print & {text: "R=\(#a.stdout)"}
}
"#,
            true,
        ),
        (
            "defaliaschain",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#a: #b
#b: #c
#c: lib.run
command: defaliaschain: {
	q: cli.Print & {text: "R=\(#a.stdout)"}
}
"#,
            true,
        ),
        (
            "defaliastaskref",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#a: #b
#b: lib.run
command: defaliastaskref: {
	x: p1: #a
	q: cli.Print & {text: "R=\(x.p1.stdout)"}
}
"#,
            true,
        ),
        (
            "defcontainertaskref",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defcontainertaskref: {
	a: p1: #G.run
	q: cli.Print & {text: "R=\(a.p1.stdout)"}
}
"#,
            false,
        ),
        (
            "defaliasfield",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run
#a: base
command: defaliasfield: {
	x: p1: #a
	q: cli.Print & {text: "R=\(x.p1.stdout)"}
}
"#,
            false,
        ),
        (
            "defaliasdefchain",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run
#a: #b
#b: base
command: defaliasdefchain: {
	x: p1: #a
	q: cli.Print & {text: "R=\(x.p1.stdout)"}
}
"#,
            true,
        ),
        (
            "deftwocontainers",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#A: {run: lib.run}
#B: {run: lib.run}
command: deftwocontainers: {
	q: cli.Print & {text: "R=\(#B.run.stdout)"}
}
"#,
            true,
        ),
        (
            "defshadowrun",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#B: {run: lib.run}
other: {run: 1}
command: defshadowrun: {
	q: cli.Print & {text: "R=\(#B.run.stdout)"}
}
"#,
            true,
        ),
        (
            "defcontaineralias",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#b: {run: lib.run}
#a: #b
command: defcontaineralias: {
	q: cli.Print & {text: "R=\(#a.run.stdout)"}
}
"#,
            true,
        ),
        (
            "defcontainerdeclread",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defcontainerdeclread: {
	a: p1: #G.run
	q: cli.Print & {text: "R=\(#G.run.stdout)"}
}
"#,
            false,
        ),
        (
            "defcontainerreadfirst",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defcontainerreadfirst: {
	q: cli.Print & {text: "R=\(#G.run.stdout)"}
	a: p1: #G.run
}
"#,
            true,
        ),
        (
            "defaliasreadfirst",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#b: {run: lib.run}
#a: #b
command: defaliasreadfirst: {
	q: cli.Print & {text: "R=\(#a.run.stdout)"}
	x: p1: #b.run
}
"#,
            true,
        ),
        (
            "defaliasdeclread",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#b: {run: lib.run}
#a: #b
command: defaliasdeclread: {
	x: p1: #a.run
	q: cli.Print & {text: "R=\(#a.run.stdout)"}
}
"#,
            false,
        ),
        (
            "defcontaineraliasdecl",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#b: {run: lib.run}
#a: #b
command: defcontaineraliasdecl: {
	x: p1: #a.run
	q: cli.Print & {text: "R=\(x.p1.stdout)"}
}
"#,
            false,
        ),
        (
            "defletreadafter",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defletreadafter: {
	let s = #G.run.stdout
	a: p1: #G.run
	q: cli.Print & {text: "R=\(s)"}
}
"#,
            false,
        ),
        (
            "defletreadbefore",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defletreadbefore: {
	let s = #G.run.stdout
	q: cli.Print & {text: "R=\(s)"}
	a: p1: #G.run
}
"#,
            true,
        ),
        (
            "defaliasreadafter",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defaliasreadafter: {
	r: #G.run.stdout
	a: p1: #G.run
	q: cli.Print & {text: "R=\(r)"}
}
"#,
            false,
        ),
        (
            "defaliasreadfirst",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defaliasreadfirst: {
	r: #G.run.stdout
	q: cli.Print & {text: "R=\(r)"}
	a: p1: #G.run
}
"#,
            true,
        ),
        (
            "defnestedorder",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defnestedorder: {
	a: {
		p: #G.run
		q: cli.Print & {text: "R=\(#G.run.stdout)"}
	}
}
"#,
            false,
        ),
        (
            "defhiddendecl",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defhiddendecl: {
	_x: #G.run
	q: cli.Print & {text: "R=\(#G.run.stdout)"}
}
"#,
            false,
        ),
        (
            "defoptionaldecl",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defoptionaldecl: {
	x?: #G.run
	q: cli.Print & {text: "R=\(#G.run.stdout)"}
}
"#,
            false,
        ),
        (
            "defnesteddefdecl",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defnesteddefdecl: {
	#D: {p: #G.run}
	q: cli.Print & {text: "R=\(#G.run.stdout)"}
}
"#,
            false,
        ),
        (
            "defdynamicdecl",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
x: "a"
command: defdynamicdecl: {
	"\(x)": p1: #G.run
	q: cli.Print & {text: "R=\(#G.run.stdout)"}
}
"#,
            false,
        ),
        (
            "defquoteddecl",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defquoteddecl: {
	"_x": p1: #G.run
	q: cli.Print & {text: "R=\(#G.run.stdout)"}
}
"#,
            false,
        ),
        (
            "defselectoraliasdecl",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defselectoraliasdecl: {
	a: p1: #G.run
	r: {s: #G.run.stdout}
	q: cli.Print & {text: "R=\(r.s)"}
}
"#,
            false,
        ),
        (
            "defselectoraliasfirst",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
command: defselectoraliasfirst: {
	r: {s: #G.run.stdout}
	q: cli.Print & {text: "R=\(r.s)"}
	a: p1: #G.run
}
"#,
            true,
        ),
        (
            "defcontainerfieldalias",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
r: #G
command: defcontainerfieldalias: {
	q: cli.Print & {text: "R=\(r.run.stdout)"}
}
"#,
            true,
        ),
        (
            "defcontainerfieldaliasdecl",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
r: #G
command: defcontainerfieldaliasdecl: {
	a: p1: r.run
	q: cli.Print & {text: "R=\(a.p1.stdout)"}
}
"#,
            false,
        ),
        (
            "defaliastwiceread",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#a: #b
#b: lib.run
command: defaliastwiceread: {
	x: p1: #a
	y: p2: #a
	q: cli.Print & {text: "R=\(x.p1.stdout)-\(y.p2.stdout)"}
}
"#,
            true,
        ),
        (
            "defaliascontainerdeclread",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
r: #G
command: defaliascontainerdeclread: {
	a: p1: r.run
	q: cli.Print & {text: "R=\(r.run.stdout)"}
}
"#,
            false,
        ),
        (
            "defaliascontainerreadfirst",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
r: #G
command: defaliascontainerreadfirst: {
	q: cli.Print & {text: "R=\(r.run.stdout)"}
	a: p1: r.run
}
"#,
            true,
        ),
        (
            "defconcretestructread",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
r: {run: lib.run}
command: defconcretestructread: {
	q: cli.Print & {text: "R=\(r.run.stdout)"}
}
"#,
            false,
        ),
        (
            "defletcontainerread",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
let G = {run: lib.run}
r: G
command: defletcontainerread: {
	q: cli.Print & {text: "R=\(r.run.stdout)"}
}
"#,
            false,
        ),
        (
            "defrootnestedread",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
root: p1: lib.run
command: defrootnestedread: {
	q: cli.Print & {text: "R=\(root.p1.stdout)"}
}
"#,
            false,
        ),
        (
            "defstructfieldtask",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
r: {run: #G.run}
command: defstructfieldtask: {
	q: cli.Print & {text: "R=\(r.run.stdout)"}
}
"#,
            true,
        ),
        (
            "defstructfieldtaskdecl",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
r: {run: #G.run}
command: defstructfieldtaskdecl: {
	d: p: r.run
	q: cli.Print & {text: "R=\(r.run.stdout)"}
}
"#,
            true,
        ),
        (
            "defnestedaliascontainer",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#G: {run: lib.run}
n: r: #G
command: defnestedaliascontainer: {
	q: cli.Print & {text: "R=\(n.r.run.stdout)"}
}
"#,
            true,
        ),
        (
            "readrootmodule",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run
command: readrootmodule: {
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            false,
        ),
        (
            "readrootmodulechain",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run
other: base
command: readrootmodulechain: {
	let t = other
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            false,
        ),
        (
            "readrootmodulecomposed",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run & {stdout: string}
command: readrootmodulecomposed: {
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            true,
        ),
        (
            "readrootmodulecomposeddecl",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run
base: lib.run & {stdout: string}
command: readrootmodulecomposeddecl: {
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            true,
        ),
        (
            "readrootmodulestructdecl",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run
base: {stdout: string}
command: readrootmodulestructdecl: {
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            true,
        ),
        (
            "readrootmodulebaretwice",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run
base: lib.run
command: readrootmodulebaretwice: {
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            false,
        ),
        (
            "readrootunifybare",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run
base: lib.run & lib.run
command: readrootunifybare: {
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            false,
        ),
        (
            "readrootunifyopen",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run
base: lib.run & _
command: readrootunifyopen: {
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            false,
        ),
        (
            "readrootunifybarealone",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run & lib.run
command: readrootunifybarealone: {
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            false,
        ),
        (
            "readrootdisjunction",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run & (string | {stdout: string})
command: readrootdisjunction: {
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            true,
        ),
        (
            "readrootselector",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
x: {y: {stdout: string}}
base: lib.run & x.y
command: readrootselector: {
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            true,
        ),
        (
            "readrootlistindex",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
x: [{stdout: string}]
base: lib.run & x[0]
command: readrootlistindex: {
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            true,
        ),
        (
            "readrootselectorbare",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
x: {y: lib.run}
base: lib.run & x.y
command: readrootselectorbare: {
	let t = base
	q: cli.Print & {text: "R=\(t.stdout)"}
}
"#,
            false,
        ),
        (
            "readrootfield",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run
command: readrootfield: {
	q: cli.Print & {text: "R=\(base.stdout)"}
}
"#,
            false,
        ),
    ];
    for (name, source, success) in cases {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        for directory in [cue_dir.path(), rust_dir.path()] {
            std::fs::create_dir_all(directory.join("cue.mod")).expect("cue.mod");
            std::fs::create_dir_all(directory.join("lib")).expect("lib");
            write(&directory.join("cue.mod/module.cue"), module);
            write(&directory.join("lib/lib.cue"), library);
            write(&directory.join("x_tool.cue"), source);
        }
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{name}: cue stderr: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        if success {
            assert_eq!(actual.stderr, expected.stderr, "{name}");
        }
    }
}

#[test]
fn tool_command_let_task_outputs_match_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let module = "module: \"example.com/k\"\nlanguage: version: \"v0.17.1\"\n";
    let library = r#"package lib

import e "tool/exec"

run: e.Run & {cmd: ["sh", "-c", "printf abc"], stdout: string}
"#;
    let cases = [
        (
            "letinline",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: letinline: {
	let t = exec.Run & {cmd: ["sh", "-c", "printf INL"], stdout: string}
	a: producer: t
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}
"#,
            true,
        ),
        (
            "letmodule",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: letmodule: {
	let t = lib.run
	a: producer: t
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}
"#,
            true,
        ),
        (
            "letouter",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let t = exec.Run & {cmd: ["sh", "-c", "printf OUT"], stdout: string}
command: letouter: {
	a: producer: t
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}
"#,
            true,
        ),
        (
            "letcomposed",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: letcomposed: {
	let t = exec.Run & {cmd: ["sh", "-c", "printf C"], stdout: string}
	a: producer: t & {stdout: string}
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}
"#,
            true,
        ),
        (
            "letsuccess",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: letsuccess: {
	let t = exec.Run & {cmd: ["false"], success: bool, mustSucceed: false}
	a: producer: t
	q: cli.Print & {text: "ok=\(a.producer.success)"}
}
"#,
            true,
        ),
        (
            "letinlinetwice",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
command: letinlinetwice: {
	let t = exec.Run & {cmd: ["sh", "-c", "printf T"], stdout: string}
	a: p1: t
	b: p2: t
	q: cli.Print & {text: "R=\(a.p1.stdout)-\(b.p2.stdout)"}
}
"#,
            true,
        ),
        (
            "lettwoname",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: lettwoname: {
	let t1 = lib.run
	let t2 = lib.run
	a: p1: t1
	b: p2: t2
	q: cli.Print & {text: "R=\(a.p1.stdout)-\(b.p2.stdout)"}
}
"#,
            false,
        ),
        (
            "letplusdirect",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: letplusdirect: {
	let t = lib.run
	a: p1: t
	b: p2: lib.run
	q: cli.Print & {text: "R=\(a.p1.stdout)-\(b.p2.stdout)"}
}
"#,
            false,
        ),
        (
            "letchain",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: letchain: {
	let a = lib.run
	let b = a
	x: p1: a
	y: p2: b
	q: cli.Print & {text: "R=\(x.p1.stdout)-\(y.p2.stdout)"}
}
"#,
            false,
        ),
        (
            "letfieldmodule",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run
command: letfieldmodule: {
	a: p1: base
	q: cli.Print & {text: "R=\(a.p1.stdout)"}
}
"#,
            true,
        ),
        (
            "letfieldtwice",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run
command: letfieldtwice: {
	a: p1: base
	b: p2: base
	q: cli.Print & {text: "R=\(a.p1.stdout)-\(b.p2.stdout)"}
}
"#,
            false,
        ),
        (
            "letcommandfield",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: letcommandfield: {
	base: lib.run
	a: p1: base
	b: p2: base
	q: cli.Print & {text: "R=\(a.p1.stdout)-\(b.p2.stdout)"}
}
"#,
            true,
        ),
        (
            "letdefref",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#base: lib.run
command: letdefref: {
	a: p1: #base
	q: cli.Print & {text: "R=\(a.p1.stdout)"}
}
"#,
            false,
        ),
        (
            "letcmddefref",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: letcmddefref: {
	#base: lib.run
	a: p1: #base
	q: cli.Print & {text: "R=\(a.p1.stdout)"}
}
"#,
            false,
        ),
        (
            "letdefalias",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#base: lib.run
x: #base
command: letdefalias: {
	a: p1: x
	q: cli.Print & {text: "R=\(a.p1.stdout)"}
}
"#,
            true,
        ),
        (
            "letdefletref",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
#base: lib.run
command: letdefletref: {
	let t = #base
	a: p1: t
	q: cli.Print & {text: "R=\(a.p1.stdout)"}
}
"#,
            false,
        ),
        (
            "letfieldalias",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: lib.run
command: letfieldalias: {
	let t = base
	a: p1: t
	q: cli.Print & {text: "R=\(a.p1.stdout)"}
}
"#,
            true,
        ),
        (
            "lettwice",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: lettwice: {
	let t = lib.run
	a: p1: t
	b: p2: t
	q: cli.Print & {text: "R=\(a.p1.stdout)-\(b.p2.stdout)"}
}
"#,
            false,
        ),
        (
            "letfieldref",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
base: exec.Run & {cmd: ["sh", "-c", "printf B"], stdout: string}
command: letfieldref: {
	run: base
	q: cli.Print & {text: "R=\(run.stdout)"}
}
"#,
            false,
        ),
    ];
    for (name, source, success) in cases {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        for directory in [cue_dir.path(), rust_dir.path()] {
            std::fs::create_dir_all(directory.join("cue.mod")).expect("cue.mod");
            std::fs::create_dir_all(directory.join("lib")).expect("lib");
            write(&directory.join("cue.mod/module.cue"), module);
            write(&directory.join("lib/lib.cue"), library);
            write(&directory.join("x_tool.cue"), source);
        }
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{name}: cue stderr: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        if success {
            assert_eq!(actual.stderr, expected.stderr, "{name}");
        }
    }
}

#[test]
fn tool_command_module_task_exports_match_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let module = "module: \"example.com/k\"\nlanguage: version: \"v0.17.1\"\n";
    let library = r#"package lib

import (
	e "tool/exec"
	"tool/cli"
)

run:  e.Run & {cmd: ["sh", "-c", "printf abc"], stdout: string}
ok:   e.Run & {cmd: ["false"], success: bool, mustSucceed: false}
tasks: {
	r1: e.Run & {cmd: ["sh", "-c", "printf A"], stdout: string}
	r2: e.Run & {cmd: ["sh", "-c", "printf B"], stdout: string}
}
group: {
	run: e.Run & {cmd: ["sh", "-c", "printf G"], stdout: string}
	p:   cli.Print & {text: "G=\(run.stdout)"}
}
val: {stdout: "concrete"}
"#;
    let cases = [
        (
            "moduledirect",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: moduledirect: {
	a: producer: lib.run
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}
"#,
        ),
        (
            "moduledeep",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: moduledeep: {
	a: producer: lib.tasks.r1
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}
"#,
        ),
        (
            "modulegroup",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: modulegroup: {
	a: producer: lib.group.run
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}
"#,
        ),
        (
            "modulesuccess",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: modulesuccess: {
	a: producer: lib.ok
	q: cli.Print & {text: "ok=\(a.producer.success)"}
}
"#,
        ),
        (
            "modulecollision",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: modulecollision: {
	p: {producer: lib.val}
	q: cli.Print & {text: "S=\(p.producer.stdout)"}
	a: producer: lib.run
}
"#,
        ),
        (
            "moduleconcreteuse",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: moduleconcreteuse: {
	a: producer: lib.run
	x: other: lib.val
	q: cli.Print & {text: "R=\(a.producer.stdout)-S=\(x.other.stdout)"}
}
"#,
        ),
        (
            "modulecomposed",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: {
	a: p1: lib.run & {stdout: string}
	b: p2: lib.tasks.r1 & {stdout: string}
	q: cli.Print & {text: "R=\(a.p1.stdout)-\(b.p2.stdout)"}
}
command: modulecomposed: base
"#,
        ),
        (
            "moduleignoreddef",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: moduleignoreddef: {
	a: producer: lib.run
	#template: {t: lib.tasks.r2}
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}
"#,
        ),
        (
            "moduleignoredoptional",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: moduleignoredoptional: {
	a: producer: lib.run
	b?: producer2: lib.tasks.r2
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}
"#,
        ),
        (
            "moduleignoredif",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: moduleignoredif: {
	a: producer: lib.run
	if false { b: producer2: lib.tasks.r2 }
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}
"#,
        ),
        (
            "moduleignoredhidden",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: moduleignoredhidden: {
	a: producer: lib.run
	_hidden: producer2: lib.tasks.r2
	q: cli.Print & {text: "R=\(a.producer.stdout)"}
}
"#,
        ),
    ];
    for (name, source) in cases {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        for directory in [cue_dir.path(), rust_dir.path()] {
            std::fs::create_dir_all(directory.join("cue.mod")).expect("cue.mod");
            std::fs::create_dir_all(directory.join("lib")).expect("lib");
            write(&directory.join("cue.mod/module.cue"), module);
            write(&directory.join("lib/lib.cue"), library);
            write(&directory.join("x_tool.cue"), source);
        }
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{name}: cue stderr: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        assert_eq!(actual.stderr, expected.stderr, "{name}");
    }

    // CUE leaves both tasks non-concrete when one module supplies two tasks.
    let source = r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: moduletwo: {
	a: producer: lib.tasks.r1
	b: producer2: lib.tasks.r2
	q: cli.Print & {text: "R=\(a.producer.stdout)-\(b.producer2.stdout)"}
}
"#;
    let cue_dir = tempfile::tempdir().expect("tempdir");
    let rust_dir = tempfile::tempdir().expect("tempdir");
    for directory in [cue_dir.path(), rust_dir.path()] {
        std::fs::create_dir_all(directory.join("cue.mod")).expect("cue.mod");
        std::fs::create_dir_all(directory.join("lib")).expect("lib");
        write(&directory.join("cue.mod/module.cue"), module);
        write(&directory.join("lib/lib.cue"), library);
        write(&directory.join("x_tool.cue"), source);
    }
    let expected = Command::new(&cue)
        .args(["cmd", "moduletwo"])
        .current_dir(cue_dir.path())
        .output()
        .expect("run cue cmd");
    let actual = ost(&["cmd", "moduletwo", rust_dir.path().to_str().expect("path")]);
    assert!(
        !expected.status.success(),
        "cue must reject two module tasks"
    );
    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);

    // Suppression must follow referenced/let/parenthesised command values.
    let referenced = [
        (
            "moduletwobase",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: {
	a: p1: lib.tasks.r1
	b: p2: lib.tasks.r2
	q: cli.Print & {text: "R=\(a.p1.stdout)-\(b.p2.stdout)"}
}
command: moduletwobase: base
"#,
        ),
        (
            "moduletwolet",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: moduletwolet: {
	let base = {
		a: p1: lib.tasks.r1
		b: p2: lib.tasks.r2
		q: cli.Print & {text: "R=\(a.p1.stdout)-\(b.p2.stdout)"}
	}
	x: base
}
"#,
        ),
        (
            "moduletwoparen",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: {
	a: p1: lib.tasks.r1
	b: p2: lib.tasks.r2
	q: cli.Print & {text: "R=\(a.p1.stdout)-\(b.p2.stdout)"}
}
command: moduletwoparen: (base)
"#,
        ),
    ];
    for (name, source) in referenced {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        for directory in [cue_dir.path(), rust_dir.path()] {
            std::fs::create_dir_all(directory.join("cue.mod")).expect("cue.mod");
            std::fs::create_dir_all(directory.join("lib")).expect("lib");
            write(&directory.join("cue.mod/module.cue"), module);
            write(&directory.join("lib/lib.cue"), library);
            write(&directory.join("x_tool.cue"), source);
        }
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        assert!(!expected.status.success(), "{name}: cue must reject");
        assert_eq!(actual.status.code(), expected.status.code(), "{name}");
        assert_eq!(actual.stdout, expected.stdout, "{name}");
    }
}

#[test]
fn tool_command_second_hop_alias_chains_match_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let module = "module: \"example.com/k\"\nlanguage: version: \"v0.17.1\"\n";
    let library = r#"package lib

import e "tool/exec"

run: e.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
#def: e.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
"#;
    let base = "base: exec.Run & {cmd: [\"sh\", \"-c\", \"printf P >> order.txt; printf out\"], stdout: string}\n";
    let module_import = "package foo\nimport \"example.com/k/lib\"\n";
    let module_cli = "package foo\nimport (\n\t\"tool/cli\"\n\t\"example.com/k/lib\"\n)\n";
    let local_cli = "package foo\nimport (\n\t\"tool/cli\"\n\t\"tool/exec\"\n)\n";
    let local = format!("package foo\nimport \"tool/exec\"\n{base}");
    let cases: Vec<(&str, String, bool)> = vec![
        (
            "secondhopdirectmodule",
            format!(
                "{module_import}prep: lib.run\ncommand: secondhopdirectmodule: {{ r: prep }}\n"
            ),
            false,
        ),
        (
            "secondhopmodule",
            format!(
                "{module_import}prep: lib.run\np2: prep\ncommand: secondhopmodule: {{ r: p2 }}\n"
            ),
            false,
        ),
        (
            "secondhopmoduleplain",
            format!(
                "{module_import}prep: lib.run\np2: prep\ncommand: secondhopmoduleplain: {{\n\tr: p2\n\tq: \"\\\\(p2.stdout)\"\n}}\n"
            ),
            false,
        ),
        (
            "secondhopmoduleoutput",
            format!(
                "{module_import}prep: lib.run\np2: prep\ncommand: secondhopmoduleoutput: {{\n\tr: p2\n\tq: \"\\\\(r.stdout)\"\n}}\n"
            ),
            false,
        ),
        (
            "secondhopmoduletask",
            format!(
                "{module_cli}prep: lib.run\np2: prep\ncommand: secondhopmoduletask: {{\n\tr: p2\n\tt: cli.Print & {{text: \"R=\\\\(p2.stdout)\"}}\n}}\n"
            ),
            false,
        ),
        (
            "secondhopmoduledef",
            format!(
                "{module_import}prep: lib.#def\np2: prep\ncommand: secondhopmoduledef: {{ r: p2 }}\n"
            ),
            false,
        ),
        (
            "secondhoplocaldirect",
            format!("{local}command: secondhoplocaldirect: {{ r: base }}\n"),
            false,
        ),
        (
            "secondhoplocal",
            format!("{local}p2: base\ncommand: secondhoplocal: {{ r: p2 }}\n"),
            false,
        ),
        (
            "secondhoplocalplain",
            format!(
                "{local}p2: base\ncommand: secondhoplocalplain: {{\n\tr: p2\n\tq: \"\\\\(p2.stdout)\"\n}}\n"
            ),
            false,
        ),
        (
            "secondhoplocaltask",
            format!(
                "{local_cli}{base}p2: base\ncommand: secondhoplocaltask: {{\n\tr: p2\n\tt: cli.Print & {{text: \"R=\\\\(p2.stdout)\"}}\n}}\n"
            ),
            false,
        ),
        (
            "secondhopthree",
            format!("{local}p2: base\np3: p2\ncommand: secondhopthree: {{ r: p3 }}\n"),
            false,
        ),
        (
            "secondhopcomposed",
            format!(
                "{local}p2: base & {{stdout: string}}\ncommand: secondhopcomposed: {{ r: p2 }}\n"
            ),
            false,
        ),
        (
            "secondhoplet",
            format!(
                "{module_import}let l = lib.run\nprep: l\ncommand: secondhoplet: {{ r: prep }}\n"
            ),
            false,
        ),
        (
            "secondhopdefinition",
            "package foo\nimport \"tool/exec\"\n#base: exec.Run & {cmd: [\"sh\", \"-c\", \"printf P >> order.txt; printf out\"], stdout: string}\nprep: #base\ncommand: secondhopdefinition: { r: prep }\n".to_owned(),
            false,
        ),
        (
            "secondhopstruct",
            "package foo\nimport \"tool/exec\"\ngroup: {base: exec.Run & {cmd: [\"sh\", \"-c\", \"printf P >> order.txt; printf out\"], stdout: string}}\np2: group.base\ncommand: secondhopstruct: { r: p2 }\n".to_owned(),
            false,
        ),
        (
            "secondhoptaskbase",
            format!(
                "{local_cli}{base}p2: base\ncommand: secondhoptaskbase: {{\n\tr: p2\n\tt: cli.Print & {{text: \"R=\\\\(base.stdout)\"}}\n}}\n"
            ),
            false,
        ),
        (
            "secondhoptaskgroupbase",
            "package foo\nimport (\n\t\"tool/cli\"\n\t\"tool/exec\"\n)\ngroup: {base: exec.Run & {cmd: [\"sh\", \"-c\", \"printf P >> order.txt; printf out\"], stdout: string}}\np2: group.base\ncommand: secondhoptaskgroupbase: {\n\tr: p2\n\tt: cli.Print & {text: \"R=\\(group.base.stdout)\"}\n}\n".to_owned(),
            false,
        ),
        (
            "secondhoptaskcomposed",
            format!(
                "{local_cli}{base}p2: base & {{stdout: string}}\ncommand: secondhoptaskcomposed: {{\n\tr: p2\n\tt: cli.Print & {{text: \"R=\\\\(base.stdout)\"}}\n}}\n"
            ),
            false,
        ),
        (
            "secondhoptwonestedtask",
            format!(
                "{local_cli}{base}p2: base\ncommand: secondhoptwonestedtask: {{\n\tx: {{\n\t\tr: p2\n\t\tt: cli.Print & {{text: \"R=\\\\(p2.stdout)\"}}\n\t}}\n}}\n"
            ),
            false,
        ),
        (
            "secondhopmoduletaskprep",
            format!(
                "{module_cli}prep: lib.run\np2: prep\ncommand: secondhopmoduletaskprep: {{\n\tr: p2\n\tt: cli.Print & {{text: \"R=\\\\(prep.stdout)\"}}\n}}\n"
            ),
            false,
        ),
        (
            "secondhopmoduletaskp2",
            format!(
                "{module_cli}prep: lib.run\np2: prep\ncommand: secondhopmoduletaskp2: {{\n\tr: p2\n\tt: cli.Print & {{text: \"R=\\\\(p2.stdout)\"}}\n}}\n"
            ),
            false,
        ),
        (
            "secondhopdefinitionread",
            "package foo\nimport (\n\t\"tool/cli\"\n\t\"tool/exec\"\n)\n#base: exec.Run & {cmd: [\"sh\", \"-c\", \"printf P >> order.txt; printf out\"], stdout: string}\nprep: #base\ncommand: secondhopdefinitionread: {\n\tr: prep\n\tt: cli.Print & {text: \"R=\\(prep.stdout)\"}\n}\n".to_owned(),
            false,
        ),
        (
            "secondhopcommandlocal",
            format!("{local}command: secondhopcommandlocal: {{\n\tr: base\n\tr2: r\n}}\n"),
            false,
        ),
    ];
    for (name, source, _) in cases {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        for directory in [cue_dir.path(), rust_dir.path()] {
            std::fs::create_dir_all(directory.join("cue.mod")).expect("cue.mod");
            std::fs::create_dir_all(directory.join("lib")).expect("lib");
            write(&directory.join("cue.mod/module.cue"), module);
            write(&directory.join("lib/lib.cue"), library);
            write(&directory.join("x_tool.cue"), &source);
        }
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{name}: cue stderr: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        assert_eq!(
            std::fs::read(cue_dir.path().join("order.txt")).ok(),
            std::fs::read(rust_dir.path().join("order.txt")).ok(),
            "{name}: order.txt differs"
        );
    }
}

#[test]
fn tool_command_module_alias_plain_reads_match_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let module = "module: \"example.com/k\"\nlanguage: version: \"v0.17.1\"\n";
    let library = r#"package lib

import e "tool/exec"

run: e.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
fail: e.Run & {cmd: ["false"], mustSucceed: false, success: bool}
group: {
	run: e.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
	#inner: e.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
}
#def: e.Run & {cmd: ["sh", "-c", "printf P >> order.txt; printf out"], stdout: string}
"#;
    let cases: &[(&str, &str, bool)] = &[
        (
            "modulealiasplain",
            r#"
package foo
import "example.com/k/lib"
command: modulealiasplain: {
	r: lib.run
	q: "\(lib.run.stdout)"
}
"#,
            true,
        ),
        (
            "modulealiasplainreadfirst",
            r#"
package foo
import "example.com/k/lib"
command: modulealiasplainreadfirst: {
	q: "\(lib.run.stdout)"
	r: lib.run
}
"#,
            true,
        ),
        (
            "modulealiashidden",
            r#"
package foo
import "example.com/k/lib"
command: modulealiashidden: {
	r: lib.run
	_q: "\(lib.run.stdout)"
}
"#,
            true,
        ),
        (
            "modulealiastwo",
            r#"
package foo
import "example.com/k/lib"
command: modulealiastwo: {
	r1: lib.run
	r2: lib.run
	q: "\(lib.run.stdout)"
}
"#,
            true,
        ),
        (
            "modulealiasinline",
            r#"
package foo
import "example.com/k/lib"
command: modulealiasinline: {
	a: lib.run
	q: "\(lib.run.stdout)"
}
"#,
            true,
        ),
        (
            "modulealiaslet",
            r#"
package foo
import "example.com/k/lib"
let t = lib.run
command: modulealiaslet: {
	r: t
	q: "\(t.stdout)"
}
"#,
            true,
        ),
        (
            "modulealiasgroup",
            r#"
package foo
import "example.com/k/lib"
command: modulealiasgroup: {
	r: lib.group.run
	q: "\(lib.group.run.stdout)"
}
"#,
            true,
        ),
        (
            "modulealiasdef",
            r#"
package foo
import "example.com/k/lib"
command: modulealiasdef: {
	r: lib.#def
	q: "\(lib.#def.stdout)"
}
"#,
            true,
        ),
        (
            "modulealiasindex",
            r#"
package foo
import "example.com/k/lib"
command: modulealiasindex: {
	r: lib["run"]
	q: "\(lib.run.stdout)"
}
"#,
            true,
        ),
        (
            "modulealiasrootfield",
            r#"
package foo
import "example.com/k/lib"
prep: lib.run
command: modulealiasrootfield: {
	r: prep
	q: "\(prep.stdout)"
}
"#,
            true,
        ),
        (
            "modulealiasrootcontainer",
            r#"
package foo
import "example.com/k/lib"
prep: {inner: lib.run}
command: modulealiasrootcontainer: {
	r: prep.inner
	q: "\(prep.inner.stdout)"
}
"#,
            true,
        ),
        (
            "modulealiasrootlet",
            r#"
package foo
import "example.com/k/lib"
let l = lib.run
prep: l
command: modulealiasrootlet: {
	r: prep
	q: "\(prep.stdout)"
}
"#,
            true,
        ),
        (
            "modulealiasrootmixed",
            r#"
package foo
import "example.com/k/lib"
prep: lib.run
command: modulealiasrootmixed: {
	r: prep
	q: "\(r.stdout)-\(prep.stdout)"
}
"#,
            true,
        ),
        (
            "modulealiastaskinputplain",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: modulealiastaskinputplain: {
	r: lib.run
	t: cli.Print & {text: "R=\(lib.run.stdout)"}
	q: "\(lib.run.stdout)"
}
"#,
            false,
        ),
        (
            "modulealiastaskinputgroup",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: modulealiastaskinputgroup: {
	r: lib.group.run
	t: cli.Print & {text: "R=\(lib.group.run.stdout)"}
	q: "\(lib.group.run.stdout)"
}
"#,
            false,
        ),
        (
            "modulealiastaskinputdef",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: modulealiastaskinputdef: {
	r: lib.#def
	t: cli.Print & {text: "R=\(lib.#def.stdout)"}
	q: "\(lib.#def.stdout)"
}
"#,
            false,
        ),
        (
            "modulealiastaskinputindex",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: modulealiastaskinputindex: {
	r: lib["run"]
	t: cli.Print & {text: "R=\(lib.run.stdout)"}
	q: "\(lib.run.stdout)"
}
"#,
            false,
        ),
        (
            "modulealiasrootcontainertaskinput",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: {inner: lib.run}
command: modulealiasrootcontainertaskinput: {
	r: base
	t: cli.Print & {text: "R=\(r.inner.stdout)"}
}
"#,
            true,
        ),
        (
            "modulealiasdeepcontainertaskinput",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
base: {inner: {run: lib.run}}
command: modulealiasdeepcontainertaskinput: {
	r: base
	t: cli.Print & {text: "R=\(r.inner.run.stdout)"}
}
"#,
            true,
        ),
        (
            "moduleshadowfield",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
	"example.com/k/lib"
)
base: lib.run
command: moduleshadowfield: {
	base: exec.Run & {cmd: ["sh", "-c", "printf L"], stdout: string}
	t: cli.Print & {text: "R=\(base.stdout)"}
}
"#,
            false,
        ),
        (
            "moduleshadowcontainer",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
	"example.com/k/lib"
)
base: {inner: lib.run}
command: moduleshadowcontainer: {
	base: {inner: exec.Run & {cmd: ["sh", "-c", "printf L"], stdout: string}}
	t: cli.Print & {text: "R=\(base.inner.stdout)"}
}
"#,
            false,
        ),
        (
            "moduleshadownested",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
	"example.com/k/lib"
)
base: lib.run
command: moduleshadownested: {
	x: {
		base: exec.Run & {cmd: ["sh", "-c", "printf L"], stdout: string}
		t: cli.Print & {text: "R=\(base.stdout)"}
	}
}
"#,
            false,
        ),
        (
            "moduledefaliasroot",
            r#"
package foo
import "example.com/k/lib"
prep: lib.#def
command: moduledefaliasroot: {
	r: prep
	q: "\(prep.stdout)"
}
"#,
            true,
        ),
        (
            "moduledefaliasrootreadfirst",
            r#"
package foo
import "example.com/k/lib"
prep: lib.#def
command: moduledefaliasrootreadfirst: {
	q: "\(prep.stdout)"
	r: prep
}
"#,
            true,
        ),
        (
            "moduledefaliashidden",
            r#"
package foo
import "example.com/k/lib"
prep: lib.#def
command: moduledefaliashidden: {
	r: prep
	_q: "\(prep.stdout)"
}
"#,
            true,
        ),
        (
            "moduledefrootcontainer",
            r#"
package foo
import "example.com/k/lib"
prep: {inner: lib.#def}
command: moduledefrootcontainer: {
	r: prep.inner
	q: "\(prep.inner.stdout)"
}
"#,
            true,
        ),
        (
            "moduledefletchain",
            r#"
package foo
import "example.com/k/lib"
let l = lib.#def
prep: l
command: moduledefletchain: {
	r: prep
	q: "\(prep.stdout)"
}
"#,
            true,
        ),
        (
            "moduledefgroupalias",
            r#"
package foo
import "example.com/k/lib"
prep: lib.group.#inner
command: moduledefgroupalias: {
	r: prep
	q: "\(prep.stdout)"
}
"#,
            true,
        ),
        (
            "moduledefaliasoutput",
            r#"
package foo
import "example.com/k/lib"
prep: lib.#def
command: moduledefaliasoutput: {
	r: prep
	q: "\(r.stdout)-\(prep.stdout)"
}
"#,
            true,
        ),
        (
            "modulealiassuccess",
            r#"
package foo
import "example.com/k/lib"
command: modulealiassuccess: {
	r: lib.fail
	q: "\(lib.fail.success)"
}
"#,
            false,
        ),
        (
            "moduleplainnoalias",
            r#"
package foo
import (
	"tool/exec"
	"example.com/k/lib"
)
command: moduleplainnoalias: {
	a: exec.Run & {cmd: ["true"]}
	q: "\(lib.run.stdout)"
}
"#,
            false,
        ),
        (
            "modulealiastaskinputdecl",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: modulealiastaskinputdecl: {
	r: lib.run
	q: cli.Print & {text: "R=\(lib.run.stdout)"}
}
"#,
            false,
        ),
        (
            "modulealiastaskinputreadfirst",
            r#"
package foo
import (
	"tool/cli"
	"example.com/k/lib"
)
command: modulealiastaskinputreadfirst: {
	q: cli.Print & {text: "R=\(lib.run.stdout)"}
	r: lib.run
}
"#,
            false,
        ),
    ];
    for (name, source, expects_run) in cases {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        for directory in [cue_dir.path(), rust_dir.path()] {
            std::fs::create_dir_all(directory.join("cue.mod")).expect("cue.mod");
            std::fs::create_dir_all(directory.join("lib")).expect("lib");
            write(&directory.join("cue.mod/module.cue"), module);
            write(&directory.join("lib/lib.cue"), library);
            write(&directory.join("x_tool.cue"), source);
        }
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{name}: cue stderr: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        if expected.status.success() {
            assert_eq!(actual.stderr, expected.stderr, "{name}");
        }
        if expected.status.success() {
            assert_eq!(
                std::fs::read(cue_dir.path().join("order.txt")).ok(),
                std::fs::read(rust_dir.path().join("order.txt")).ok(),
                "{name}: order.txt differs"
            );
        }
        assert_eq!(
            rust_dir.path().join("order.txt").exists(),
            *expects_run,
            "{name}: expected run={expects_run}"
        );
    }
}

#[test]
fn validate_tolerates_unused_incomplete_module_fields() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let module = "module: \"example.com/k\"\nlanguage: version: \"v0.17.1\"\n";
    let library = r#"package lib

import (
	e "tool/exec"
	"tool/cli"
)

run: e.Run & {cmd: ["sh", "-c", "printf abc"], stdout: string}
p:   cli.Print & {text: "G=\(run.stdout)"}
val: {stdout: "concrete"}
"#;
    let workflow = r#"package foo
import "example.com/k/lib"
workflow: {
	name: "v=\(lib.val.stdout)"
	steps: {a: {kind: "exec", exec: {cmd: ["sh", "-c", "echo hello"]}}}
}
"#;
    let directory = tempfile::tempdir().expect("tempdir");
    let pkg = directory.path().join("pkg");
    std::fs::create_dir_all(pkg.join("cue.mod")).expect("cue.mod");
    std::fs::create_dir_all(pkg.join("lib")).expect("lib");
    write(&pkg.join("cue.mod/module.cue"), module);
    write(&pkg.join("lib/lib.cue"), library);
    write(&pkg.join("workflow.cue"), workflow);
    let expected = Command::new(&cue)
        .args(["export", "workflow.cue"])
        .current_dir(&pkg)
        .output()
        .expect("run cue export");
    assert!(expected.status.success(), "cue export must succeed");
    let actual = ost(&[
        "--db",
        directory.path().join("db.sqlite").to_str().expect("db"),
        "validate",
        pkg.join("workflow.cue").to_str().expect("workflow"),
    ]);
    assert!(
        actual.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&actual.stderr)
    );

    // A genuinely incomplete top-level interpolation still fails.
    write(
        &pkg.join("bad.cue"),
        "package foo\nworkflow: {name: \"\\(undefined_field)\"}\n",
    );
    let bad = ost(&[
        "--db",
        directory.path().join("db.sqlite").to_str().expect("db"),
        "validate",
        pkg.join("bad.cue").to_str().expect("bad"),
    ]);
    assert!(!bad.status.success());
}

#[test]
fn tool_command_selector_commands_match_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let cases = [
        (
            "selafter",
            r#"
package foo
import "tool/exec"
lib: {
	base: {
		a: exec.Run & {$after: z, cmd: ["sh", "-c", "cat data.txt"]}
		z: exec.Run & {cmd: ["sh", "-c", "printf fresh > data.txt"]}
	}
}
command: selafter: lib.base
"#,
            Some(("data.txt", "stale")),
            Some("data.txt"),
        ),
        (
            "seltwobases",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
	"tool/file"
)
lib: {
	b1: {
		run1: exec.Run & {cmd: ["sh", "-c", "printf 'AAA'"], stdout: string}
		p1: cli.Print & {text: "1=\(run1.stdout)"}
	}
	b2: {
		run2: exec.Run & {cmd: ["sh", "-c", "printf 'BBB'"], stdout: string}
		w2: file.Create & {filename: "out2.txt", contents: "2=\(run2.stdout)"}
	}
}
command: seltwobases: lib.b1 & lib.b2
"#,
            None,
            Some("out2.txt"),
        ),
        (
            "selletnooutput",
            r#"
package foo
import "tool/exec"
let group = {inner: {run: exec.Run & {cmd: ["sh", "-c", "printf 'X'"]}}}
command: selletnooutput: group.inner
"#,
            None,
            None,
        ),
        (
            "selletafter",
            r#"
package foo
import "tool/exec"
let group = {
	inner: {
		a: exec.Run & {$after: z, cmd: ["sh", "-c", "cat data.txt > result.txt"]}
		z: exec.Run & {cmd: ["sh", "-c", "printf fresh > data.txt"]}
	}
}
command: selletafter: group.inner
"#,
            Some(("data.txt", "stale")),
            Some("result.txt"),
        ),
        (
            "selconcrete",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let group = {inner: {
	a: producer: exec.Run & {cmd: ["true"], stdout: string}
	pipeline: {
		producer: {stdout: "concrete"}
		helper: producer.stdout
		deep: p2: cli.Print & {text: "S=\(helper)"}
	}
}}
command: selconcrete: group.inner
"#,
            None,
            None,
        ),
        (
            "selconcretesel",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let group = {inner: {
	a: producer: exec.Run & {cmd: ["true"], stdout: string}
	src: {producer: {stdout: "concrete"}}
	pipeline: {
		producer: src.producer
		helper: producer.stdout
		p: cli.Print & {text: "S=\(helper)"}
	}
}}
command: selconcretesel: group.inner
"#,
            None,
            None,
        ),
        (
            "selindexconcrete",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let group = {inner: {
	src: {
		run: exec.Run & {cmd: ["true"], stdout: string}
		producer: {stdout: "concrete"}
	}
	k: "producer"
	alias: src[k]
	p: cli.Print & {text: "S=\(alias.stdout)"}
}}
command: selindexconcrete: group.inner
"#,
            None,
            None,
        ),
        (
            "selconcreteorder",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let group = {inner: {
	pipeline: {
		producer: {stdout: "concrete"}
		helper:   producer.stdout
		deep: p2: cli.Print & {text: "S=\(helper)"}
	}
	a: producer: exec.Run & {cmd: ["true"], stdout: string}
}}
command: selconcreteorder: group.inner
"#,
            None,
            None,
        ),
        (
            "selconcretedefault",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let group = {inner: {
	pipeline: {
		producer: {stdout: *"concrete" | string}
		helper:   producer.stdout
		p:        cli.Print & {text: "S=\(helper)"}
	}
	a: producer: exec.Run & {cmd: ["sh", "-c", "printf 'real'"], stdout: string}
}}
command: selconcretedefault: group.inner
"#,
            None,
            None,
        ),
        (
            "selconcretenote",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
base: {p: "P"}
let group = {inner: {
	pipeline: {
		producer: {stdout: "concrete", note: base}
		helper:   producer.stdout
		p:        cli.Print & {text: "S=\(helper)"}
	}
	a: producer: exec.Run & {cmd: ["sh", "-c", "printf 'real'"], stdout: string}
}}
command: selconcretenote: group.inner
"#,
            None,
            None,
        ),
        (
            "selconcretenotedefault",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
base: {p: "P"}
let group = {inner: {
	pipeline: {
		producer: {stdout: *"concrete" | string, note: base}
		helper:   producer.stdout
		p:        cli.Print & {text: "S=\(helper)"}
	}
	a: producer: exec.Run & {cmd: ["sh", "-c", "printf 'real'"], stdout: string}
}}
command: selconcretenotedefault: group.inner
"#,
            None,
            None,
        ),
        (
            "selbuiltinmath",
            r#"
package foo
import (
	"math"
	"tool/cli"
	"tool/exec"
)
let group = {inner: {
	pipeline: {
		producer: math.Pi & number
		helper:   producer
		p:        cli.Print & {text: "S=\(helper)"}
	}
	a: producer: exec.Run & {cmd: ["sh", "-c", "printf 'real'"], stdout: string}
}}
command: selbuiltinmath: group.inner
"#,
            None,
            None,
        ),
        (
            "selbuiltintime",
            r#"
package foo
import (
	"time"
	"tool/cli"
	"tool/exec"
)
command: selbuiltintime: {
	pipeline: {
		producer: time.RFC3339 & string
		helper:   producer
		p:        cli.Print & {text: "S=\(helper)"}
	}
	a: producer: exec.Run & {cmd: ["sh", "-c", "printf 'real'"], stdout: string}
}
"#,
            None,
            None,
        ),
    ];
    for (name, source, seed, artifact) in cases {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        write(&cue_dir.path().join("fixture_tool.cue"), source);
        write(&rust_dir.path().join("fixture_tool.cue"), source);
        if let Some((file, contents)) = seed {
            write(&cue_dir.path().join(file), contents);
            write(&rust_dir.path().join(file), contents);
        }
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        assert_eq!(actual.status.code(), expected.status.code(), "{name}");
        assert_eq!(actual.stdout, expected.stdout, "{name}");
        assert_eq!(actual.stderr, expected.stderr, "{name}");
        if let Some(artifact) = artifact {
            assert_eq!(
                std::fs::read(cue_dir.path().join(artifact)).expect("cue artifact"),
                std::fs::read(rust_dir.path().join(artifact)).expect("rust artifact"),
                "{name}: file contents differ"
            );
        }
    }
}

#[test]
fn tool_command_let_selector_injection_is_an_ostinato_extension() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let fixtures = [
        (
            "letselector",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let group = {
	inner: {
		run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'; touch marker"], stdout: string}
		p: cli.Print & {text: "R=\(run.stdout)"}
	}
}
command: letselector: group.inner
"#,
        ),
        (
            "letancestor",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let group = {inner: {
	pipeline: {
		run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'; touch marker"], stdout: string}
		msg: run.stdout
		report: {
			p: cli.Print & {text: "R=\(msg)"}
		}
	}
}}
command: letancestor: group.inner
"#,
        ),
        (
            "letindexalias",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let group = {inner: {
	src: {run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'; touch marker"], stdout: string}}
	alias: src["run"]
	p: cli.Print & {text: "R=\(alias.stdout)"}
}}
command: letindexalias: group.inner
"#,
        ),
        (
            "letindexconst",
            r#"
package foo
import (
	"tool/cli"
	"tool/exec"
)
let group = {inner: {
	src: {run: exec.Run & {cmd: ["sh", "-c", "printf 'abc'; touch marker"], stdout: string}}
	k: "run"
	alias: src[k]
	p: cli.Print & {text: "R=\(alias.stdout)"}
}}
command: letindexconst: group.inner
"#,
        ),
    ];
    for (name, source) in fixtures {
        let cue_dir = tempfile::tempdir().expect("tempdir");
        let rust_dir = tempfile::tempdir().expect("tempdir");
        write(&cue_dir.path().join("fixture_tool.cue"), source);
        write(&rust_dir.path().join("fixture_tool.cue"), source);
        let expected = Command::new(&cue)
            .args(["cmd", name])
            .current_dir(cue_dir.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", name, rust_dir.path().to_str().expect("path")]);
        // CUE v0.17.1 rejects task outputs selected through a `let`; injecting
        // them is an intentional Ostinato extension.
        assert!(!expected.status.success(), "{name}: cue must reject");
        assert!(actual.status.success(), "{name}: extension must succeed");
        assert_eq!(actual.stdout, b"R=abc\n", "{name}: stdout differs");
        assert!(
            rust_dir.path().join("marker").exists(),
            "{name}: task did not execute"
        );
    }
}

#[test]
fn unused_imports_match_cue() {
    let Some(cue) = cue_command() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let fixtures = [
        ("unused", "package p\nimport \"strings\"\nx: 1\n", false),
        (
            "unused-alias",
            "package p\nimport s \"strings\"\nx: 1\n",
            false,
        ),
        (
            "used",
            "package p\nimport \"strings\"\nx: strings.ToUpper(\"a\")\n",
            true,
        ),
        (
            "used-alias",
            "package p\nimport s \"strings\"\nx: s.ToUpper(\"a\")\n",
            true,
        ),
        (
            "hidden-use",
            "package p\nimport \"strings\"\n_x: strings.ToUpper(\"a\")\ny: 1\n",
            true,
        ),
        (
            "definition-use",
            "package p\nimport \"strings\"\n#D: strings.ToUpper(\"a\")\ny: 1\n",
            true,
        ),
        (
            "interpolation-use",
            "package p\nimport \"strings\"\nx: \"\\(strings.ToUpper(\"a\"))\"\n",
            true,
        ),
        (
            "nested-shadow",
            "package p\nimport \"strings\"\nx: {strings: 1, y: strings}\n",
            false,
        ),
        (
            "for-shadow",
            "package p\nimport \"strings\"\nx: [for strings in [1] {strings}]\n",
            false,
        ),
        ("blank", "package p\nimport _ \"strings\"\nx: 1\n", false),
        (
            "params-alias",
            "package p\nimport params \"strings\"\nx: params.ToUpper(\"a\")\n",
            true,
        ),
        (
            "comp-second-for",
            "package p\nimport \"strings\"\nx: [for strings in [[1]] for b in strings {b}]\n",
            false,
        ),
        (
            "duplicate-import",
            "package p\nimport \"strings\"\nimport \"strings\"\nx: strings.ToUpper(\"a\")\n",
            false,
        ),
    ];
    for (name, body, used) in fixtures {
        let directory = tempfile::tempdir().expect("tempdir");
        write(
            &directory.path().join("a.cue"),
            &format!("{body}\nworkflow: {{name: \"{name}\", steps: {{s: {{kind: \"noop\"}}}}}}\n"),
        );
        let expected = Command::new(&cue)
            .arg("export")
            .current_dir(directory.path())
            .output()
            .expect("run cue export");
        let actual = ost(&[
            "--db",
            directory.path().join("db.sqlite").to_str().expect("db"),
            "--json",
            "validate",
            directory.path().to_str().expect("path"),
        ]);
        assert_eq!(
            actual.status.success(),
            used,
            "{name}: rust success mismatch; stderr: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(
            expected.status.success(),
            used,
            "{name}: cue success mismatch"
        );
    }

    let tool_cases = [
        (
            "tool-unused",
            "package p\nimport (\n\t\"tool/exec\"\n\t\"strings\"\n)\ncommand: c: {run: exec.Run & {cmd: [\"true\"]}}\n",
            false,
        ),
        (
            "tool-used",
            "package p\nimport (\n\t\"tool/exec\"\n\t\"strings\"\n)\ncommand: c: {run: exec.Run & {cmd: [\"true\"], env: X: strings.ToUpper(\"a\")}}\n",
            true,
        ),
    ];
    for (name, body, used) in tool_cases {
        let directory = tempfile::tempdir().expect("tempdir");
        write(&directory.path().join("a_tool.cue"), body);
        let expected = Command::new(&cue)
            .args(["cmd", "c"])
            .current_dir(directory.path())
            .output()
            .expect("run cue cmd");
        let actual = ost(&["cmd", "c", directory.path().to_str().expect("path")]);
        assert_eq!(
            actual.status.success(),
            used,
            "{name}: rust success mismatch; stderr: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(
            expected.status.success(),
            used,
            "{name}: cue success mismatch"
        );
        assert_eq!(actual.stdout, expected.stdout, "{name}: stdout differs");
    }
}
