#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ostinato"))
}

fn cue_version(text: &str) -> Option<(u32, u32, u32)> {
    let token = text
        .split_whitespace()
        .find(|token| token.starts_with('v') && token.contains('.'))?;
    let core = token.trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.');
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ))
}

fn cue_bin() -> Result<Option<String>, String> {
    let explicit = std::env::var("CUE_BIN").ok();
    let cue = explicit.clone().unwrap_or_else(|| "cue".to_owned());
    let output = match Command::new(&cue).arg("version").output() {
        Ok(output) => output,
        Err(err) if explicit.is_some() => {
            return Err(format!("CUE_BIN `{cue}` is not executable: {err}"));
        }
        Err(_) => return Ok(None),
    };
    if !output.status.success() {
        if explicit.is_some() {
            return Err(format!("`{cue} version` failed"));
        }
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let Some(version) = cue_version(&text) else {
        if explicit.is_some() {
            return Err(format!("cannot parse CUE version from `{text}`"));
        }
        return Ok(None);
    };
    if version < (0, 17, 1) {
        if explicit.is_some() {
            return Err(format!(
                "CUE {version:?} is too old; the oracle requires >= v0.17.1"
            ));
        }
        return Ok(None);
    }
    Ok(Some(cue))
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
    let Some(cue) = cue_bin().expect("explicit CUE_BIN must be usable") else {
        eprintln!("skipping: official CUE >= 0.17.1 not installed and CUE_BIN is unset");
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
    compare(&cue, &root.join("nestedmodule").join("main.cue"), &[]);
    compare(&cue, &root.join("versioned").join("main.cue"), &[]);
    compare(&cue, &root.join("module").join("complex.cue"), &[]);
}
