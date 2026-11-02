#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ostinato"))
}

fn cue_bin() -> Option<String> {
    let cue = std::env::var("CUE_BIN").unwrap_or_else(|_| "cue".to_owned());
    let output = Command::new(&cue).arg("version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout);
    if version.contains("v0.17") || version.contains("v0.18") || version.contains("v0.19") {
        Some(cue)
    } else {
        None
    }
}

fn rust_export(path: &Path, params: &[&str]) -> Value {
    let mut arguments = vec![
        "--db",
        "/tmp/ostinato-conformance.sqlite",
        "--evaluator",
        "rust",
        "--json",
        "validate",
        path.to_str().expect("path"),
    ];
    for param in params {
        arguments.push("-p");
        arguments.push(param);
    }
    let output = Command::new(bin())
        .args(&arguments)
        .output()
        .expect("run ostinato");
    assert!(
        output.status.success(),
        "rust validate failed for {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("rust json")
}

fn cue_export(cue: &str, path: &Path, params: &[&str]) -> Value {
    let mut arguments = vec![
        "--db",
        "/tmp/ostinato-conformance-oracle.sqlite",
        "--evaluator",
        "cue",
        "--cue",
        cue,
        "--json",
        "validate",
        path.to_str().expect("path"),
    ];
    for param in params {
        arguments.push("-p");
        arguments.push(param);
    }
    let output = Command::new(bin())
        .args(&arguments)
        .output()
        .expect("run cue oracle");
    assert!(
        output.status.success(),
        "cue oracle failed for {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("cue json")
}

fn compare(cue: &str, path: &Path, params: &[&str]) {
    let rust = rust_export(path, params);
    let oracle = cue_export(cue, path, params);
    assert_eq!(
        rust["workflow"],
        oracle["workflow"],
        "workflow mismatch for {}",
        path.display()
    );
    assert_eq!(
        rust["params"],
        oracle
            .get("params")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
        "params mismatch for {}",
        path.display()
    );
}

#[test]
fn rust_matches_official_cue_on_the_conformance_corpus() {
    let Some(cue) = cue_bin() else {
        eprintln!("skipping: official CUE >= 0.17 not installed");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("conformance");
    let cases = root.join("cases");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&cases)
        .expect("read cases")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "cue"))
        .collect();
    entries.sort();
    assert!(!entries.is_empty());
    for path in entries {
        let is_params = path.file_stem().is_some_and(|stem| stem == "params");
        let params: &[&str] = if is_params { &["name=Alice"] } else { &[] };
        compare(&cue, &path, params);
    }
    compare(&cue, &root.join("scope").join("a.cue"), &[]);
    compare(
        &cue,
        &root.join("splitparams").join("a.cue"),
        &["name=Alice"],
    );
    compare(&cue, &root.join("hidden").join("a.cue"), &[]);
    compare(&cue, &root.join("module"), &[]);
    compare(&cue, &root.join("versioned").join("main.cue"), &[]);
    compare(&cue, &root.join("module").join("complex.cue"), &[]);
}
