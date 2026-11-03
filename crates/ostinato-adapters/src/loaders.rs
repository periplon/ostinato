use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use ostinato_application::{LoadRequest, LoadedWorkflow, PortError, PortResult, SpecLoader};
use ostinato_lang::{
    Value, builtin_module, compile_json, compile_value, eval_file_with_imports_mode, parse,
    rename_file_locals,
};
use serde_json::{Value as Json, json};

/// Default Rust workflow-CUE loader. Implements the scope documented in
/// `docs/SCOPE.md`; use [`CueCliLoader`] for the full official oracle.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeLoader;

impl SpecLoader for NativeLoader {
    fn evaluator(&self) -> &'static str {
        "rust"
    }

    fn load(&self, request: &LoadRequest) -> PortResult<LoadedWorkflow> {
        let absolute = canonical(&request.source);
        let mut cache = BTreeMap::new();
        let mut stack = Vec::new();
        let root = evaluate_path(&absolute, &request.params, &mut cache, &mut stack, false)
            .map_err(|err| PortError::Loader(err.to_string()))?;
        let spec = compile_value(&root).map_err(|err| PortError::Loader(err.to_string()))?;
        let tags =
            collect_parameter_tags(&absolute).map_err(|err| PortError::Loader(err.to_string()))?;
        let params = parameters_json(&root, &request.params, &tags)
            .map_err(|err| PortError::Loader(err.to_string()))?;
        Ok(LoadedWorkflow {
            spec,
            params,
            source_file: absolute.display().to_string(),
            evaluator: "rust (CUE workflow semantics)".to_owned(),
        })
    }
}

fn evaluate_path(
    path: &Path,
    overrides: &BTreeMap<String, String>,
    cache: &mut BTreeMap<PathBuf, Value>,
    stack: &mut Vec<PathBuf>,
    include_definitions: bool,
) -> Result<Value, ostinato_lang::LangError> {
    let canonical_path = canonical(path);
    let key = if canonical_path.is_dir() {
        canonical_path.clone()
    } else {
        canonical_path
            .parent()
            .map_or_else(|| canonical_path.clone(), Path::to_path_buf)
    };
    if let Some(cached) = cache.get(&key) {
        return Ok(cached.clone());
    }
    if stack.contains(&key) {
        return Err(ostinato_lang::LangError::Workflow(format!(
            "import cycle involving {}",
            key.display()
        )));
    }
    stack.push(key.clone());

    let files = collect_package_files(&canonical_path)?;
    let source_dir = if canonical_path.is_dir() {
        canonical_path.clone()
    } else {
        canonical_path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    };
    let mut combined = ostinato_lang::ast::File {
        package: None,
        imports: Vec::new(),
        items: Vec::new(),
    };
    for (index, file) in files.iter().enumerate() {
        let text = std::fs::read_to_string(file).map_err(|err| {
            ostinato_lang::LangError::Workflow(format!("cannot read {}: {err}", file.display()))
        })?;
        let mut parsed = parse(&text)?;
        rename_file_locals(&mut parsed, index)?;
        if combined.package.is_none() {
            combined.package = parsed.package.clone();
        }
        combined.imports.extend(parsed.imports);
        combined.items.extend(parsed.items);
    }

    let mut imports = BTreeMap::new();
    for import in &combined.imports {
        if let Some(module) = builtin_module(&import.path) {
            imports.insert(import.alias.clone(), module);
            continue;
        }
        let imported_path = resolve_import(&source_dir, &import.path)?;
        let value = evaluate_path(&imported_path, &BTreeMap::new(), cache, stack, true)?;
        imports.insert(import.alias.clone(), value);
    }

    let tags = ostinato_lang::parameter_tags(&combined);
    let mut injected = BTreeMap::new();
    for (key, raw) in overrides {
        if let Some(info) = tags.get(key) {
            injected.insert(info.field.clone(), parse_override_value(raw, Some(info.ty)));
        } else {
            injected.insert(key.clone(), parse_override_value(raw, None));
        }
    }
    let value = eval_file_with_imports_mode(&combined, &imports, &injected, include_definitions)?;
    cache.insert(key, value.clone());
    stack.pop();
    Ok(value)
}

