use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use ostinato_application::{LoadRequest, LoadedWorkflow, PortError, PortResult, SpecLoader};
use ostinato_lang::{compile, compile_json};
use serde_json::{Value as Json, json};

/// Native CUE-subset loader. **Not** fully CUE compliant; used for tests and
/// offline environments. The default is [`CueCliLoader`].
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeLoader;

impl SpecLoader for NativeLoader {
    fn evaluator(&self) -> &'static str {
        "native-subset"
    }

    fn load(&self, request: &LoadRequest) -> PortResult<LoadedWorkflow> {
        let source = std::fs::read_to_string(&request.source).map_err(|err| {
            PortError::Loader(format!("cannot read {}: {err}", request.source.display()))
        })?;
        let compiled = compile(&source).map_err(|err| PortError::Loader(err.to_string()))?;
        let params = compiled
            .resolve_params(&request.params)
            .map_err(|err| PortError::Loader(err.to_string()))?;
        Ok(LoadedWorkflow {
            spec: compiled.spec,
            params,
            source_file: request.source.display().to_string(),
            evaluator: "native CUE subset (not fully compliant; use --evaluator cue)".to_owned(),
        })
    }
}

/// Fully compliant loader: shells out to the official CUE implementation.
///
/// The exported root value is compiled into a workflow by the shared typed
/// compiler. All CUE semantics (unification, defaults, comprehensions,
/// imports, modules, builtins, `@tag` injection, ...) are handled by the
/// reference implementation.
#[derive(Debug, Clone)]
pub struct CueCliLoader {
    executable: PathBuf,
    version: String,
}

impl CueCliLoader {
    /// Create a loader for a `cue` executable.
    ///
    /// # Errors
    /// Fails when the executable is missing or not a working CUE CLI.
    pub fn new(executable: Option<PathBuf>) -> PortResult<Self> {
        let executable = executable.unwrap_or_else(|| PathBuf::from("cue"));
        let output = Command::new(&executable)
            .arg("version")
            .output()
            .map_err(|err| {
                PortError::Config(format!(
                    "official CUE CLI not found (`{}`): {err}. \
                     Install CUE v0.17.1 from https://cuelang.org or pass --cue <path>.",
                    executable.display()
                ))
            })?;
        if !output.status.success() {
            return Err(PortError::Config(format!(
                "`{} version` failed: {}",
                executable.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let version = stdout
            .lines()
            .find(|line| line.starts_with("cue version") || line.starts_with("CUE language"))
            .unwrap_or(stdout.lines().next().unwrap_or("cue (unknown version)"))
            .trim()
            .to_owned();
        Ok(Self {
            executable,
            version,
        })
    }

    /// The detected CUE version string.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }
}

impl SpecLoader for CueCliLoader {
    fn evaluator(&self) -> &'static str {
        "cue"
    }

    fn load(&self, request: &LoadRequest) -> PortResult<LoadedWorkflow> {
        if !request.source.exists() {
            return Err(PortError::Loader(format!(
                "source file not found: {}",
                request.source.display()
            )));
        }
        let absolute =
            std::fs::canonicalize(&request.source).unwrap_or_else(|_| request.source.clone());
        let mut command = Command::new(&self.executable);
        if let Some(directory) = absolute.parent() {
            command.current_dir(directory);
        }
        command
            .arg("export")
            .arg(&absolute)
            .arg("--out")
            .arg("json");
        for (key, value) in &request.params {
            command.arg("-t").arg(format!("{key}={value}"));
        }
        let output = command.output().map_err(|err| {
            PortError::Loader(format!("cannot run `{}`: {err}", self.executable.display()))
        })?;
        if !output.status.success() {
            return Err(PortError::Loader(format!(
                "CUE evaluation failed:\n{}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() {
            return Err(PortError::Loader(
                "CUE produced no output; the file may not define `workflow`".to_owned(),
            ));
        }
        let root: Json = serde_json::from_str(&stdout)
            .map_err(|err| PortError::Loader(format!("CUE produced invalid JSON: {err}")))?;
        let spec = compile_json(&root).map_err(|err| PortError::Loader(err.to_string()))?;
        let params = root.get("params").cloned().unwrap_or_else(|| json!({}));
        Ok(LoadedWorkflow {
            spec,
            params,
            source_file: request.source.display().to_string(),
            evaluator: format!("official CUE ({})", self.version),
        })
    }
}

/// Convenience: create the fully compliant official loader from the
/// environment (`CUE_BIN`, default `cue`).
///
/// # Errors
/// Fails when the CUE CLI is not available.
pub fn official_loader() -> PortResult<Arc<dyn SpecLoader>> {
    let executable = std::env::var_os("CUE_BIN").map(PathBuf::from);
    Ok(Arc::new(CueCliLoader::new(executable)?))
}
