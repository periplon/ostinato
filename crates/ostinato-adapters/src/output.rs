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
        let destination = absolute_destination(path)?;
        for input in protected {
            let input = std::fs::canonicalize(input)
                .unwrap_or_else(|_| absolute_destination(input).unwrap_or_else(|_| input.clone()));
            if input == destination {
                return Err(PortError::Io(format!(
                    "refusing to write {}: it is one of the inputs",
                    destination.display()
                )));
            }
        }
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
                    return Ok(WriteOutcome::Skipped);
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
        if options.force {
            temporary.persist(&destination).map_err(|error| {
                PortError::Io(format!(
                    "cannot replace {}: {}",
                    destination.display(),
                    error.error
                ))
            })?;
        } else {
            temporary.persist_noclobber(&destination).map_err(|error| {
                PortError::Io(format!(
                    "cannot write {}: {}",
                    destination.display(),
                    error.error
                ))
            })?;
        }
        Ok(WriteOutcome::Written)
    }
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