fn collect_package_files(path: &Path) -> Result<Vec<PathBuf>, ostinato_lang::LangError> {
    if path.is_dir() {
        let mut by_package: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
        let mut anonymous = Vec::new();
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(|err| {
                ostinato_lang::LangError::Workflow(format!(
                    "cannot read directory {}: {err}",
                    path.display()
                ))
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for entry in entries {
            let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !entry.is_file()
                || entry.extension().is_none_or(|ext| ext != "cue")
                || name.starts_with('.')
                || name.starts_with('_')
                || name.ends_with("_test.cue")
                || name.ends_with("_tool.cue")
            {
                continue;
            }
            let text = std::fs::read_to_string(&entry).map_err(|err| {
                ostinato_lang::LangError::Workflow(format!(
                    "cannot read {}: {err}",
                    entry.display()
                ))
            })?;
            let parsed = parse(&text)?;
            match parsed.package {
                Some(package) if package != "_" => {
                    by_package.entry(package).or_default().push(entry);
                }
                Some(_) | None => anonymous.push(entry),
            }
        }
        if by_package.len() > 1 {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "multiple packages in {}: {}",
                path.display(),
                by_package.keys().cloned().collect::<Vec<_>>().join(", ")
            )));
        }
        if let Some(files) = by_package.into_values().next() {
            return Ok(files);
        }
        if !anonymous.is_empty() {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "directory {} has no named package; pass a file instead",
                path.display()
            )));
        }
        return Err(ostinato_lang::LangError::Workflow(format!(
            "no CUE files in {}",
            path.display()
        )));
    }

    if path.extension().is_none_or(|ext| ext != "cue") {
        return Err(ostinato_lang::LangError::Workflow(format!(
            "import target is not a .cue file: {}",
            path.display()
        )));
    }
    let text = std::fs::read_to_string(path).map_err(|err| {
        ostinato_lang::LangError::Workflow(format!("cannot read {}: {err}", path.display()))
    })?;
    let parsed = parse(&text)?;
    let Some(package) = parsed.package else {
        return Ok(vec![path.to_path_buf()]);
    };
    if package == "_" {
        return Ok(vec![path.to_path_buf()]);
    }
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let mut files = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(directory)
        .map_err(|err| {
            ostinato_lang::LangError::Workflow(format!(
                "cannot read directory {}: {err}",
                directory.display()
            ))
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for entry in entries {
        let Some(name) = entry.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !entry.is_file()
            || entry.extension().is_none_or(|ext| ext != "cue")
            || name.starts_with('.')
            || name.starts_with('_')
            || name.ends_with("_test.cue")
            || name.ends_with("_tool.cue")
        {
            continue;
        }
        let text = std::fs::read_to_string(&entry).map_err(|err| {
            ostinato_lang::LangError::Workflow(format!("cannot read {}: {err}", entry.display()))
        })?;
        let parsed = parse(&text)?;
        if parsed.package.as_deref() == Some(package.as_str()) {
            files.push(entry);
        }
    }
    Ok(files)
}

fn resolve_import(
    source_dir: &Path,
    import_path: &str,
) -> Result<PathBuf, ostinato_lang::LangError> {
    if let Some(relative) = import_path.strip_prefix("./") {
        let target = safe_join(source_dir, relative)?;
        return check_import_target(&target);
    }
    let Some((module_root, module_path)) = find_module(source_dir) else {
        return Err(ostinato_lang::LangError::Workflow(format!(
            "import `{import_path}` is not a builtin and no cue.mod/module.cue was found"
        )));
    };
    let import_base = strip_version(import_path);
    let module_base = strip_version(&module_path);
    let suffix = import_base
        .strip_prefix(module_base)
        .and_then(|rest| rest.strip_prefix('/'))
        .ok_or_else(|| {
            ostinato_lang::LangError::Workflow(format!(
                "import `{import_path}` is not under module `{module_path}`"
            ))
        })?;
    let target = safe_join(&module_root, suffix)?;
    let canonical_target = canonical(&target);
    let canonical_root = canonical(&module_root);
    if !canonical_target.starts_with(&canonical_root) {
        return Err(ostinato_lang::LangError::Workflow(format!(
            "import `{import_path}` escapes the module root"
        )));
    }
    check_import_target(&canonical_target)
}

fn check_import_target(target: &Path) -> Result<PathBuf, ostinato_lang::LangError> {
    if !target.exists() {
        return Err(ostinato_lang::LangError::Workflow(format!(
            "import target does not exist: {}",
            target.display()
        )));
    }
    if target.is_file() && target.extension().is_none_or(|ext| ext != "cue") {
        return Err(ostinato_lang::LangError::Workflow(format!(
            "import target is not a .cue file: {}",
            target.display()
        )));
    }
    Ok(target.to_path_buf())
}

fn safe_join(base: &Path, suffix: &str) -> Result<PathBuf, ostinato_lang::LangError> {
    let mut out = base.to_path_buf();
    for component in suffix.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "invalid import path component `{component}`"
            )));
        }
        out.push(component);
    }
    Ok(out)
}

