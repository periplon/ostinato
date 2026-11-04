use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
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
        if request.include_git_changes {
            for (name, kind) in git_changes(&cwd)? {
                wanted.entry(name).or_insert(kind);
            }
        }

        let mut artifacts = Vec::new();
        let mut total: u64 = 0;
        for (name, kind) in wanted {
            if artifacts.len() >= MAX_ARTIFACTS {
                warnings.push(format!(
                    "artifact limit reached; {} additional paths were not captured",
                    MAX_ARTIFACTS
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

        if request.include_git_changes {
            let remaining = MAX_TOTAL_BYTES.saturating_sub(total);
            match git_patch(&cwd, remaining) {
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
        // Best effort: stale registrations must not accumulate.
        let _ = run_git(git_root, &["worktree", "prune"]);
        Ok(())
    }
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
    let output = git_output(base, &["rev-parse", "--show-toplevel"])?;
    let text = String::from_utf8_lossy(&output).trim().to_owned();
    if text.is_empty() {
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
    let output = Command::new("git")
        .arg("-C")
        .arg(git_root)
        .args([
            "-c",
            "core.fsmonitor=false",
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
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["-c", "core.fsmonitor=false", "-c", "core.pager=cat"])
        .args(["-c", "core.quotepath=false", "--no-pager"])
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
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["-c", "core.fsmonitor=false", "-c", "core.pager=cat"])
        .args(["-c", "core.quotepath=false", "--no-pager"])
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

/// Parse `git status --porcelain=v1 -z --no-renames`.
fn git_changes(cwd: &Path) -> PortResult<Vec<(String, String)>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["-c", "core.fsmonitor=false", "-c", "core.quotepath=false"])
        .args([
            "--no-pager",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ])
        .output()
        .map_err(|err| PortError::Io(format!("cannot run git status: {err}")))?;
    if !output.status.success() {
        return Err(PortError::Io(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let mut changes = Vec::new();
    for record in output.stdout.split(|byte| *byte == 0) {
        if record.len() < 4 {
            continue;
        }
        let status = &record[..2];
        let path = String::from_utf8_lossy(&record[3..]).into_owned();
        if !safe_relative(&path) || is_internal_path(&path) {
            continue;
        }
        let kind = match status {
            b"??" | b"A " | b"AM" => "created",
            b" D" | b"D " => "deleted",
            _ => "changed",
        };
        changes.push((path, kind.to_owned()));
    }
    Ok(changes)
}

/// Build a patch with a private index so untracked files are included without
/// touching the agent's worktree index. Textconv and external diff drivers are
/// disabled: `.gitattributes` or repo config written by the agent must not run
/// code in the trusted process.
fn git_patch(cwd: &Path, budget: u64) -> PortResult<Option<(Vec<u8>, bool, u64)>> {
    let index =
        std::env::temp_dir().join(format!("ostinato-index-{}", uuid::Uuid::new_v4().simple()));
    let empty_tree = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
    let has_head = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
        .output()
        .is_ok_and(|output| output.status.success());
    let base_tree = if has_head { "HEAD" } else { empty_tree };
    let result = (|| {
        let output = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .env("GIT_INDEX_FILE", &index)
            .args(["-c", "core.fsmonitor=false", "-c", "core.quotepath=false"])
            .args(["--no-pager", "read-tree", base_tree])
            .output()
            .map_err(|err| PortError::Io(format!("cannot run git read-tree: {err}")))?;
        if !output.status.success() {
            return Err(PortError::Io(format!(
                "git read-tree failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let output = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .env("GIT_INDEX_FILE", &index)
            .args(["-c", "core.fsmonitor=false", "-c", "core.quotepath=false"])
            .args(["--no-pager", "add", "-A", "--", "."])
            .output()
            .map_err(|err| PortError::Io(format!("cannot run git add: {err}")))?;
        if !output.status.success() {
            return Err(PortError::Io(format!(
                "git add failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let output = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .env("GIT_INDEX_FILE", &index)
            .args(["-c", "core.fsmonitor=false", "-c", "core.quotepath=false"])
            .args([
                "--no-pager",
                "-c",
                "diff.external=",
                "diff",
                "--cached",
                "--binary",
                "--no-ext-diff",
                "--no-textconv",
                base_tree,
                "--",
                ".",
            ])
            .output()
            .map_err(|err| PortError::Io(format!("cannot run git diff: {err}")))?;
        if !output.status.success() {
            return Err(PortError::Io(format!(
                "git diff failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(output.stdout)
    })();
    let _ = fs::remove_file(&index);
    let bytes = result?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let original_size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let (bytes, truncated) = if original_size > budget {
        (
            bytes[..usize::try_from(budget).unwrap_or(0).min(bytes.len())].to_vec(),
            true,
        )
    } else {
        (bytes, false)
    };
    Ok(Some((bytes, truncated, original_size)))
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
    let joined = cwd.join(pattern);
    let options = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };
    let entries = glob::glob_with(&joined.to_string_lossy(), options)
        .map_err(|err| format!("invalid artifact glob `{pattern}`: {err}"))?;
    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(path) => {
                if let Ok(metadata) = fs::symlink_metadata(&path)
                    && metadata.file_type().is_file()
                {
                    paths.push(path);
                }
            }
            Err(err) => return Err(format!("cannot expand artifact glob `{pattern}`: {err}")),
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
        .any(|component| matches!(component, ".git" | ".ostinato"))
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
    if !safe_relative(name) || is_internal_path(name) {
        return Err(format!("unsafe artifact path `{name}`"));
    }
    fs::create_dir_all(root)
        .map_err(|err| format!("cannot create extraction dir {}: {err}", root.display()))?;
    let destination = root.join(name);
    let parent = destination
        .parent()
        .ok_or_else(|| format!("artifact `{name}` has no parent directory"))?;
    fs::create_dir_all(parent)
        .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    let canonical_root = fs::canonicalize(root)
        .map_err(|err| format!("cannot canonicalize {}: {err}", root.display()))?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|err| format!("cannot canonicalize {}: {err}", parent.display()))?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err(format!(
            "artifact `{name}` resolves outside the extraction directory"
        ));
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true);
    if !force {
        options.create_new(true);
    } else {
        options.truncate(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .custom_flags(libc::O_NOFOLLOW)
            .mode(mode.unwrap_or(0o600));
    }
    let mut file = options
        .open(&destination)
        .map_err(|err| format!("cannot write {}: {err}", destination.display()))?;
    use std::io::Write;
    file.write_all(content)
        .map_err(|err| format!("cannot write {}: {err}", destination.display()))?;
    Ok(destination)
}

/// Convenience for tests and callers that only need the safe relative check.
#[must_use]
pub fn is_safe_relative(path: &str) -> bool {
    safe_relative(path)
}

/// Names captured from git status, exposed for adapter tests.
#[doc(hidden)]
pub fn git_status_names(cwd: &Path) -> Vec<String> {
    git_changes(cwd)
        .map(|changes| changes.into_iter().map(|(name, _)| name).collect())
        .unwrap_or_default()
}

/// Deterministic sorted set used by tests.
#[doc(hidden)]
pub fn sorted_unique(values: &[String]) -> Vec<String> {
    values
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
