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

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by_key(|(key, _)| key.as_str());
            let mut out = serde_json::Map::new();
            for (key, value) in entries {
                out.insert(key.clone(), canonical_json(value));
            }
            Value::Object(out)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        other => other.clone(),
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
        let stem = path.file_stem().and_then(|stem| stem.to_str());
        let params: &[&str] = match stem {
            Some("params") => &["name=Alice"],
            Some("params-nested-tag") => &["host=db.example", "port=5432", "name=svc"],
            Some("params-nested-tag-def") => &["host=x", "dbhost=a", "qhost=b", "short=c"],
            Some("params-nested-tag-inner") => &["host=x"],
            Some("params-nested-tag-origin") => &["host=x", "ihost=y", "lhost=z", "ehost=w"],
            Some("params-nested-tag-identity") => &["dhost=x", "phost=y", "ghost=z"],
            Some("params-nested-tag-selector") => &["leaf=Z", "h=H", "p=9", "n=5"],
            Some("params-nested-tag-scope") => &["h=x", "b=2", "n=5"],
            Some("params-nested-tag-quoted") => &["host=x", "port=9"],
            Some("params-nested-tag-depth") => &["n=5"],
            Some("params-nested-tag-expr") => &["host=x", "port=7", "b=8", "unused=9"],
            Some("params-nested-tag-alias-scope") => &["glob=5", "loc=6", "n=7"],
            _ => &[],
        };
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
    compare(&cue, &root.join("alias").join("b.cue"), &[]);
    compare(&cue, &root.join("versioned").join("main.cue"), &[]);
    compare(&cue, &root.join("module").join("complex.cue"), &[]);
}

#[test]
fn rust_and_official_cue_reject_invalid_corpus() {
    let Some(cue) = cue_bin().expect("explicit CUE_BIN must be usable") else {
        eprintln!("skipping: official CUE >= 0.17.1 not installed and CUE_BIN is unset");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("conformance")
        .join("invalid");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("read invalid cases")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "cue"))
        .collect();
    entries.sort();
    assert!(!entries.is_empty());
    let scope = tempfile::tempdir().expect("tempdir");
    for (index, path) in entries.iter().enumerate() {
        let rust_db = scope.path().join(format!("rust-{index}.sqlite"));
        let rust = Command::new(bin())
            .args([
                "--db",
                rust_db.to_str().expect("db"),
                "--evaluator",
                "rust",
                "--json",
                "validate",
                path.to_str().expect("path"),
            ])
            .output()
            .expect("run rust validate");
        assert!(
            !rust.status.success(),
            "rust accepted invalid case {}: {}",
            path.display(),
            String::from_utf8_lossy(&rust.stdout)
        );
        let cue_db = scope.path().join(format!("cue-{index}.sqlite"));
        let oracle = Command::new(bin())
            .args([
                "--db",
                cue_db.to_str().expect("db"),
                "--evaluator",
                "cue",
                "--cue",
                &cue,
                "--json",
                "validate",
                path.to_str().expect("path"),
            ])
            .output()
            .expect("run cue validate");
        assert!(
            !oracle.status.success(),
            "cue accepted invalid case {}: {}",
            path.display(),
            String::from_utf8_lossy(&oracle.stdout)
        );
    }
}