fn strip_version(path: &str) -> &str {
    path.split('@').next().unwrap_or(path)
}

fn find_module(start: &Path) -> Option<(PathBuf, String)> {
    let mut directory = Some(start);
    while let Some(current) = directory {
        let module_file = current.join("cue.mod").join("module.cue");
        if module_file.exists() {
            let text = std::fs::read_to_string(&module_file).ok()?;
            let file = parse(&text).ok()?;
            let value = ostinato_lang::eval_file(&file).ok()?;
            if let Value::Struct(map) | Value::Definition { fields: map, .. } = value
                && let Some(Value::String(path)) = map.get("module")
            {
                return Some((current.to_path_buf(), path.clone()));
            }
        }
        directory = current.parent();
    }
    None
}

fn parse_override_value(raw: &str, tag: Option<ostinato_lang::TagType>) -> Value {
    use ostinato_lang::TagType;
    let trimmed = raw.trim();
    match tag {
        Some(TagType::String) => return Value::String(raw.to_owned()),
        Some(TagType::Int) => {
            if let Some(value) = parse_tag_int(trimmed) {
                return Value::Int(value);
            }
        }
        Some(TagType::Bool) => match trimmed {
            "true" | "TRUE" | "True" | "t" | "T" | "1" => return Value::Bool(true),
            "false" | "FALSE" | "False" | "f" | "F" | "0" => return Value::Bool(false),
            _ => {}
        },
        Some(TagType::Number) => {
            if let Ok(value) = serde_json::from_str::<Json>(trimmed) {
                return ostinato_lang::value_from_json(&value);
            }
        }
        None => {}
    }
    let looks_like_json = trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.starts_with('"')
        || matches!(trimmed, "true" | "false" | "null")
        || trimmed.parse::<f64>().is_ok();
    if looks_like_json && let Ok(value) = serde_json::from_str::<Json>(trimmed) {
        return ostinato_lang::value_from_json(&value);
    }
    Value::String(raw.to_owned())
}

fn parse_tag_int(text: &str) -> Option<i64> {
    let text = text.trim().replace('_', "");
    let text = text.strip_prefix('+').unwrap_or(&text);
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).ok();
    }
    text.parse::<i64>().ok()
}

fn collect_parameter_tags(
    path: &Path,
) -> Result<BTreeMap<String, ostinato_lang::TagInfo>, ostinato_lang::LangError> {
    let files = collect_package_files(path)?;
    let mut tags = BTreeMap::new();
    for file in files {
        let text = std::fs::read_to_string(&file).map_err(|err| {
            ostinato_lang::LangError::Workflow(format!("cannot read {}: {err}", file.display()))
        })?;
        let parsed = ostinato_lang::parse(&text)?;
        tags.extend(ostinato_lang::parameter_tags(&parsed));
    }
    Ok(tags)
}

