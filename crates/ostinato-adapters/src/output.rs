//! Safe output sink adapter.
//!
//! Writes are atomic (temporary file in the destination directory followed by
//! a rename), and refuse symlinks, hard links, input/destination aliasing and
//! accidental overwrites unless `--force` is set.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use ostinato_application::{OutputOptions, OutputSink, PortError, PortResult, WriteOutcome};

/// Filesystem/stdout output sink used by the CLI.
#[derive(Debug, Default, Clone, Copy)]
pub struct FileOutputSink;

impl OutputSink for FileOutputSink {
    fn write(
        &self,
        target: Option<&Path>,
        content: &str,
        options: OutputOptions,
        protected: &[PathBuf],
    ) -> PortResult<WriteOutcome> {
        let Some(path) = target else {
            return write_stdout(content);
        };
        if path == Path::new("-") {
            return write_stdout(content);
        }
        let Some(staged) = stage_output(path, content, options, protected)? else {
            return Ok(WriteOutcome::Skipped);
        };
        persist_staged(staged, options.force)?;
        Ok(WriteOutcome::Written)
    }
}

/// Write several outputs as one transaction: every destination is preflighted
/// and every temporary file is written before any destination is replaced.
///
/// If staging fails, all temporary files are discarded and no destination is
/// touched. Stdout targets are not supported here; callers should use
/// [`FileOutputSink`] for those.
///
/// # Errors
/// Returns the first preflight, staging or rename failure.
pub fn write_all_atomic(
    files: &[(PathBuf, String)],
    options: OutputOptions,
    protected: &[PathBuf],
) -> PortResult<Vec<WriteOutcome>> {
    // Preflight every destination's name/path limits before writing anything.
    for (path, _) in files {
        let destination = absolute_destination(path)?;
        prepare_parent(&destination)?;
        check_path_limits(&destination)?;
    }
    let mut staged = Vec::with_capacity(files.len());
    let mut outcomes = Vec::with_capacity(files.len());
    for (path, content) in files {
        match stage_output(path, content, options, protected)? {
            Some(output) => {
                staged.push(output);
                outcomes.push(WriteOutcome::Written);
            }
            None => {
                outcomes.push(WriteOutcome::Skipped);
            }
        }
    }
    // All temp files exist and are complete; commit with rollback so a late
    // rename failure leaves no output behind.
    commit_all(staged, options.force)?;
    Ok(outcomes)
}

struct StagedOutput {
    temporary: tempfile::NamedTempFile,
    destination: PathBuf,
}

fn stage_output(
    path: &Path,
    content: &str,
    options: OutputOptions,
    protected: &[PathBuf],
) -> PortResult<Option<StagedOutput>> {
    let destination = absolute_destination(path)?;
    let resolved_destination = if std::fs::symlink_metadata(&destination).is_ok() {
        std::fs::canonicalize(&destination).unwrap_or_else(|_| destination.clone())
    } else {
        destination.clone()
    };
    for input in protected {
        let input = std::fs::canonicalize(input)
            .unwrap_or_else(|_| absolute_destination(input).unwrap_or_else(|_| input.clone()));
        if input == resolved_destination {
            return Err(PortError::Io(format!(
                "refusing to write {}: it is one of the inputs",
                destination.display()
            )));
        }
    }
    prepare_parent(&destination)?;
    check_path_limits(&destination)?;
    let parent = destination.parent().ok_or_else(|| {
        PortError::Io(format!(
            "destination {} has no parent directory",
            destination.display()
        ))
    })?;
    if let Ok(metadata) = std::fs::symlink_metadata(&destination) {
        if metadata.file_type().is_symlink() {
            return Err(PortError::Io(format!(
                "refusing to write {}: destination is a symbolic link",
                destination.display()
            )));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() > 1 {
                return Err(PortError::Io(format!(
                    "refusing to write {}: destination is hard-linked",
                    destination.display()
                )));
            }
        }
        if !options.force {
            if options.skip_existing {
                return Ok(None);
            }
            return Err(PortError::Io(format!(
                "refusing to overwrite {}: pass --force",
                destination.display()
            )));
        }
    }
    let existing_permissions = std::fs::metadata(&destination)
        .ok()
        .map(|metadata| metadata.permissions());
    let mut temporary = tempfile::Builder::new()
        .prefix(".ostinato-output-")
        .tempfile_in(parent)
        .map_err(|error| {
            PortError::Io(format!(
                "cannot create temporary output in {}: {error}",
                parent.display()
            ))
        })?;
    temporary
        .write_all(content.as_bytes())
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|error| {
            PortError::Io(format!(
                "cannot write temporary output for {}: {error}",
                destination.display()
            ))
        })?;
    if let Some(permissions) = existing_permissions {
        std::fs::set_permissions(temporary.path(), permissions).map_err(|error| {
            PortError::Io(format!(
                "cannot preserve permissions for {}: {error}",
                destination.display()
            ))
        })?;
    }
    Ok(Some(StagedOutput {
        temporary,
        destination,
    }))
}

fn persist_staged(staged: StagedOutput, force: bool) -> PortResult<()> {
    let destination = staged.destination;
    if force {
        staged.temporary.persist(&destination).map_err(|error| {
            PortError::Io(format!(
                "cannot replace {}: {}",
                destination.display(),
                error.error
            ))
        })?;
    } else {
        staged
            .temporary
            .persist_noclobber(&destination)
            .map_err(|error| {
                PortError::Io(format!(
                    "cannot write {}: {}",
                    destination.display(),
                    error.error
                ))
            })?;
    }
    Ok(())
}

struct CommitEntry {
    destination: PathBuf,
    backup: Option<PathBuf>,
}

