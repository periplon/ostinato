#![allow(clippy::expect_used)]

use std::path::Path;
use std::process::Command;

use ostinato_adapters::{LocalWorkspace, is_safe_relative, write_artifact};
use ostinato_application::{CaptureRequest, IsolationRequest, WorkspaceProvider};
use ostinato_domain::IsolationMode;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, contents).expect("write");
}

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?} failed");
}

fn git_repo() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("tempdir");
    git(directory.path(), &["init", "-q"]);
    write(&directory.path().join("tracked.txt"), "base\n");
    git(directory.path(), &["add", "."]);
    git(directory.path(), &["commit", "-qm", "init"]);
    directory
}

#[test]
fn declared_globs_are_sorted_and_deduplicated() {
    let directory = tempfile::tempdir().expect("tempdir");
    write(&directory.path().join("out/b.txt"), "b");
    write(&directory.path().join("out/a.txt"), "a");
    write(&directory.path().join("out/nested/c.txt"), "c");
    let workspace = LocalWorkspace::default();
    let captured = workspace
        .capture(&CaptureRequest {
            cwd: directory.path().to_path_buf(),
            patterns: vec![
                "out/*.txt".to_owned(),
                "out/a.txt".to_owned(),
                "out/**/*.txt".to_owned(),
            ],
            include_git_changes: false,
        })
        .expect("capture");
    let names: Vec<&str> = captured
        .artifacts
        .iter()
        .map(|artifact| artifact.name.as_str())
        .collect();
    assert_eq!(names, vec!["out/a.txt", "out/b.txt", "out/nested/c.txt"]);
    assert!(captured.warnings.is_empty(), "{:?}", captured.warnings);
}

#[test]
fn symlinks_and_hardlinks_are_not_captured() {
    let directory = tempfile::tempdir().expect("tempdir");
    let outside = directory.path().join("outside.txt");
    write(&outside, "secret");
    std::os::unix::fs::symlink(&outside, directory.path().join("link.txt")).expect("symlink");
    std::fs::hard_link(&outside, directory.path().join("hard.txt")).expect("hardlink");
    let workspace = LocalWorkspace::default();
    let captured = workspace
        .capture(&CaptureRequest {
            cwd: directory.path().to_path_buf(),
            patterns: vec!["*.txt".to_owned()],
            include_git_changes: false,
        })
        .expect("capture");
    assert!(captured.artifacts.is_empty(), "{:?}", captured.artifacts);
    assert!(
        captured
            .warnings
            .iter()
            .any(|warning| warning.contains("hard links")),
        "{:?}",
        captured.warnings
    );
}

#[test]
fn extraction_rejects_traversal_and_symlinked_parents() {
    let directory = tempfile::tempdir().expect("tempdir");
    let root = directory.path().join("root");
    std::fs::create_dir_all(&root).expect("root");
    assert!(write_artifact(&root, "../escape.txt", b"x", None, false).is_err());
    assert!(write_artifact(&root, "/tmp/escape.txt", b"x", None, false).is_err());
    let outside = directory.path().join("outside");
    std::fs::create_dir_all(&outside).expect("outside");
    std::os::unix::fs::symlink(&outside, root.join("link")).expect("symlink");
    assert!(write_artifact(&root, "link/escape.txt", b"x", None, false).is_err());

    let written = write_artifact(&root, "safe/file.txt", b"content", None, false).expect("safe");
    assert_eq!(std::fs::read_to_string(written).expect("read"), "content");
    assert!(write_artifact(&root, "safe/file.txt", b"other", None, false).is_err());
    write_artifact(&root, "safe/file.txt", b"other", None, true).expect("force");
}

#[test]
fn relative_path_validation_is_strict() {
    assert!(is_safe_relative("a/b.txt"));
    assert!(is_safe_relative("a/./b.txt"));
    assert!(!is_safe_relative(""));
    assert!(!is_safe_relative("/abs"));
    assert!(!is_safe_relative("../x"));
    assert!(!is_safe_relative("a/../../x"));
}

