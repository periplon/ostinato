use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use ostinato_application::{
    CaptureOutcome, CaptureRequest, IsolationRequest, PortError, PortResult, WorkspaceHandle,
    WorkspaceProvider,
};
use ostinato_domain::{CapturedArtifact, IsolationMode};

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ARTIFACTS: usize = 512;

/// Filesystem and git-backed workspace isolation.
///
/// `worktree_root` is an optional override; by default worktrees live under
/// `<git-toplevel>/.ostinato/worktrees`.
#[derive(Debug)]
pub struct LocalWorkspace {
    worktree_root: Option<PathBuf>,
    /// Serialises `git worktree` mutations: concurrent steps may prepare at
    /// the same time, and git's worktree metadata is not lock-free.
    git_lock: Mutex<()>,
}

impl LocalWorkspace {
    #[must_use]
    pub fn new(worktree_root: Option<PathBuf>) -> Self {
        Self {
            worktree_root,
            git_lock: Mutex::new(()),
        }
    }
}

impl Default for LocalWorkspace {
    fn default() -> Self {
        Self::new(None)
    }
}

impl WorkspaceProvider for LocalWorkspace {
    fn prepare(&self, request: &IsolationRequest) -> PortResult<WorkspaceHandle> {
        let base_dir = request
            .base_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("."));
        if request.mode == IsolationMode::None {
            return Ok(WorkspaceHandle {
                dir: base_dir,
                mode: IsolationMode::None,
                path: None,
                git_root: None,
                base: None,
                keep: request.keep,
            });
        }

        let _guard = self
            .git_lock
            .lock()
            .map_err(|_| PortError::Io("workspace git lock poisoned".to_owned()))?;
        let git_root = fs::canonicalize(git_toplevel(&base_dir)?)
            .map_err(|err| PortError::Io(format!("cannot canonicalize git root: {err}")))?;
        let base_dir = fs::canonicalize(&base_dir).unwrap_or(base_dir);
        let base = resolve_base(&git_root, request.base.as_deref())?;
        let root = self
            .worktree_root
            .clone()
            .unwrap_or_else(|| git_root.join(".ostinato/worktrees"));
        create_private_dir(&root)?;
        let run_dir = root.join(sanitise_component(&request.run_id));
        create_private_dir(&run_dir)?;
        let unique = uuid::Uuid::new_v4().simple().to_string();
        let short = &unique[..8];
        let name = format!(
            "{}-r{}-a{}-{short}",
            sanitise_component(&request.step),
            request.round,
            request.attempt
        );
        let path = run_dir.join(name);
        run_git(
            &git_root,
            &[
                "worktree",
                "add",
                "--detach",
                &path.to_string_lossy(),
                &base,
            ],
        )?;