#[test]
fn rust_export_omits_incomplete_definitions_and_matches_cue() {
    let Some(cue) = cue_bin().expect("explicit CUE_BIN must be usable") else {
        eprintln!("skipping: official CUE >= 0.17.1 not installed and CUE_BIN is unset");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("conformance")
        .join("cases");
    let cases = [
        "optional-compound-template.cue",
        "optional-compound-params.cue",
        "optional-nested-default-params.cue",
        "optional-nested-definition-params.cue",
    ];
    let export = |path: &Path| {
        let rust = Command::new(bin())
            .args(["export", path.to_str().expect("path"), "--out", "json"])
            .output()
            .expect("run rust export");
        assert!(
            rust.status.success(),
            "rust export failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&rust.stderr)
        );
        let oracle = Command::new(&cue)
            .args(["export", path.to_str().expect("path"), "--out", "json"])
            .output()
            .expect("run cue export");
        assert!(
            oracle.status.success(),
            "cue export failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&oracle.stderr)
        );
        let rust_json: Value = serde_json::from_slice(&rust.stdout).expect("rust json");
        let cue_json: Value = serde_json::from_slice(&oracle.stdout).expect("cue json");
        assert_eq!(
            rust_json,
            cue_json,
            "export mismatch for {}",
            path.display()
        );
        assert_eq!(
            serde_json::to_string(&canonical_json(&rust_json)).expect("canonical rust json"),
            serde_json::to_string(&canonical_json(&cue_json)).expect("canonical cue json"),
            "canonical export mismatch for {}",
            path.display()
        );
        let text = String::from_utf8_lossy(&rust.stdout);
        assert!(
            !text.contains("\"#"),
            "export must not emit definitions: {text}"
        );
    };
    for case in cases {
        export(&root.join(case));
    }

    let scope = tempfile::tempdir().expect("tempdir");
    let minimal = scope.path().join("minimal.cue");
    std::fs::write(&minimal, "package demo\n#P: {a?: int, v: a}\nx: 1\n").expect("write minimal");
    export(&minimal);

    let extras = [
        (
            "soft-definition",
            "package demo\n#D: {a: int, b: a + 1}\nx: 1\n",
        ),
        (
            "soft-hidden",
            "package demo\n_h: {a: int, b: a + 1}\nx: 1\n",
        ),
        (
            "non-params-narrowing",
            "package demo\n#E: {n: *2 | int, m: n * 3}\n#P: {ls?: [#E]}\nx: #P & {ls: [{n: 4}]}\n",
        ),
    ];
    for (name, source) in extras {
        let directory = scope.path().join(name);
        std::fs::create_dir_all(&directory).expect("create dir");
        let path = directory.join(format!("{name}.cue"));
        std::fs::write(&path, format!("{source}\n")).expect("write extra");
        export(&path);
    }
}

#[test]
fn rust_json_yaml_inputs_and_import_match_official_cue() {
    let Some(cue) = cue_bin().expect("explicit CUE_BIN must be usable") else {
        eprintln!("skipping: official CUE >= 0.17.1 not installed and CUE_BIN is unset");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("conformance")
        .join("data");
    let base = root.join("base.cue");
    for data in ["data.json", "data.yaml"] {
        let data_path = root.join(data);
        let rust = Command::new(bin())
            .args([
                "--evaluator",
                "rust",
                "export",
                base.to_str().expect("base"),
                "--input",
                data_path.to_str().expect("data"),
                "--out",
                "json",
            ])
            .output()
            .expect("run rust export");
        assert!(
            rust.status.success(),
            "rust export failed for {data}: {}",
            String::from_utf8_lossy(&rust.stderr)
        );
        let oracle = Command::new(&cue)
            .current_dir(&root)
            .args(["export", "base.cue", data, "--out", "json"])
            .output()
            .expect("run cue export");
        assert!(
            oracle.status.success(),
            "cue export failed for {data}: {}",
            String::from_utf8_lossy(&oracle.stderr)
        );
        let rust: Value = serde_json::from_slice(&rust.stdout).expect("rust json");
        let oracle: Value = serde_json::from_slice(&oracle.stdout).expect("cue json");
        assert_eq!(rust, oracle, "data input mismatch for {data}");
    }

    let scope = tempfile::tempdir().expect("tempdir");
    for data in ["data.json", "data.yaml"] {
        let data_path = root.join(data);
        let imported = scope.path().join(format!("{data}.cue"));
        let output = Command::new(bin())
            .args([
                "import",
                data_path.to_str().expect("data"),
                "-o",
                imported.to_str().expect("imported"),
                "--force",
            ])
            .output()
            .expect("run import");
        assert!(
            output.status.success(),
            "import failed for {data}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let imported_value = Command::new(&cue)
            .args([
                "export",
                imported.to_str().expect("imported"),
                "--out",
                "json",
            ])
            .output()
            .expect("cue export imported");
        assert!(
            imported_value.status.success(),
            "cue export imported failed: {}",
            String::from_utf8_lossy(&imported_value.stderr)
        );
        let direct = Command::new(&cue)
            .current_dir(&root)
            .args(["export", data, "--out", "json"])
            .output()
            .expect("cue export data");
        assert!(direct.status.success());
        let imported_value: Value =
            serde_json::from_slice(&imported_value.stdout).expect("imported json");
        let direct: Value = serde_json::from_slice(&direct.stdout).expect("direct json");
        assert_eq!(imported_value, direct, "import mismatch for {data}");
    }
}
