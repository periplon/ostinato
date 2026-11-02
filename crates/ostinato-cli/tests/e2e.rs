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
fn cancel_marks_run_and_blocks_resume() {
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
        a: { kind: "exec", exec: { cmd: ["sh", "-c", "exit 9"] } }
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
    let cancelled = ost(&["--db", db.to_str().expect("db"), "cancel", &run]);
    assert!(
        cancelled.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&cancelled.stderr)
    );
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
}