        // Run in the same relative subdirectory as the workflow, so relative
        // artifact globs and cwd behave identically in both isolation modes.
        let relative = base_dir
            .strip_prefix(&git_root)
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let dir = if relative.as_os_str().is_empty() {
            path.clone()
        } else {
            path.join(&relative)
        };
        Ok(WorkspaceHandle {
            dir,
            mode: IsolationMode::Worktree,
            path: Some(path),
            git_root: Some(git_root),
            base: Some(base),
            keep: request.keep,
        })
    }

    fn capture(&self, request: &CaptureRequest) -> PortResult<CaptureOutcome> {
        let cwd = request.cwd.clone();
        if !cwd.is_dir() {
            return Err(PortError::Io(format!(
                "artifact capture cwd is not a directory: {}",
                cwd.display()
            )));
        }
        let mut warnings = Vec::new();
        // Collect names/kinds first, deterministically, then read bytes within
        // the budget. This makes the captured set independent of glob order.
        let mut wanted: BTreeMap<String, String> = BTreeMap::new();
        for pattern in &request.patterns {
            match expand_pattern(&cwd, pattern) {
                Ok(paths) => {
                    for path in paths {
                        if let Some(name) = relative_name(&cwd, &path) {
                            wanted.entry(name).or_insert_with(|| "declared".to_owned());
                        }
                    }
                }
                Err(err) => warnings.push(err),
            }
        }
        if let Some(base) = &request.base {
            for (name, kind) in git_changes(&cwd, base)? {
                wanted.entry(name).or_insert(kind);
            }
        }

        let total_wanted = wanted.len();
        let mut artifacts = Vec::new();
        let mut total: u64 = 0;
        for (name, kind) in wanted {
            if artifacts.len() >= MAX_ARTIFACTS {
                warnings.push(format!(
                    "artifact limit reached; {} additional paths were not captured",
                    total_wanted.saturating_sub(MAX_ARTIFACTS)
                ));
                break;
            }
            let path = cwd.join(&name);
            if kind == "deleted" {
                artifacts.push(CapturedArtifact {
                    name,
                    kind,
                    bytes: Vec::new(),
                    truncated: false,
                    original_size: 0,
                    mode: None,
                });
                continue;
            }
            match read_regular_file(&cwd, &path, &mut warnings) {
                Ok(Some(ReadFile {
                    bytes,
                    original_size,
                    truncated,
                    mode,
                })) => {
                    let remaining = MAX_TOTAL_BYTES.saturating_sub(total);
                    let (bytes, truncated) =
                        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > remaining {
                            let take = usize::try_from(remaining).unwrap_or(0);
                            (bytes[..take.min(bytes.len())].to_vec(), true)
                        } else {
                            (bytes, truncated)
                        };
                    total = total.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
                    artifacts.push(CapturedArtifact {
                        name,
                        kind,
                        bytes,
                        truncated,
                        original_size,
                        mode,
                    });
                }
                Ok(None) => {}
                Err(err) => warnings.push(err),
            }
        }

        if let Some(base) = &request.base {
            let remaining = MAX_TOTAL_BYTES.saturating_sub(total);
            if artifacts.len() >= MAX_ARTIFACTS {
                warnings.push(
                    "artifact limit reached; the isolation patch was not captured".to_owned(),
                );
            } else {
                match git_patch(&cwd, base, remaining) {
                    Ok(Some(patch)) => artifacts.push(CapturedArtifact {
                        name: "_isolation/changes.patch".to_owned(),
                        kind: "patch".to_owned(),
                        bytes: patch.0,
                        truncated: patch.1,
                        original_size: patch.2,
                        mode: None,
                    }),
                    Ok(None) => {}
                    Err(err) => warnings.push(err.to_string()),
                }
            }
        }
        Ok(CaptureOutcome {
            artifacts,
            warnings,
        })
    }

    fn finish(&self, handle: &WorkspaceHandle) -> PortResult<()> {
        if handle.mode == IsolationMode::None || handle.keep {
            return Ok(());
        }
        let (Some(path), Some(git_root)) = (&handle.path, &handle.git_root) else {
            return Ok(());
        };
        let _guard = self
            .git_lock
            .lock()
            .map_err(|_| PortError::Io("workspace git lock poisoned".to_owned()))?;
        run_git(
            git_root,
            &["worktree", "remove", "--force", &path.to_string_lossy()],
        )?;
        // Best effort: stale registrations and empty run directories must not
        // accumulate.
        let _ = run_git(git_root, &["worktree", "prune"]);
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir(parent);
        }
        Ok(())
    }
}

fn run_git_limited(command: &mut Command, limit: u64) -> PortResult<(Vec<u8>, bool)> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| PortError::Io(format!("cannot run git: {err}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| PortError::Io("git stdout unavailable".to_owned()))?;
    let mut buffer = Vec::new();
    stdout
        .take(limit.saturating_add(1))
        .read_to_end(&mut buffer)
        .map_err(|err| PortError::Io(format!("cannot read git output: {err}")))?;
    let truncated = u64::try_from(buffer.len()).unwrap_or(u64::MAX) > limit;
    if truncated {
        buffer.truncate(usize::try_from(limit).unwrap_or(0));
    }
    if truncated {
        // Stop git once the budget is reached; dropping the pipe also closes it.
        let _ = child.kill();
        let _ = child.wait();
    } else {
        let status = child
            .wait()
            .map_err(|err| PortError::Io(format!("git wait failed: {err}")))?;
        if !status.success() {
            return Err(PortError::Io(format!(
                "git diff failed with exit {}",
                status.code().unwrap_or(-1)
            )));
        }
    }
    Ok((buffer, truncated))
}

fn base_git_command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(cwd).args([
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.pager=cat",
        "-c",
        "core.quotepath=false",
        "-c",
        "diff.external=",
        "--no-pager",
    ]);
    command
}

