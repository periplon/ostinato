#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ostinato"))
}

fn cue_bin() -> Option<String> {
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

fn run_cue(cue: &str, directory: &Path, args: &[&str]) -> Output {
    Command::new(cue)
        .current_dir(directory)
        .args(args)
        .output()
        .expect("run cue")
}

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write fixture");
}

fn cue_export_json(cue: &str, directory: &Path, path: &Path) -> Value {
    let output = run_cue(
        cue,
        directory,
        &["export", path.to_str().expect("path"), "--out", "json"],
    );
    assert!(
        output.status.success(),
        "cue export failed for {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("cue json")
}

fn ost_export_json(path: &Path) -> Value {
    let output = ost(&["export", path.to_str().expect("path"), "--out", "json"]);
    assert!(
        output.status.success(),
        "ostinato export failed for {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("ostinato json")
}

const DATA_JSON: &str = r#"{
  "name": "alice",
  "age": 30,
  "active": true,
  "score": 1.5,
  "tags": ["a", "b"],
  "nested": {"x": 1, "y": false},
  "big": 9007199254740993,
  "nothing": null
}
"#;

const DATA_YAML: &str = r#"name: bob
age: 41
active: false
score: 2.25
tags:
  - x
  - y
nested:
  x: 2
  y: true
big: 9007199254740993
nothing: null
quoted: "123"
newline: "a\nb"
"#;

#[test]
fn import_matches_cue_import_for_json_and_yaml() {
    let Some(cue_bin) = cue_bin() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    for (name, contents) in [("data.json", DATA_JSON), ("data.yaml", DATA_YAML)] {
        let input = directory.path().join(name);
        write(&input, contents);
        let ours = directory.path().join(format!("ours-{name}.cue"));
        let theirs = directory.path().join(format!("cue-{name}.cue"));
        let output = ost(&[
            "import",
            input.to_str().expect("input"),
            "-o",
            ours.to_str().expect("ours"),
            "--package",
            "demo",
            "--force",
        ]);
        assert!(
            output.status.success(),
            "import failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = run_cue(
            &cue_bin,
            directory.path(),
            &[
                "import",
                input.to_str().expect("input"),
                "-o",
                theirs.to_str().expect("theirs"),
                "-p",
                "demo",
                "-f",
            ],
        );
        assert!(
            output.status.success(),
            "cue import failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            cue_export_json(&cue_bin, directory.path(), &ours),
            cue_export_json(&cue_bin, directory.path(), &theirs),
            "import mismatch for {name}"
        );
    }
}

#[test]
fn import_list_multi_document_yaml_matches_cue() {
    let Some(cue_bin) = cue_bin() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let input = directory.path().join("multi.yaml");
    write(
        &input,
        "kind: Service\nname: booster\n---\nkind: Deployment\nname: booster\nreplicas: 1\n",
    );
    let ours = directory.path().join("ours.cue");
    let theirs = directory.path().join("theirs.cue");
    let output = ost(&[
        "import",
        input.to_str().expect("input"),
        "-o",
        ours.to_str().expect("ours"),
        "--list",
        "--force",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = run_cue(
        &cue_bin,
        directory.path(),
        &[
            "import",
            input.to_str().expect("input"),
            "-o",
            theirs.to_str().expect("theirs"),
            "--list",
            "-f",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        cue_export_json(&cue_bin, directory.path(), &ours),
        cue_export_json(&cue_bin, directory.path(), &theirs)
    );
}

#[test]
fn import_multiple_inputs_into_one_file_is_cue_compatible() {
    let Some(cue_bin) = cue_bin() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    write(&directory.path().join("a.json"), r#"{"a":1}"#);
    write(&directory.path().join("b.yaml"), "b: 2\n");
    let ours = directory.path().join("merged.cue");
    let output = ost(&[
        "import",
        directory.path().join("a.json").to_str().expect("a"),
        directory.path().join("b.yaml").to_str().expect("b"),
        "-o",
        ours.to_str().expect("ours"),
        "--merge",
        "--force",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        cue_export_json(&cue_bin, directory.path(), &ours),
        serde_json::json!({"a": 1, "b": 2})
    );
}

#[test]
fn import_rejects_invalid_input_without_partial_output() {
    let directory = tempfile::tempdir().expect("tempdir");
    let output_path = directory.path().join("bad.cue");
    for (name, contents) in [
        ("dup.json", r#"{"a":1,"a":2}"#),
        ("bad.json", "{not json"),
        ("bad.yaml", "a: [1,\n"),
        ("tag.yaml", "a: !custom 1\n"),
    ] {
        let input = directory.path().join(name);
        write(&input, contents);
        let _ = std::fs::remove_file(&output_path);
        let output = ost(&[
            "import",
            input.to_str().expect("input"),
            "-o",
            output_path.to_str().expect("output"),
            "--force",
        ]);
        assert!(!output.status.success(), "accepted invalid {name}");
        assert!(
            !output_path.exists(),
            "wrote partial output for invalid {name}"
        );
    }
}

#[test]
fn export_json_and_yaml_match_cue_values() {
    let Some(cue_bin) = cue_bin() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let source = directory.path().join("base.cue");
    write(
        &source,
        r#"package demo

_hidden: 99
obj: {a: 1, b: 2, _h: 3, opt?: 4}
list: [3, 1, 2]
big: 9007199254740993
s: "hi"
nil: null
opt?: string
"#,
    );
    let expected = cue_export_json(&cue_bin, directory.path(), &source);
    assert_eq!(ost_export_json(&source), expected);

    let yaml = directory.path().join("out.yaml");
    let output = ost(&[
        "export",
        source.to_str().expect("source"),
        "--out",
        "yaml",
        "-o",
        yaml.to_str().expect("yaml"),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(cue_export_json(&cue_bin, directory.path(), &yaml), expected);

    for (expression, expected) in [
        ("obj.a", serde_json::json!(1)),
        ("list[1]", serde_json::json!(1)),
    ] {
        let output = ost(&[
            "export",
            source.to_str().expect("source"),
            "-e",
            expression,
            "--out",
            "json",
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).expect("json");
        assert_eq!(value, expected, "expression {expression}");
    }

    let output = ost(&["export", source.to_str().expect("source"), "-e", "_hidden"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "99");

    let output = ost(&["export", source.to_str().expect("source"), "-e", "opt"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("optional field"));
}

#[test]
fn export_data_files_and_inputs_match_cue() {
    let Some(cue_bin) = cue_bin() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let data = directory.path().join("data.yaml");
    write(&data, "a: 1\nb: [1, 2]\n");
    let other = directory.path().join("other.json");
    write(&other, r#"{"c":3}"#);
    assert_eq!(
        ost_export_json(&data),
        cue_export_json(&cue_bin, directory.path(), &data)
    );
    let output = run_cue(
        &cue_bin,
        directory.path(),
        &[
            "export",
            data.to_str().expect("data"),
            other.to_str().expect("other"),
            "--out",
            "json",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: Value = serde_json::from_slice(&output.stdout).expect("json");
    let output = ost(&[
        "export",
        data.to_str().expect("data"),
        "--input",
        other.to_str().expect("other"),
        "--out",
        "json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(actual, expected);
}

#[test]
fn import_single_output_skips_extra_inputs_like_cue_but_is_not_silent() {
    let directory = tempfile::tempdir().expect("tempdir");
    write(&directory.path().join("a.json"), r#"{"a":1}"#);
    write(&directory.path().join("b.yaml"), "b: 2\n");
    let output_path = directory.path().join("out.cue");
    let input_a = directory.path().join("a.json");
    let input_b = directory.path().join("b.yaml");
    let args = [
        "import",
        input_a.to_str().expect("a"),
        input_b.to_str().expect("b"),
        "-o",
        output_path.to_str().expect("out"),
    ];
    let output = ost(&args);
    assert_eq!(output.status.code(), Some(1));
    assert!(output_path.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("skipping"));
    let _ = std::fs::remove_file(&output_path);
    let mut compat = args.to_vec();
    compat.push("--skip-existing");
    let output = ost(&compat);
    assert!(output.status.success());
    assert!(output_path.exists());
}

#[test]
fn import_fails_on_output_collisions_and_refuses_destructive_outputs() {
    let directory = tempfile::tempdir().expect("tempdir");
    write(&directory.path().join("x.json"), r#"{"a":1}"#);
    write(&directory.path().join("x.yaml"), "b: 2\n");
    let output = ost(&[
        "import",
        directory.path().join("x.json").to_str().expect("x"),
        directory.path().join("x.yaml").to_str().expect("x"),
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("same output"));

    let source = directory.path().join("a.json");
    write(&source, r#"{"a":1}"#);
    let output = ost(&[
        "import",
        source.to_str().expect("source"),
        "-o",
        source.to_str().expect("source"),
        "--force",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("one of the inputs"));
}

#[test]
fn import_split_files_matches_cue_names() {
    let Some(cue_bin) = cue_bin() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let input = directory.path().join("multi.yaml");
    write(&input, "kind: Service\n---\nkind: Deployment\n");
    let output = ost(&["import", input.to_str().expect("input"), "--files"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let first = directory.path().join("multi.cue");
    let second = directory.path().join("multi-1.cue");
    assert!(first.exists());
    assert!(second.exists());
    assert_eq!(
        cue_export_json(&cue_bin, directory.path(), &first),
        serde_json::json!({"kind": "Service"})
    );
    assert_eq!(
        cue_export_json(&cue_bin, directory.path(), &second),
        serde_json::json!({"kind": "Deployment"})
    );
}

#[test]
fn import_respects_no_clobber_and_force() {
    let directory = tempfile::tempdir().expect("tempdir");
    let input = directory.path().join("a.json");
    write(&input, r#"{"a":1}"#);
    let output = directory.path().join("a.cue");
    write(&output, "old");
    let args = ["import", input.to_str().expect("input")];
    let result = ost(&args);
    assert!(!result.status.success());
    assert_eq!(std::fs::read_to_string(&output).expect("old"), "old");
    let mut forced = args.to_vec();
    forced.push("--force");
    let result = ost(&forced);
    assert!(result.status.success());
    assert!(
        std::fs::read_to_string(&output)
            .expect("new")
            .contains("a: 1")
    );
}

#[test]
fn export_repeated_expressions_stream_and_stdout_is_pure() {
    let directory = tempfile::tempdir().expect("tempdir");
    let data = directory.path().join("data.json");
    write(&data, r#"{"a":1,"b":2}"#);
    let output = ost(&[
        "export",
        data.to_str().expect("data"),
        "-e",
        "a",
        "-e",
        "b",
        "--out",
        "json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n2\n");
    assert!(output.stderr.is_empty());

    let output = ost(&[
        "export",
        data.to_str().expect("data"),
        "-e",
        "a",
        "-e",
        "b",
        "--out",
        "yaml",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n---\n2\n");
}

#[test]
fn export_refuses_no_clobber_symlinks_hardlinks_and_input_aliasing() {
    let directory = tempfile::tempdir().expect("tempdir");
    let data = directory.path().join("data.json");
    write(&data, r#"{"a":1}"#);
    let output = directory.path().join("out.json");
    write(&output, "old");
    let result = ost(&[
        "export",
        data.to_str().expect("data"),
        "-o",
        output.to_str().expect("out"),
    ]);
    assert!(!result.status.success());
    assert_eq!(std::fs::read_to_string(&output).expect("old"), "old");
    let result = ost(&[
        "export",
        data.to_str().expect("data"),
        "-o",
        output.to_str().expect("out"),
        "--force",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );

    let alias = ost(&[
        "export",
        data.to_str().expect("data"),
        "-o",
        data.to_str().expect("data"),
        "--force",
    ]);
    assert!(!alias.status.success());
    assert!(String::from_utf8_lossy(&alias.stderr).contains("one of the inputs"));

    #[cfg(unix)]
    {
        let link = directory.path().join("link.json");
        std::os::unix::fs::symlink(&output, &link).expect("symlink");
        let result = ost(&[
            "export",
            data.to_str().expect("data"),
            "-o",
            link.to_str().expect("link"),
            "--force",
        ]);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("symbolic link"));

        let hard = directory.path().join("hard.json");
        std::fs::hard_link(&output, &hard).expect("hardlink");
        let result = ost(&[
            "export",
            data.to_str().expect("data"),
            "-o",
            hard.to_str().expect("hard"),
            "--force",
        ]);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("hard-linked"));
    }
}

#[test]
fn export_preserves_source_order_and_multiple_cue_sources() {
    let directory = tempfile::tempdir().expect("tempdir");
    let data = directory.path().join("data.json");
    write(&data, r#"{"z":1,"a":2,"m":{"y":3,"b":4}}"#);
    let output = ost(&["export", data.to_str().expect("data"), "--out", "json"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    let z = text.find("\"z\"").expect("z");
    let a = text.find("\"a\"").expect("a");
    let m = text.find("\"m\"").expect("m");
    assert!(z < a && a < m, "{text}");

    let first = directory.path().join("first.cue");
    let second = directory.path().join("second.cue");
    write(&first, "a: 1\n");
    write(&second, "b: 2\n");
    let output = ost(&[
        "export",
        first.to_str().expect("first"),
        second.to_str().expect("second"),
        "--out",
        "json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value, serde_json::json!({"a": 1, "b": 2}));
}

#[test]
fn import_scalars_arrays_and_anchors_match_cue() {
    let Some(cue_bin) = cue_bin() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let cases = [
        ("scalar.json", "5\n"),
        ("array.json", "[1,{\"a\":2}]\n"),
        (
            "anchors.yaml",
            "base: &b\n  x: 1\n  y: 2\nchild:\n  <<: *b\n  y: 3\n",
        ),
    ];
    for (name, contents) in cases {
        let input = directory.path().join(name);
        write(&input, contents);
        let ours = directory.path().join(format!("ours-{name}.cue"));
        let theirs = directory.path().join(format!("theirs-{name}.cue"));
        let output = ost(&[
            "import",
            input.to_str().expect("input"),
            "-o",
            ours.to_str().expect("ours"),
            "--force",
        ]);
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = run_cue(
            &cue_bin,
            directory.path(),
            &[
                "import",
                input.to_str().expect("input"),
                "-o",
                theirs.to_str().expect("theirs"),
                "-f",
            ],
        );
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            cue_export_json(&cue_bin, directory.path(), &ours),
            cue_export_json(&cue_bin, directory.path(), &theirs),
            "{name}"
        );
    }
}

#[test]
fn export_reports_data_conflicts_and_array_struct_mismatches() {
    let directory = tempfile::tempdir().expect("tempdir");
    let first = directory.path().join("one.yaml");
    let second = directory.path().join("two.json");
    write(&first, "a: 1\n");
    write(&second, r#"{"a":2}"#);
    let output = ost(&[
        "export",
        first.to_str().expect("first"),
        "--input",
        second.to_str().expect("second"),
        "--out",
        "json",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflicting"));

    let workflow = directory.path().join("wf.cue");
    write(&workflow, "package demo\nroot: string\n");
    let list = directory.path().join("list.json");
    write(&list, "[1,2]");
    let output = ost(&[
        "export",
        workflow.to_str().expect("workflow"),
        "--input",
        list.to_str().expect("list"),
        "--out",
        "json",
    ]);
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(
        message.contains("must be an object") || message.contains("conflicting"),
        "{message}"
    );
}

#[test]
fn export_out_flag_beats_extension_and_dash_is_stdout() {
    let directory = tempfile::tempdir().expect("tempdir");
    let data = directory.path().join("data.json");
    write(&data, r#"{"a":1}"#);
    let output_path = directory.path().join("out.json");
    let output = ost(&[
        "export",
        data.to_str().expect("data"),
        "-o",
        output_path.to_str().expect("out"),
        "--out",
        "yaml",
        "--force",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(&output_path).expect("yaml");
    assert!(text.starts_with("a: 1"), "{text}");

    let output = ost(&[
        "export",
        data.to_str().expect("data"),
        "-o",
        "-",
        "--out",
        "json",
    ]);
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value, serde_json::json!({"a": 1}));
}

const WORKFLOW: &str = r#"package demo

params: {
    name: string
    count: int
}

workflow: {
    name: "wf-\(params.name)"
    maxParallel: 1
    steps: {
        hello: {
            kind: "exec"
            exec: {cmd: ["sh", "-c", "echo hello \(params.name) \(params.count)"]}
        }
    }
}
"#;

#[test]
fn run_feeds_yaml_inputs_and_emits_yaml_results() {
    let Some(cue_bin) = cue_bin() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let workflow = directory.path().join("wf.cue");
    let data = directory.path().join("data.yaml");
    write(&workflow, WORKFLOW);
    write(&data, "params:\n  name: alice\n  count: 3\n");
    let db = directory.path().join("db.sqlite");

    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "--json",
        "run",
        workflow.to_str().expect("workflow"),
        "--input",
        data.to_str().expect("data"),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(report["status"], "succeeded");
    assert_eq!(report["steps"][0]["output"]["stdout"], "hello alice 3\n");

    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "run",
        workflow.to_str().expect("workflow"),
        "--input",
        data.to_str().expect("data"),
        "--out",
        "yaml",
        "-o",
        directory
            .path()
            .join("report.yaml")
            .to_str()
            .expect("report"),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report_yaml = directory.path().join("report.yaml");
    let report = cue_export_json(&cue_bin, directory.path(), &report_yaml);
    assert_eq!(report["status"], "succeeded");
    assert_eq!(report["steps"][0]["output"]["stdout"], "hello alice 3\n");

    let output = ost(&[
        "--db",
        db.to_str().expect("db"),
        "run",
        workflow.to_str().expect("workflow"),
        "--input",
        data.to_str().expect("data"),
        "--dry-run",
        "--out",
        "yaml",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dry = directory.path().join("dry.yaml");
    write(&dry, &String::from_utf8_lossy(&output.stdout));
    let dry = cue_export_json(&cue_bin, directory.path(), &dry);
    assert_eq!(dry["params"]["name"], "alice");
    assert_eq!(dry["workflow"]["name"], "wf-alice");
}

#[test]
fn param_file_flag_is_rejected_and_inputs_feed_params() {
    let Some(cue_bin) = cue_bin() else {
        eprintln!("skipping: official cue not installed");
        return;
    };
    let directory = tempfile::tempdir().expect("tempdir");
    let workflow = directory.path().join("wf.cue");
    write(&workflow, WORKFLOW);
    let params = directory.path().join("params.yaml");
    write(&params, "params:\n  name: carol\n  count: 9\n");
    let db = directory.path().join("db.sqlite");

    let parameter = format!("@{}", params.display());
    let rejected = ost(&[
        "--db",
        db.to_str().expect("db"),
        "validate",
        workflow.to_str().expect("workflow"),
        "-p",
        &parameter,
    ]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("`-p @file` is not supported"));

    for evaluator in ["rust", "cue"] {
        let output = ost(&[
            "--db",
            db.to_str().expect("db"),
            "--evaluator",
            evaluator,
            "--cue",
            &cue_bin,
            "--json",
            "validate",
            workflow.to_str().expect("workflow"),
            "--input",
            params.to_str().expect("params"),
        ]);
        assert!(
            output.status.success(),
            "{evaluator}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).expect("json");
        assert_eq!(report["params"]["name"], "carol", "evaluator {evaluator}");
    }
}