#[test]
fn worktree_lifecycle_captures_tracked_and_untracked_changes() {
    let repo = git_repo();
    let workspace = LocalWorkspace::default();
    let handle = workspace
        .prepare(&IsolationRequest {
            run_id: "run-1".to_owned(),
            step: "step".to_owned(),
            round: 0,
            attempt: 0,
            mode: IsolationMode::Worktree,
            base: None,
            keep: false,
            base_dir: Some(repo.path().to_path_buf()),
        })
        .expect("prepare");
    assert_eq!(handle.mode, IsolationMode::Worktree);
    assert!(handle.path.as_ref().expect("path").exists());
    assert!(handle.base.as_ref().expect("base").len() >= 7);

    write(&handle.dir.join("tracked.txt"), "base\nagent\n");
    write(&handle.dir.join("created.txt"), "new\n");
    write(&handle.dir.join("reports/review.md"), "report\n");
    let captured = workspace
        .capture(&CaptureRequest {
            cwd: handle.dir.clone(),
            patterns: vec!["reports/*.md".to_owned()],
            include_git_changes: true,
        })
        .expect("capture");
    let names: Vec<&str> = captured
        .artifacts
        .iter()
        .map(|artifact| artifact.name.as_str())
        .collect();
    assert!(names.contains(&"tracked.txt"), "{names:?}");
    assert!(names.contains(&"created.txt"), "{names:?}");
    assert!(names.contains(&"reports/review.md"), "{names:?}");
    assert!(names.contains(&"_isolation/changes.patch"), "{names:?}");
    let patch = captured
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == "patch")
        .expect("patch");
    let patch = String::from_utf8_lossy(&patch.bytes);
    assert!(patch.contains("+agent"), "{patch}");
    assert!(patch.contains("created.txt"), "{patch}");

    let path = handle.path.clone().expect("path");
    workspace.finish(&handle).expect("finish");
    assert!(!path.exists(), "worktree should be removed");
    // The original checkout must be untouched.
    assert_eq!(
        std::fs::read_to_string(repo.path().join("tracked.txt")).expect("tracked"),
        "base\n"
    );
    assert!(!repo.path().join("created.txt").exists());
}

#[test]
fn worktree_base_injection_and_non_repos_fail_cleanly() {
    let repo = git_repo();
    let workspace = LocalWorkspace::default();
    let error = workspace
        .prepare(&IsolationRequest {
            run_id: "run".to_owned(),
            step: "step".to_owned(),
            round: 0,
            attempt: 0,
            mode: IsolationMode::Worktree,
            base: Some("--force".to_owned()),
            keep: false,
            base_dir: Some(repo.path().to_path_buf()),
        })
        .expect_err("injection");
    assert!(error.to_string().contains("does not resolve"), "{error}");

    let plain = tempfile::tempdir().expect("plain");
    let error = workspace
        .prepare(&IsolationRequest {
            run_id: "run".to_owned(),
            step: "step".to_owned(),
            round: 0,
            attempt: 0,
            mode: IsolationMode::Worktree,
            base: None,
            keep: false,
            base_dir: Some(plain.path().to_path_buf()),
        })
        .expect_err("non-repo");
    assert!(error.to_string().contains("git repository"), "{error}");
}

#[test]
fn worktree_capture_is_relative_to_a_subdirectory_workflow() {
    let repo = git_repo();
    std::fs::create_dir_all(repo.path().join("sub")).expect("sub");
    write(&repo.path().join("sub/before.txt"), "before\n");
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-qm", "sub"]);
    let workspace = LocalWorkspace::default();
    let handle = workspace
        .prepare(&IsolationRequest {
            run_id: "run-sub".to_owned(),
            step: "step".to_owned(),
            round: 0,
            attempt: 0,
            mode: IsolationMode::Worktree,
            base: None,
            keep: false,
            base_dir: Some(repo.path().join("sub")),
        })
        .expect("prepare");
    assert_eq!(handle.dir, handle.path.as_ref().expect("path").join("sub"));
    write(&handle.dir.join("created.txt"), "new\n");
    write(&handle.dir.join("before.txt"), "before\nchanged\n");
    let captured = workspace
        .capture(&CaptureRequest {
            cwd: handle.dir.clone(),
            patterns: Vec::new(),
            include_git_changes: true,
        })
        .expect("capture");
    let names: Vec<&str> = captured
        .artifacts
        .iter()
        .map(|artifact| artifact.name.as_str())
        .collect();
    assert!(names.contains(&"created.txt"), "{names:?}");
    assert!(names.contains(&"before.txt"), "{names:?}");
    assert!(
        !names.iter().any(|name| name.starts_with("sub/")),
        "{names:?}"
    );
    let patch = captured
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == "patch")
        .expect("patch");
    let patch = String::from_utf8_lossy(&patch.bytes);
    assert!(patch.contains("a/before.txt"), "{patch}");
    assert!(!patch.contains("a/sub/"), "{patch}");
    workspace.finish(&handle).expect("finish");
}