/// Neutralise every repository-configured filter and diff driver. The agent
/// can write the shared `.git/config`; capture must never execute it.
fn git_command(cwd: &Path) -> Command {
    let mut command = base_git_command(cwd);
    let output = base_git_command(cwd)
        .args([
            "config",
            "--get-regexp",
            r"^filter\..*\.(clean|smudge|process)$",
        ])
        .output();
    if let Ok(output) = output
        && output.status.success()
    {
        let mut seen = std::collections::BTreeSet::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let Some(key) = line.split_whitespace().next() else {
                continue;
            };
            let parts: Vec<&str> = key.split('.').collect();
            if parts.len() >= 3 && parts[0] == "filter" {
                seen.insert(parts[1].to_owned());
            }
        }
        for driver in seen {
            for kind in ["clean", "smudge"] {
                command.arg("-c").arg(format!("filter.{driver}.{kind}=cat"));
            }
            command.arg("-c").arg(format!("filter.{driver}.process="));
        }
    }
    let output = base_git_command(cwd)
        .args(["config", "--get-regexp", r"^diff\..*\.(command|textconv)$"])
        .output();
    if let Ok(output) = output
        && output.status.success()
    {
        let mut seen = std::collections::BTreeSet::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let Some(key) = line.split_whitespace().next() else {
                continue;
            };
            let parts: Vec<&str> = key.split('.').collect();
            if parts.len() >= 3 && parts[0] == "diff" {
                seen.insert(parts[1].to_owned());
            }
        }
        for driver in seen {
            command.arg("-c").arg(format!("diff.{driver}.command="));
            command.arg("-c").arg(format!("diff.{driver}.textconv="));
        }
    }
    command
}

fn sanitise_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len().min(64));
    for ch in value.chars().take(64) {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() || out == "." || out == ".." {
        "step".to_owned()
    } else {
        out
    }
}