fn parameters_json(
    root: &Value,
    overrides: &BTreeMap<String, String>,
    tags: &BTreeMap<String, ostinato_lang::TagInfo>,
) -> Result<Json, ostinato_lang::LangError> {
    let Some(params_value) = root_value(root, "params") else {
        if overrides.is_empty() {
            return Ok(json!({}));
        }
        return Err(ostinato_lang::LangError::Workflow(
            "parameters were supplied with -p, but the workflow has no `params` struct".to_owned(),
        ));
    };
    let (Value::Struct(params) | Value::Definition { fields: params, .. }) = params_value else {
        return Err(ostinato_lang::LangError::Workflow(format!(
            "`params` must be a struct, found {}",
            params_value.type_name()
        )));
    };
    if !overrides.is_empty() {
        for key in overrides.keys() {
            let field = tags
                .get(key)
                .map_or(key.as_str(), |info| info.field.as_str());
            if !params.contains_key(field) {
                return Err(ostinato_lang::LangError::Workflow(format!(
                    "unknown parameter `{key}`; declare it under `params`"
                )));
            }
        }
    }
    let mut out = serde_json::Map::new();
    for (key, value) in params {
        if let Some(json) = value.to_json() {
            out.insert(key.clone(), json);
        } else {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "missing required parameter `{key}` (pass `-p {key}=...`)"
            )));
        }
    }
    Ok(Json::Object(out))
}

fn root_value<'a>(root: &'a Value, name: &str) -> Option<&'a Value> {
    match root {
        Value::Struct(map) | Value::Definition { fields: map, .. } => map.get(name),
        _ => None,
    }
}

/// Official CUE oracle: shells out to the reference implementation.
///
/// The containing **package** is exported (not a single file), so split
/// packages, modules and sibling files behave exactly as they do in CUE.
#[derive(Debug, Clone)]
pub struct CueCliLoader {
    executable: PathBuf,
    version: String,
}

impl CueCliLoader {
    /// Create a loader for a `cue` executable.
    ///
    /// # Errors
    /// Fails when the executable is missing, not a working CUE CLI, or older
    /// than the supported language version.
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
        let parsed = parse_version(&version).ok_or_else(|| {
            PortError::Config(format!("cannot parse CUE version from `{version}`"))
        })?;
        if parsed < (0, 17, 1) {
            return Err(PortError::Config(format!(
                "unsupported CUE version `{version}`; Ostinato requires >= v0.17.1 \
                 (install from https://cuelang.org)"
            )));
        }
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

