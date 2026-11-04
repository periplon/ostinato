#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

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

fn copy_dir(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).expect("create destination");
    for entry in std::fs::read_dir(source).expect("read case directory") {
        let entry = entry.expect("entry");
        let path = entry.path();
        let target = destination.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir(&path, &target);
        } else {
            std::fs::copy(&path, target).expect("copy case file");
        }
    }
}

#[test]
fn tool_commands_match_cue_on_the_conformance_corpus() {
    let Some(cue) = cue_bin().expect("explicit CUE_BIN must be usable") else {
        eprintln!("skipping: official CUE >= 0.17.1 not installed and CUE_BIN is unset");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("conformance")
        .join("tool_cases");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("read tool cases")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    entries.sort();
    assert!(!entries.is_empty());

    for case_dir in entries {
        let name = case_dir
            .file_name()
            .and_then(|name| name.to_str())
            .expect("case name");
        let command = std::fs::read_to_string(case_dir.join("command"))
            .expect("read command")
            .trim()
            .to_owned();
        let scratch = tempfile::tempdir().expect("tempdir");
        let cue_dir = scratch.path().join("cue");
        let rust_dir = scratch.path().join("rust");
        copy_dir(&case_dir, &cue_dir);
        copy_dir(&case_dir, &rust_dir);

        let expected = Command::new(&cue)
            .args(["cmd", &command])
            .current_dir(&cue_dir)
            .output()
            .expect("run cue cmd");
        let actual = Command::new(bin())
            .args(["cmd", &command, rust_dir.to_str().expect("path")])
            .current_dir(&rust_dir)
            .output()
            .expect("run ostinato cmd");

        if case_dir.join("extension").exists() {
            // Documented Ostinato extension: CUE rejects the fixture while
            // Ostinato injects the let-rooted task outputs.
            let expected_stdout =
                std::fs::read(case_dir.join("ostinato_stdout")).expect("extension stdout");
            assert!(!expected.status.success(), "{name}: cue must reject");
            assert!(
                actual.status.success(),
                "{name}: ostinato extension failed: {}",
                String::from_utf8_lossy(&actual.stderr)
            );
            assert_eq!(actual.stdout, expected_stdout, "{name}: stdout differs");
            if let Ok(artifacts) = std::fs::read_to_string(case_dir.join("artifacts")) {
                for artifact in artifacts
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                {
                    assert!(
                        rust_dir.join(artifact).exists(),
                        "{name}: {artifact} missing"
                    );
                }
            }
            continue;
        }

        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{name}: exit codes differ (cue stderr: {})",
            String::from_utf8_lossy(&expected.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "{name}: stdout differs");
        assert_eq!(actual.stderr, expected.stderr, "{name}: stderr differs");

        if let Ok(artifacts) = std::fs::read_to_string(case_dir.join("artifacts")) {
            for artifact in artifacts
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
            {
                assert_eq!(
                    std::fs::read(cue_dir.join(artifact)).expect("cue artifact"),
                    std::fs::read(rust_dir.join(artifact)).expect("rust artifact"),
                    "{name}: {artifact} contents differ"
                );
            }
        }
    }
}