fn create_private_dir(path: &Path) -> PortResult<()> {
    fs::create_dir_all(path)
        .map_err(|err| PortError::Io(format!("cannot create {}: {err}", path.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn git_toplevel(base: &Path) -> PortResult<PathBuf> {
    let output = git_command(base)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|err| PortError::Io(format!("cannot run git: {err}")))?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || text.is_empty() {
        return Err(PortError::Config(format!(
            "worktree isolation requires a git repository; `{}` is not inside one",
            base.display()
        )));
    }
    Ok(PathBuf::from(text))
}

fn resolve_base(git_root: &Path, base: Option<&str>) -> PortResult<String> {
    let base = base.unwrap_or("HEAD");
    if base.is_empty() {
        return Err(PortError::Config(
            "isolation.base must not be empty".to_owned(),
        ));
    }
    let spec = format!("{base}^{{commit}}");
    let output = git_command(git_root)
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            &spec,
        ])
        .output()
        .map_err(|err| PortError::Io(format!("cannot run git: {err}")))?;
    if !output.status.success() {
        return Err(PortError::Config(format!(
            "isolation.base `{base}` does not resolve to a commit"
        )));
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if sha.len() < 7 || !sha.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(PortError::Config(format!(
            "isolation.base `{base}` resolved to an invalid commit id"
        )));
    }
    Ok(sha)
}

fn run_git(cwd: &Path, args: &[&str]) -> PortResult<()> {
    let output = git_command(cwd)
        .args(args)
        .output()
        .map_err(|err| PortError::Io(format!("cannot run git: {err}")))?;
    if !output.status.success() {
        return Err(PortError::Io(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

fn git_output(cwd: &Path, args: &[&str]) -> PortResult<Vec<u8>> {
    let output = git_command(cwd)
        .args(args)
        .output()
        .map_err(|err| PortError::Io(format!("cannot run git: {err}")))?;
    if !output.status.success() {
        return Err(PortError::Io(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

/// Changes relative to the recorded isolation base, including commits the
/// agent made inside the worktree plus untracked files.
fn git_changes(cwd: &Path, base: &str) -> PortResult<Vec<(String, String)>> {
    let prefix = git_prefix(cwd)?;
    let mut changes = BTreeMap::new();
    let output = git_command(cwd)
        .args([
            "diff",
            "--name-status",
            "-z",
            "--no-renames",
            "--relative",
            base,
            "--",
            ".",
        ])
        .output()
        .map_err(|err| PortError::Io(format!("cannot run git diff: {err}")))?;
    if !output.status.success() {
        return Err(PortError::Io(format!(
            "git diff --name-status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let mut fields = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    while let Some(status) = fields.next() {
        let Some(path) = fields.next() else {
            break;
        };
        let path = String::from_utf8_lossy(path).into_owned();
        let path = path.strip_prefix(&prefix).unwrap_or(&path).to_owned();
        if !safe_relative(&path) || is_internal_path(&path) {
            continue;
        }
        let kind = match status.first().copied() {
            Some(b'A') => "created",
            Some(b'D') => "deleted",
            _ => "changed",
        };
        changes.insert(path, kind.to_owned());
    }

    let output = git_command(cwd)
        .args([
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".",
        ])
        .output()
        .map_err(|err| PortError::Io(format!("cannot run git ls-files: {err}")))?;
    if !output.status.success() {
        return Err(PortError::Io(format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    for field in output.stdout.split(|byte| *byte == 0) {
        if field.is_empty() {
            continue;
        }
        let path = String::from_utf8_lossy(field).into_owned();
        let path = path.strip_prefix(&prefix).unwrap_or(&path).to_owned();
        if safe_relative(&path) && !is_internal_path(&path) {
            changes.entry(path).or_insert_with(|| "created".to_owned());
        }
    }
    Ok(changes.into_iter().collect())
}

/// Build a patch against the isolation base without running repository clean
/// filters. `git add` is deliberately avoided: an agent-written
/// `.gitattributes` or `.git/config` filter must never run in the trusted
/// process. Untracked files are diffed one by one with `--no-index`.
fn git_patch(cwd: &Path, base: &str, budget: u64) -> PortResult<Option<(Vec<u8>, bool, u64)>> {
    let mut patch = Vec::new();
    let mut truncated = false;
    let mut tracked_command = git_command(cwd);
    tracked_command.args([
        "diff",
        "--binary",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--relative",
        base,
        "--",
        ".",
    ]);
    let (tracked, tracked_truncated) = run_git_limited(&mut tracked_command, budget)?;
    patch.extend_from_slice(&tracked);
    if tracked_truncated {
        patch.truncate(usize::try_from(budget).unwrap_or(0).min(patch.len()));
        let original_size = u64::try_from(patch.len()).unwrap_or(u64::MAX);
        return Ok(Some((patch, true, original_size)));
    }

    // Include untracked files without staging them. Skip very large files:
    // they are still captured (truncated) as individual artifacts.
    let untracked = git_command(cwd)
        .args([
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".",
        ])
        .output()
        .map_err(|err| PortError::Io(format!("cannot run git ls-files: {err}")))?;
    if untracked.status.success() {
        for field in untracked.stdout.split(|byte| *byte == 0) {
            if field.is_empty() {
                continue;
            }
            if u64::try_from(patch.len()).unwrap_or(u64::MAX) >= budget {
                truncated = true;
                break;
            }
            let name = String::from_utf8_lossy(field).into_owned();
            let path = cwd.join(&name);
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !metadata.file_type().is_file() || metadata.len() > MAX_FILE_BYTES {
                continue;
            }
            let output = git_command(cwd)
                .args([
                    "diff",
                    "--no-index",
                    "--binary",
                    "--no-color",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--",
                    "/dev/null",
                    &name,
                ])
                .output()
                .map_err(|err| PortError::Io(format!("cannot run git diff --no-index: {err}")))?;
            // exit code 1 is the normal "files differ" result.
            if output.status.code() == Some(1) || output.status.success() {
                let remaining =
                    budget.saturating_sub(u64::try_from(patch.len()).unwrap_or(u64::MAX));
                if u64::try_from(output.stdout.len()).unwrap_or(u64::MAX) > remaining {
                    let take = usize::try_from(remaining).unwrap_or(0);
                    patch.extend_from_slice(&output.stdout[..take.min(output.stdout.len())]);
                    truncated = true;
                    break;
                }
                patch.extend_from_slice(&output.stdout);
            }
        }
    }

    if patch.is_empty() {
        return Ok(None);
    }
    let original_size = u64::try_from(patch.len()).unwrap_or(u64::MAX);
    if original_size > budget {
        patch.truncate(usize::try_from(budget).unwrap_or(0).min(patch.len()));
        truncated = true;
    }
    Ok(Some((patch, truncated, original_size)))
}

fn expand_pattern(cwd: &Path, pattern: &str) -> Result<Vec<PathBuf>, String> {
    if pattern.trim().is_empty() {
        return Err("artifact glob must not be empty".to_owned());
    }
    if !safe_relative(pattern) {
        return Err(format!(
            "artifact glob `{pattern}` must be relative and must not contain `..`"
        ));
    }
    let compiled = glob::Pattern::new(pattern)
        .map_err(|err| format!("invalid artifact glob `{pattern}`: {err}"))?;
    let options = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: true,
    };
    let mut paths = Vec::new();
    let mut visited = 0usize;
    for entry in walkdir::WalkDir::new(cwd)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || entry
                    .path()
                    .strip_prefix(cwd)
                    .map(|relative| !is_internal_path(&relative.to_string_lossy()))
                    .unwrap_or(true)
        })
    {
        visited += 1;
        if visited > 100_000 {
            return Err(format!(
                "artifact glob `{pattern}` exceeded the traversal limit"
            ));
        }
        let entry = entry.map_err(|err| format!("cannot walk artifacts: {err}"))?;
        if entry.file_type().is_dir() {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(cwd) else {
            continue;
        };
        if compiled.matches_path_with(relative, options) {
            paths.push(entry.into_path());
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn relative_name(cwd: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(cwd).ok()?;
    let name = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if name.is_empty() || !safe_relative(&name) || is_internal_path(&name) {
        None
    } else {
        Some(name)
    }
}

struct ReadFile {
    bytes: Vec<u8>,
    original_size: u64,
    truncated: bool,
    mode: Option<u32>,
}

fn read_regular_file(
    root: &Path,
    path: &Path,
    warnings: &mut Vec<String>,
) -> Result<Option<ReadFile>, String> {
    let display = path.display();
    let canonical_root = fs::canonicalize(root)
        .map_err(|err| format!("cannot canonicalize capture root {}: {err}", root.display()))?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("cannot inspect {display}: no parent"))?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|err| format!("cannot canonicalize {}: {err}", parent.display()))?;
    if !canonical_parent.starts_with(&canonical_root) {
        warnings.push(format!("skipped {display}: resolves outside the workspace"));
        return Ok(None);
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|err| format!("cannot stat {display}: {err}"))?;
    if metadata.file_type().is_symlink() {
        warnings.push(format!(
            "skipped {display}: symbolic links are not captured"
        ));
        return Ok(None);
    }
    if !metadata.is_file() {
        return Ok(None);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() > 1 {
            warnings.push(format!("skipped {display}: hard links are not captured"));
            return Ok(None);
        }
    }

    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .map_err(|err| format!("cannot read {display}: {err}"))?;
    let opened = file
        .metadata()
        .map_err(|err| format!("cannot stat open file {display}: {err}"))?;
    if !opened.is_file() {
        warnings.push(format!("skipped {display}: not a regular file"));
        return Ok(None);
    }
    let original_size = opened.len();
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("cannot read {display}: {err}"))?;
    let truncated = u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES;
    if truncated {
        bytes.truncate(usize::try_from(MAX_FILE_BYTES).unwrap_or(usize::MAX));
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        Some(opened.permissions().mode() & 0o777)
    };
    #[cfg(not(unix))]
    let mode = None;
    Ok(Some(ReadFile {
        bytes,
        original_size,
        truncated,
        mode,
    }))
}

fn git_prefix(cwd: &Path) -> PortResult<String> {
    let output = git_output(cwd, &["rev-parse", "--show-prefix"])?;
    Ok(String::from_utf8_lossy(&output).trim().to_owned())
}

fn safe_relative(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') {
        return false;
    }
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return false;
    }
    candidate.components().all(|component| match component {
        Component::Normal(value) => {
            let text = value.to_string_lossy();
            !text.is_empty() && text != "." && !text.contains('\0')
        }
        Component::CurDir => true,
        Component::ParentDir | Component::RootDir | Component::Prefix(_) => false,
    })
}

fn is_internal_path(path: &str) -> bool {
    path.split('/')
        .any(|component| matches!(component, ".git" | ".ostinato" | "_isolation"))
}

/// Write an extracted artifact under `root` without following symlinks and
/// without escaping the root. `force` allows replacing an existing regular
/// file; the path itself is always validated.
///
/// # Errors
/// Returns a human-readable error for unsafe or unwritable paths.
pub fn write_artifact(
    root: &Path,
    name: &str,
    content: &[u8],
    mode: Option<u32>,
    force: bool,
) -> Result<PathBuf, String> {
    if !safe_relative(name) || (is_internal_path(name) && name != "_isolation/changes.patch") {
        return Err(format!("unsafe artifact path `{name}`"));
    }
    fs::create_dir_all(root)
        .map_err(|err| format!("cannot create extraction dir {}: {err}", root.display()))?;
    let destination = root.join(name);
    let parent = destination
        .parent()
        .ok_or_else(|| format!("artifact `{name}` has no parent directory"))?;
    let relative_parent = parent
        .strip_prefix(root)
        .map_err(|_| format!("artifact `{name}` has no parent under the extraction root"))?;
    create_dirs_no_symlink(root, relative_parent)?;
    let canonical_root = fs::canonicalize(root)
        .map_err(|err| format!("cannot canonicalize {}: {err}", root.display()))?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|err| format!("cannot canonicalize {}: {err}", parent.display()))?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err(format!(
            "artifact `{name}` resolves outside the extraction directory"
        ));
    }
    if let Ok(metadata) = fs::symlink_metadata(&destination) {
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "artifact destination {} is a symbolic link",
                destination.display()
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() > 1 {
                return Err(format!(
                    "artifact destination {} is hard-linked; refusing to overwrite it",
                    destination.display()
                ));
            }
        }
        if !force {
            return Err(format!(
                "artifact destination {} already exists; pass --force to replace it",
                destination.display()
            ));
        }
    }
    let temp = parent.join(format!(
        ".ostinato-extract-{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .custom_flags(libc::O_NOFOLLOW)
            .mode(mode.unwrap_or(0o600));
    }
    let mut file = options
        .open(&temp)
        .map_err(|err| format!("cannot write {}: {err}", temp.display()))?;
    use std::io::Write;
    if let Err(err) = file.write_all(content) {
        let _ = fs::remove_file(&temp);
        return Err(format!("cannot write {}: {err}", temp.display()));
    }
    drop(file);
    if let Err(err) = fs::rename(&temp, &destination) {
        let _ = fs::remove_file(&temp);
        return Err(format!("cannot replace {}: {err}", destination.display()));
    }
    Ok(destination)
}

fn create_dirs_no_symlink(root: &Path, relative: &Path) -> Result<(), String> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(format!(
                "unsafe extraction path component in {}",
                relative.display()
            ));
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(format!(
                        "extraction path {} is a symbolic link",
                        current.display()
                    ));
                }
                if !metadata.is_dir() {
                    return Err(format!(
                        "extraction path {} is not a directory",
                        current.display()
                    ));
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)
                    .map_err(|err| format!("cannot create {}: {err}", current.display()))?;
            }
            Err(err) => {
                return Err(format!("cannot inspect {}: {err}", current.display()));
            }
        }
    }
    Ok(())
}

/// Convenience for tests and callers that only need the safe relative check.
#[must_use]
pub fn is_safe_relative(path: &str) -> bool {
    safe_relative(path)
}