    fn export(
        &self,
        directory: &Path,
        instance: &str,
        expression: &str,
        params: &std::collections::BTreeMap<String, String>,
    ) -> PortResult<Json> {
        let mut command = Command::new(&self.executable);
        command
            .current_dir(directory)
            .arg("export")
            .arg(instance)
            .arg("--out")
            .arg("json")
            .arg("-e")
            .arg(expression);
        for (key, value) in params {
            command.arg("-t").arg(format!("{key}={value}"));
        }
        let output = command.output().map_err(|err| {
            PortError::Loader(format!("cannot run `{}`: {err}", self.executable.display()))
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PortError::Loader(format!(
                "CUE evaluation failed (exit {}):\n{}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str(stdout.trim()).map_err(|err| {
            PortError::Loader(format!(
                "CUE produced invalid JSON for `{expression}`: {err}"
            ))
        })
    }
}

impl SpecLoader for CueCliLoader {
    fn evaluator(&self) -> &'static str {
        "cue"
    }

    fn load(&self, request: &LoadRequest) -> PortResult<LoadedWorkflow> {
        let absolute = canonical(&request.source);
        let (directory, instance) = cue_instance(&absolute)?;

        let workflow = self.export(&directory, &instance, "workflow", &request.params)?;
        let params = match self.export(&directory, &instance, "params", &request.params) {
            Ok(params) => params,
            Err(PortError::Loader(message))
                if message.contains("reference \"params\" not found") =>
            {
                if !request.params.is_empty() {
                    return Err(PortError::Loader(
                        "parameters were supplied with -p, but the workflow has no `params` struct"
                            .to_owned(),
                    ));
                }
                json!({})
            }
            Err(err) => return Err(err),
        };
        if !params.is_object() {
            return Err(PortError::Loader(format!(
                "`params` must be a struct, found {}",
                json_type_name(&params)
            )));
        }
        if !request.params.is_empty() {
            let Some(declared) = params.as_object() else {
                return Err(PortError::Loader("`params` must be a struct".to_owned()));
            };
            for key in request.params.keys() {
                if !declared.contains_key(key) {
                    return Err(PortError::Loader(format!(
                        "unknown parameter `{key}`; declare it under `params` with @tag"
                    )));
                }
            }
        }
        let spec = compile_json(&json!({ "workflow": workflow, "params": params }))
            .map_err(|err| PortError::Loader(err.to_string()))?;
        Ok(LoadedWorkflow {
            spec,
            params,
            source_file: absolute.display().to_string(),
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

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Work out the CUE instance selector and working directory.
///
/// * A directory is exported as `.` from inside it.
/// * A file is exported as `.:<package>` from its parent directory, which
///   makes CUE consider every sibling file in the package. This is what makes
///   split definitions/conflicts behave like real CUE.
fn cue_instance(path: &Path) -> PortResult<(PathBuf, String)> {
    if path.is_dir() {
        return Ok((path.to_path_buf(), ".".to_owned()));
    }
    let directory = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    match package_name(path)? {
        Some(name) if name != "_" => Ok((directory, format!(".:{name}"))),
        _ => {
            let file = path.file_name().ok_or_else(|| {
                PortError::Loader(format!("invalid source path: {}", path.display()))
            })?;
            Ok((directory, file.to_string_lossy().into_owned()))
        }
    }
}

fn package_name(path: &Path) -> PortResult<Option<String>> {
    let source = std::fs::read_to_string(path)
        .map_err(|err| PortError::Loader(format!("cannot read {}: {err}", path.display())))?;
    let mut in_block_comment = false;
    for line in source.lines() {
        let mut trimmed = line.trim_start();
        if in_block_comment {
            if let Some(end) = trimmed.find("*/") {
                in_block_comment = false;
                trimmed = &trimmed[end + 2..];
            } else {
                continue;
            }
        }
        if trimmed.starts_with("//") {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("/*") {
            if !rest.contains("*/") {
                in_block_comment = true;
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("package")
            && rest.starts_with(char::is_whitespace)
        {
            let name = rest
                .trim_start()
                .split(|c: char| c.is_whitespace() || c == '{' || c == ';')
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                return Ok(Some(name.to_owned()));
            }
        }
    }
    Ok(None)
}

fn parse_version(version: &str) -> Option<(u32, u32, u32)> {
    let token = version
        .split_whitespace()
        .find(|token| token.starts_with('v') && token.contains('.'))?;
    let core = token.trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn json_type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "null",
        Json::Bool(_) => "bool",
        Json::Number(_) => "number",
        Json::String(_) => "string",
        Json::Array(_) => "list",
        Json::Object(_) => "struct",
    }
}

#[cfg(test)]
mod tests {
    use super::parse_version;

    #[test]
    fn parses_release_pre_release_and_build_metadata() {
        assert_eq!(parse_version("cue version v0.17.1"), Some((0, 17, 1)));
        assert_eq!(
            parse_version("CUE language version v0.17.1"),
            Some((0, 17, 1))
        );
        assert_eq!(parse_version("cue version v0.17.1+meta"), Some((0, 17, 1)));
        assert_eq!(parse_version("cue version v0.18.0-rc.1"), Some((0, 18, 0)));
        assert_eq!(parse_version("cue version v0.16.1"), Some((0, 16, 1)));
    }
}