fn commit_all(staged: Vec<StagedOutput>, force: bool) -> PortResult<()> {
    let mut committed: Vec<CommitEntry> = Vec::with_capacity(staged.len());
    for StagedOutput {
        temporary,
        destination,
    } in staged
    {
        let backup = match backup_existing(&destination) {
            Ok(backup) => backup,
            Err(error) => {
                rollback(&committed);
                return Err(error);
            }
        };
        let persisted = if force {
            temporary.persist(&destination)
        } else {
            temporary.persist_noclobber(&destination)
        };
        match persisted {
            Ok(_) => committed.push(CommitEntry {
                destination,
                backup,
            }),
            Err(error) => {
                let restore_error = backup
                    .as_ref()
                    .and_then(|backup| std::fs::rename(backup, &destination).err());
                rollback(&committed);
                if let Some(rename_error) = restore_error {
                    return Err(PortError::Io(format!(
                        "cannot write {}: {}; cannot restore backup: {rename_error}",
                        destination.display(),
                        error.error
                    )));
                }
                return Err(PortError::Io(format!(
                    "cannot write {}: {}",
                    destination.display(),
                    error.error
                )));
            }
        }
    }
    for entry in committed {
        if let Some(backup) = entry.backup {
            let _ = std::fs::remove_file(backup);
        }
    }
    Ok(())
}

fn backup_existing(destination: &Path) -> PortResult<Option<PathBuf>> {
    if std::fs::symlink_metadata(destination).is_err() {
        return Ok(None);
    }
    let parent = destination.parent().ok_or_else(|| {
        PortError::Io(format!(
            "destination {} has no parent directory",
            destination.display()
        ))
    })?;
    let temporary = tempfile::Builder::new()
        .prefix(".ostinato-backup-")
        .tempfile_in(parent)
        .map_err(|error| {
            PortError::Io(format!(
                "cannot create backup in {}: {error}",
                parent.display()
            ))
        })?;
    let backup = temporary.into_temp_path().keep().map_err(|error| {
        PortError::Io(format!(
            "cannot reserve backup for {}: {}",
            destination.display(),
            error.error
        ))
    })?;
    if let Err(error) = std::fs::rename(destination, &backup) {
        let _ = std::fs::remove_file(&backup);
        return Err(PortError::Io(format!(
            "cannot replace {}: {error}",
            destination.display()
        )));
    }
    Ok(Some(backup))
}

fn rollback(committed: &[CommitEntry]) {
    for entry in committed.iter().rev() {
        let _ = std::fs::remove_file(&entry.destination);
        if let Some(backup) = &entry.backup {
            let _ = std::fs::rename(backup, &entry.destination);
        }
    }
}

fn prepare_parent(destination: &Path) -> PortResult<()> {
    let parent = destination.parent().ok_or_else(|| {
        PortError::Io(format!(
            "destination {} has no parent directory",
            destination.display()
        ))
    })?;
    if !parent.as_os_str().is_empty() {
        std::fs::create_dir_all(parent).map_err(|error| {
            PortError::Io(format!("cannot create {}: {error}", parent.display()))
        })?;
    }
    Ok(())
}

#[cfg(unix)]
fn check_path_limits(destination: &Path) -> PortResult<()> {
    use std::os::unix::ffi::OsStrExt;

    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let name_max = rustix::fs::statvfs(parent)
        .map(|stat| stat.f_namemax)
        .unwrap_or(255);
    if let Some(name) = destination.file_name() {
        let length = name.as_bytes().len() as u64;
        if name_max > 0 && length > name_max {
            return Err(PortError::Io(format!(
                "cannot write {}: file name is {length} bytes, filesystem limit is {name_max}",
                destination.display()
            )));
        }
    }
    let path_max = if cfg!(target_vendor = "apple") {
        1024
    } else {
        4096
    };
    let length = destination.as_os_str().as_bytes().len();
    if length > path_max {
        return Err(PortError::Io(format!(
            "cannot write {}: path is {length} bytes, filesystem limit is {path_max}",
            destination.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_path_limits(_destination: &Path) -> PortResult<()> {
    Ok(())
}

fn write_stdout(content: &str) -> PortResult<WriteOutcome> {
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(content.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => Ok(WriteOutcome::Written),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
            // A downstream pager/consumer closed the pipe; treat it as success.
            Ok(WriteOutcome::Written)
        }
        Err(error) => Err(PortError::Io(format!("cannot write to stdout: {error}"))),
    }
}

fn absolute_destination(path: &Path) -> PortResult<PathBuf> {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let cwd = std::env::current_dir()
            .map_err(|error| PortError::Io(format!("cannot read current directory: {error}")))?;
        cwd.join(path)
    };
    let Some(parent) = candidate.parent() else {
        return Ok(candidate);
    };
    let parent = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    Ok(match candidate.file_name() {
        Some(name) => parent.join(name),
        None => parent,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn commit_failure_rolls_back_and_restores_backups() {
        let directory = tempfile::tempdir().expect("tempdir");
        let first = directory.path().join("first.cue");
        std::fs::write(&first, "old").expect("write old");
        let second = directory.path().join("second.cue");
        let options = OutputOptions {
            force: true,
            skip_existing: false,
        };
        let first_staged = stage_output(&first, "new", options, &[])
            .expect("stage first")
            .expect("first not skipped");
        let second_staged = stage_output(&second, "new", options, &[])
            .expect("stage second")
            .expect("second not skipped");
        // Simulate a late failure: the second destination becomes a directory
        // between staging and commit.
        std::fs::create_dir(&second).expect("mkdir");
        commit_all(vec![first_staged, second_staged], true).expect_err("commit must fail");
        assert_eq!(std::fs::read_to_string(&first).expect("read first"), "old");
        assert!(second.is_dir());
    }
}
