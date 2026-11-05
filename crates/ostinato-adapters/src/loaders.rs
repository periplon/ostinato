use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::Arc;

use bigdecimal::BigDecimal;
use num_bigint::{BigInt, ToBigInt};
use ostinato_application::{
    DataInput, DocumentEvaluator, LoadRequest, LoadedWorkflow, PortError, PortResult, SpecLoader,
};
use ostinato_lang::ast::{BytePart, CompClause, Expr, Field, Label, Span, StrPart, StructItem};
use ostinato_lang::{
    Value, builtin_module, compile_json, compile_value, eval_file_with_imports_mode,
    eval_file_with_imports_mode_allow_incomplete, eval_file_with_imports_mode_allow_outputs, parse,
    rename_file_locals, select_path,
};
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

use crate::data;

/// Default Rust workflow-CUE loader. Implements the scope documented in
/// `docs/SCOPE.md`; use [`CueCliLoader`] for the full official oracle.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeLoader;

impl DocumentEvaluator for NativeLoader {
    fn evaluator(&self) -> &'static str {
        "rust"
    }

    fn evaluate(
        &self,
        request: &LoadRequest,
        expressions: &[String],
        _list: bool,
    ) -> PortResult<Vec<Json>> {
        let (root, _) = evaluate_request(
            request,
            EvalOptions {
                include_definitions: true,
                allow_incomplete: false,
                strip_hidden_fields: false,
            },
        )?;
        if expressions.is_empty() {
            let value = root.to_json().ok_or_else(|| {
                PortError::Loader(
                    "cannot export the root value: it contains incomplete fields".to_owned(),
                )
            })?;
            return Ok(vec![value]);
        }
        let optional_paths =
            collect_optional_paths(&canonical(&request.source)).unwrap_or_default();
        let mut out = Vec::with_capacity(expressions.len());
        for expression in expressions {
            let trimmed = expression.trim();
            let selected = match select_path(&root, expression) {
                Ok(value) => value,
                Err(error) => {
                    if optional_paths.contains(trimmed) {
                        return Err(PortError::Loader(format!(
                            "cannot reference optional field: {trimmed}"
                        )));
                    }
                    return Err(PortError::Loader(error.to_string()));
                }
            };
            let value = selected.to_json().ok_or_else(|| {
                PortError::Loader(format!("cannot export `{expression}`: value is incomplete"))
            })?;
            out.push(value);
        }
        Ok(out)
    }
}

impl SpecLoader for NativeLoader {
    fn load(&self, request: &LoadRequest) -> PortResult<LoadedWorkflow> {
        let absolute = canonical(&request.source);
        let (root, tags) = evaluate_request(
            request,
            EvalOptions {
                include_definitions: false,
                allow_incomplete: false,
                strip_hidden_fields: false,
            },
        )?;
        let spec = compile_value(&root).map_err(|err| PortError::Loader(err.to_string()))?;
        let provided = provided_params(request);
        let params = parameters_json(&root, &provided, &tags)
            .map_err(|err| PortError::Loader(err.to_string()))?;
        Ok(LoadedWorkflow {
            spec,
            params,
            source_file: absolute.display().to_string(),
            evaluator: "rust (CUE workflow semantics)".to_owned(),
        })
    }
}

fn collect_optional_paths(path: &Path) -> Result<BTreeSet<String>, ostinato_lang::LangError> {
    let files = collect_package_files(path, false)?;
    let mut out = BTreeSet::new();
    for file in files {
        let text = std::fs::read_to_string(&file).map_err(|error| {
            ostinato_lang::LangError::Workflow(format!("cannot read {}: {error}", file.display()))
        })?;
        let parsed = parse(&text)?;
        collect_optional_items(&parsed.items, "", &mut out);
    }
    Ok(out)
}

fn collect_optional_items(items: &[StructItem], prefix: &str, out: &mut BTreeSet<String>) {
    for item in items {
        let StructItem::Field(field) = item else {
            continue;
        };
        let Label::Name(name, _) = &field.label else {
            continue;
        };
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        if field.optional {
            out.insert(path.clone());
        }
        if let Expr::Struct(nested, _) = &field.value {
            collect_optional_items(nested, &path, out);
        }
    }
}

fn provided_params(request: &LoadRequest) -> BTreeSet<String> {
    request.params.keys().cloned().collect()
}

fn evaluate_request(
    request: &LoadRequest,
    options: EvalOptions,
) -> PortResult<(Value, BTreeMap<String, Vec<ostinato_lang::TagInfo>>)> {
    let absolute = canonical(&request.source);
    let mut cache = BTreeMap::new();
    let mut stack = Vec::new();
    evaluate_path(
        &absolute,
        &request.params,
        EvalExtras {
            inputs: &request.inputs,
        },
        options,
        &mut cache,
        &mut stack,
    )
}

#[derive(Debug, Default, Clone, Copy)]
struct EvalExtras<'a> {
    inputs: &'a [DataInput],
}

#[derive(Debug, Default, Clone, Copy)]
struct EvalOptions {
    include_definitions: bool,
    allow_incomplete: bool,
    strip_hidden_fields: bool,
}

fn evaluate_path(
    path: &Path,
    overrides: &BTreeMap<String, String>,
    extras: EvalExtras<'_>,
    options: EvalOptions,
    cache: &mut BTreeMap<PathBuf, Value>,
    stack: &mut Vec<PathBuf>,
) -> PortResult<(Value, BTreeMap<String, Vec<ostinato_lang::TagInfo>>)> {
    let canonical_path = canonical(path);
    let key = if canonical_path.is_dir() {
        canonical_path.clone()
    } else {
        canonical_path
            .parent()
            .map_or_else(|| canonical_path.clone(), Path::to_path_buf)
    };
    if let Some(cached) = cache.get(&key) {
        return Ok((cached.clone(), BTreeMap::new()));
    }
    if stack.contains(&key) {
        return Err(PortError::Loader(format!(
            "import cycle involving {}",
            key.display()
        )));
    }
    stack.push(key.clone());

    let files = collect_package_files(&canonical_path, false)
        .map_err(|err| PortError::Loader(err.to_string()))?;
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
        let text = std::fs::read_to_string(file)
            .map_err(|err| PortError::Loader(format!("cannot read {}: {err}", file.display())))?;
        let mut parsed = parse(&text).map_err(|err| PortError::Loader(err.to_string()))?;
        rename_file_locals(&mut parsed, index).map_err(|err| PortError::Loader(err.to_string()))?;
        if combined.package.is_none() {
            combined.package = parsed.package.clone();
        }
        combined.imports.extend(parsed.imports);
        combined.items.extend(parsed.items);
    }

    let mut imports = BTreeMap::new();
    for import in &combined.imports {
        if let Some(module) = tool_module(&import.path) {
            imports.insert(import.alias.clone(), module);
            continue;
        }
        if let Some(module) = builtin_module(&import.path) {
            imports.insert(import.alias.clone(), module);
            continue;
        }
        let imported_path = resolve_import(&source_dir, &import.path)
            .map_err(|err| PortError::Loader(err.to_string()))?;
        let (value, _) = evaluate_path(
            &imported_path,
            &BTreeMap::new(),
            EvalExtras::default(),
            EvalOptions {
                include_definitions: true,
                allow_incomplete: true,
                strip_hidden_fields: true,
            },
            cache,
            stack,
        )?;
        imports.insert(import.alias.clone(), value);
    }

    if !extras.inputs.is_empty() {
        let documents = read_input_documents(extras.inputs)?;
        append_data_fields(&mut combined.items, &documents)?;
    }

    ostinato_lang::assign_field_ids(&mut combined);
    let tags = ostinato_lang::parameter_tags(&combined)
        .map_err(|err| PortError::Loader(err.to_string()))?;
    let mut injected = BTreeMap::new();
    for (key, raw) in overrides {
        if let Some(infos) = tags.get(key) {
            for info in infos {
                let value = parse_override_value(raw, Some(info.ty))
                    .map_err(|err| PortError::Loader(err.to_string()))?;
                let expr =
                    override_expr(value).map_err(|err| PortError::Loader(err.to_string()))?;
                if !inject_field(&mut combined.items, info.field_id, &expr) {
                    return Err(PortError::Loader(format!(
                        "cannot inject @tag `{key}`: declaration not found"
                    )));
                }
            }
        } else {
            let value = parse_override_value(raw, None)
                .map_err(|err| PortError::Loader(err.to_string()))?;
            injected.insert(key.clone(), value);
        }
    }
    let value = if options.allow_incomplete {
        eval_file_with_imports_mode_allow_incomplete(
            &combined,
            &imports,
            &injected,
            options.include_definitions,
        )
    } else {
        eval_file_with_imports_mode(&combined, &imports, &injected, options.include_definitions)
    }
    .map_err(|err| PortError::Loader(err.to_string()))?;
    let value = if options.strip_hidden_fields {
        ostinato_lang::strip_hidden(value)
    } else {
        value
    };
    cache.insert(key, value.clone());
    stack.pop();
    Ok((value, tags))
}

fn read_input_documents(inputs: &[DataInput]) -> PortResult<Vec<Json>> {
    let mut documents = Vec::new();
    for input in inputs {
        let file = data::DataFile::read(&input.path)
            .map_err(|error| PortError::Loader(error.to_string()))?;
        for (index, document) in file.documents.into_iter().enumerate() {
            if !document.is_object() {
                return Err(PortError::Loader(format!(
                    "{}: document {} must be an object to unify with the CUE root, found {}",
                    input.path.display(),
                    index + 1,
                    json_type_name(&document)
                )));
            }
            documents.push(document);
        }
    }
    Ok(documents)
}

fn append_data_fields(items: &mut Vec<StructItem>, documents: &[Json]) -> PortResult<()> {
    for document in documents {
        let Json::Object(fields) = document else {
            continue;
        };
        for (key, value) in fields {
            let text = serde_json::to_string(value)
                .map_err(|error| PortError::Loader(format!("cannot encode data value: {error}")))?;
            let expr = ostinato_lang::parse_expr_fragment(&text)
                .map_err(|error| PortError::Loader(error.to_string()))?;
            items.push(StructItem::Field(Field {
                id: 0,
                label: Label::Name(key.clone(), Span::default()),
                value: expr,
                optional: false,
                definition: false,
                alias: None,
                quoted: key.starts_with('_') || !is_cue_identifier(key),
                bytes_label_runes: None,
                bytes_label_multiline: false,
                attributes: Vec::new(),
                span: Span::default(),
            }));
        }
    }
    Ok(())
}

fn is_cue_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first != '_' && !first.is_ascii_alphabetic() {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn collect_package_files(
    path: &Path,
    include_tools: bool,
) -> Result<Vec<PathBuf>, ostinato_lang::LangError> {
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
                || (!include_tools && name.ends_with("_tool.cue"))
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
            || (!include_tools && name.ends_with("_tool.cue"))
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

fn parse_override_value(
    raw: &str,
    tag: Option<ostinato_lang::TagType>,
) -> Result<Value, ostinato_lang::LangError> {
    use ostinato_lang::TagType;
    let invalid =
        || ostinato_lang::LangError::Workflow(format!("invalid value `{raw}` for @tag injection"));
    let trimmed = raw.trim();
    match tag {
        Some(TagType::String) => return Ok(Value::String(raw.to_owned())),
        Some(TagType::Int) => {
            return parse_tag_int(raw).map(Value::Int).ok_or_else(invalid);
        }
        Some(TagType::Number) => {
            return parse_tag_number(raw).ok_or_else(invalid);
        }
        Some(TagType::Bool) => match trimmed {
            "true" | "TRUE" | "True" | "t" | "T" | "1" => return Ok(Value::Bool(true)),
            "false" | "FALSE" | "False" | "f" | "F" | "0" => return Ok(Value::Bool(false)),
            _ => return Err(invalid()),
        },
        None => {}
    }
    let looks_like_json = trimmed.starts_with('{')
        || trimmed.starts_with('[')
        || trimmed.starts_with('"')
        || matches!(trimmed, "true" | "false" | "null")
        || trimmed.parse::<f64>().is_ok();
    if looks_like_json && let Ok(value) = serde_json::from_str::<Json>(trimmed) {
        return Ok(ostinato_lang::value_from_json(&value));
    }
    Ok(Value::String(raw.to_owned()))
}

fn parse_tag_int(text: &str) -> Option<i64> {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (digits, radix) =
        if let Some(hex) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
            (hex, 16)
        } else if let Some(octal) = rest.strip_prefix("0o").or_else(|| rest.strip_prefix("0O")) {
            (octal, 8)
        } else if let Some(binary) = rest.strip_prefix("0b").or_else(|| rest.strip_prefix("0B")) {
            (binary, 2)
        } else {
            if rest.len() > 1 && rest.starts_with('0') {
                return None;
            }
            (rest, 10)
        };
    let digits = digits.strip_prefix('_').unwrap_or(digits);
    let digits = validate_underscores(digits)?;
    let value = i128::from_str_radix(&digits, radix).ok()?;
    let value = if negative { -value } else { value };
    i64::try_from(value).ok()
}

fn validate_underscores(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut last_underscore = false;
    for character in text.chars() {
        if character == '_' {
            if out.is_empty() || last_underscore {
                return None;
            }
            last_underscore = true;
        } else if character.is_ascii_alphanumeric() {
            out.push(character);
            last_underscore = false;
        } else {
            return None;
        }
    }
    if out.is_empty() || last_underscore {
        None
    } else {
        Some(out)
    }
}

fn parse_tag_number(text: &str) -> Option<Value> {
    if let Some(int) = parse_tag_int(text) {
        return Some(Value::Int(int));
    }
    let normalized = validate_number_literal(text)?;
    let json = serde_json::from_str::<Json>(&normalized).ok()?;
    Some(ostinato_lang::value_from_json(&json))
}

fn validate_number_literal(text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let chars_with_underscores: Vec<char> = text.chars().collect();
    for (index, character) in chars_with_underscores.iter().enumerate() {
        if *character == '_'
            && !chars_with_underscores
                .get(index + 1)
                .is_some_and(|next| next.is_ascii_digit())
        {
            return None;
        }
    }
    let cleaned: String = text.chars().filter(|character| *character != '_').collect();
    let chars: Vec<char> = cleaned.chars().collect();
    let mut index = 0usize;
    let mut out = String::with_capacity(cleaned.len() + 2);
    if matches!(chars.first(), Some('+' | '-')) {
        if chars[0] == '-' {
            out.push('-');
        }
        index = 1;
    }
    if index >= chars.len() {
        return None;
    }
    let mut digits = 0usize;
    let mut dot = false;
    let mut leading_zero = false;
    if chars[index] == '0' {
        leading_zero = true;
    }
    while index < chars.len() {
        let character = chars[index];
        match character {
            '0'..='9' => {
                out.push(character);
                digits += 1;
                index += 1;
            }
            '.' => {
                if dot {
                    return None;
                }
                dot = true;
                out.push('.');
                index += 1;
            }
            'e' | 'E' => break,
            _ => return None,
        }
    }
    if digits == 0 {
        return None;
    }
    let exponent = index < chars.len();
    if exponent {
        if chars[index] != 'e' && chars[index] != 'E' {
            return None;
        }
        out.push('e');
        index += 1;
        if matches!(chars.get(index), Some('+' | '-')) {
            out.push(chars[index]);
            index += 1;
        }
        let start = index;
        while index < chars.len() && chars[index].is_ascii_digit() {
            out.push(chars[index]);
            index += 1;
        }
        if index == start || index != chars.len() {
            return None;
        }
    }
    if leading_zero && !dot && !exponent && out.trim_start_matches('-').len() > 1 {
        return None;
    }
    if out.starts_with('.') || out.starts_with("-.") {
        out.insert(if out.starts_with('-') { 1 } else { 0 }, '0');
    }
    if out.ends_with('.') {
        out.push('0');
    }
    if out.contains(".e") {
        out = out.replace(".e", ".0e");
    }
    let sign_len = usize::from(out.starts_with('-'));
    let rest = &out[sign_len..];
    let normalized = if rest.len() > 1
        && rest.starts_with('0')
        && rest.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
    {
        let trimmed = rest.trim_start_matches('0');
        let trimmed = if trimmed.starts_with('.') {
            format!("0{trimmed}")
        } else if trimmed.is_empty() {
            "0".to_owned()
        } else {
            trimmed.to_owned()
        };
        format!("{}{}", &out[..sign_len], trimmed)
    } else {
        out
    };
    Some(normalized)
}

fn override_expr(value: Value) -> Result<Expr, ostinato_lang::LangError> {
    let json = value.to_json().ok_or_else(|| {
        ostinato_lang::LangError::Workflow("parameter value is not concrete".to_owned())
    })?;
    let text = serde_json::to_string(&json).map_err(|err| {
        ostinato_lang::LangError::Workflow(format!("cannot encode parameter value: {err}"))
    })?;
    ostinato_lang::parse_expr_fragment(&text)
}

fn inject_field(items: &mut [StructItem], id: usize, value: &Expr) -> bool {
    let span = Span::default();
    for item in items {
        match item {
            StructItem::Field(field) => {
                if field.id == id {
                    let old = field.value.clone();
                    field.value = Expr::Unify(Box::new(old), Box::new(value.clone()), span);
                    return true;
                }
                if inject_field_expr(&mut field.value, id, value) {
                    return true;
                }
            }
            StructItem::Let {
                value: let_value, ..
            }
            | StructItem::Pattern {
                value: let_value, ..
            } => {
                if inject_field_expr(let_value, id, value) {
                    return true;
                }
            }
            StructItem::For { source, body, .. } => {
                if inject_field_expr(source, id, value) {
                    return true;
                }
                if inject_field(body, id, value) {
                    return true;
                }
            }
            StructItem::If { cond, body, .. } => {
                if inject_field_expr(cond, id, value) {
                    return true;
                }
                if inject_field(body, id, value) {
                    return true;
                }
            }
            StructItem::Ellipsis(_) => {}
        }
    }
    false
}

fn inject_field_expr(expr: &mut Expr, id: usize, value: &Expr) -> bool {
    match expr {
        Expr::Struct(items, _) => inject_field(items, id, value),
        Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Bound(_, inner, _)
        | Expr::Unary(_, inner, _) => inject_field_expr(inner, id, value),
        Expr::Index(base, index, _) => {
            inject_field_expr(base, id, value) || inject_field_expr(index, id, value)
        }
        Expr::Select(base, _, _, _) => inject_field_expr(base, id, value),
        Expr::Call(callee, arguments, _) => {
            inject_field_expr(callee, id, value)
                || arguments
                    .iter_mut()
                    .any(|argument| inject_field_expr(argument, id, value))
        }
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => items
            .iter_mut()
            .any(|item| inject_field_expr(item, id, value)),
        Expr::OpenList(Some(inner), _) => inject_field_expr(inner, id, value),
        Expr::ListComp { clauses, body, .. } => {
            for clause in clauses {
                match clause {
                    ostinato_lang::ast::CompClause::For { source, .. } => {
                        if inject_field_expr(source, id, value) {
                            return true;
                        }
                    }
                    ostinato_lang::ast::CompClause::If { cond, .. } => {
                        if inject_field_expr(cond, id, value) {
                            return true;
                        }
                    }
                }
            }
            inject_field_expr(body, id, value)
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            inject_field_expr(left, id, value) || inject_field_expr(right, id, value)
        }
        Expr::Str(parts, _) => parts.iter_mut().any(|part| {
            if let ostinato_lang::ast::StrPart::Interp(inner) = part {
                inject_field_expr(inner, id, value)
            } else {
                false
            }
        }),
        Expr::BytesInterp(parts, _) => parts.iter_mut().any(|part| match part {
            ostinato_lang::ast::BytePart::Interp(inner) => inject_field_expr(inner, id, value),
            ostinato_lang::ast::BytePart::Text(_) => false,
        }),
        Expr::Null(_)
        | Expr::Bool(_, _)
        | Expr::Int(_, _)
        | Expr::BigInt(_, _)
        | Expr::Float(_, _)
        | Expr::Bytes(_, _)
        | Expr::Ident(_, _)
        | Expr::OpenList(None, _) => false,
    }
}

fn parameters_json(
    root: &Value,
    provided: &BTreeSet<String>,
    tags: &BTreeMap<String, Vec<ostinato_lang::TagInfo>>,
) -> Result<Json, ostinato_lang::LangError> {
    let Some(params_value) = root_value(root, "params") else {
        if provided.is_empty() {
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
    for key in provided {
        let known = tags.contains_key(key);
        if !known && !params.contains_key(key) {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "unknown parameter `{key}`; declare it under `params`"
            )));
        }
    }
    let mut out = serde_json::Map::new();
    for (key, value) in params {
        if key.starts_with('\u{3}') {
            continue;
        }
        let key = key.strip_prefix('\u{4}').unwrap_or(key);
        if let Some(json) = value.to_json() {
            out.insert(key.to_owned(), json);
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

    fn cue_export(&self, request: CueExport<'_>) -> PortResult<Vec<Json>> {
        let mut command = Command::new(&self.executable);
        command
            .current_dir(request.directory)
            .arg("export")
            .arg(request.instance);
        for input in request.inputs {
            command.arg(canonical(&input.path));
        }
        command.arg("--out").arg("json");
        for expression in request.expressions {
            command.arg("-e").arg(expression);
        }
        if request.list {
            command.arg("--list");
        }
        for (key, value) in request.params {
            command.arg("-t").arg(format!("{key}={value}"));
        }
        let label = if request.expressions.is_empty() {
            "<root>".to_owned()
        } else {
            request.expressions.join(",")
        };
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
        let mut values = Vec::new();
        let stream = serde_json::Deserializer::from_str(stdout.trim()).into_iter::<Json>();
        for value in stream {
            values.push(value.map_err(|err| {
                PortError::Loader(format!("CUE produced invalid JSON for `{label}`: {err}"))
            })?);
        }
        if values.is_empty() {
            return Err(PortError::Loader(format!(
                "CUE produced no JSON for `{label}`"
            )));
        }
        Ok(values)
    }
}

struct CueExport<'a> {
    directory: &'a Path,
    instance: &'a str,
    expressions: &'a [String],
    list: bool,
    params: &'a BTreeMap<String, String>,
    inputs: &'a [DataInput],
}

impl DocumentEvaluator for CueCliLoader {
    fn evaluator(&self) -> &'static str {
        "cue"
    }

    fn evaluate(
        &self,
        request: &LoadRequest,
        expressions: &[String],
        list: bool,
    ) -> PortResult<Vec<Json>> {
        let absolute = canonical(&request.source);
        let (directory, instance) = cue_instance(&absolute)?;
        self.cue_export(CueExport {
            directory: &directory,
            instance: &instance,
            expressions,
            list,
            params: &request.params,
            inputs: &request.inputs,
        })
    }
}

impl SpecLoader for CueCliLoader {
    fn load(&self, request: &LoadRequest) -> PortResult<LoadedWorkflow> {
        let absolute = canonical(&request.source);
        let (directory, instance) = cue_instance(&absolute)?;

        let workflow_expr = vec!["workflow".to_owned()];
        let workflow = self
            .cue_export(CueExport {
                directory: &directory,
                instance: &instance,
                expressions: &workflow_expr,
                list: false,
                params: &request.params,
                inputs: &request.inputs,
            })?
            .into_iter()
            .next()
            .ok_or_else(|| PortError::Loader("CUE produced no workflow value".to_owned()))?;
        let agents_expr = vec!["agents".to_owned()];
        let agents = match self.cue_export(CueExport {
            directory: &directory,
            instance: &instance,
            expressions: &agents_expr,
            list: false,
            params: &request.params,
            inputs: &request.inputs,
        }) {
            Ok(values) => values.into_iter().next().unwrap_or_else(|| json!({})),
            Err(PortError::Loader(message))
                if message.contains("reference \"agents\" not found") =>
            {
                json!({})
            }
            Err(err) => return Err(err),
        };
        let params_expr = vec!["params".to_owned()];
        let params = match self.cue_export(CueExport {
            directory: &directory,
            instance: &instance,
            expressions: &params_expr,
            list: false,
            params: &request.params,
            inputs: &request.inputs,
        }) {
            Ok(values) => values.into_iter().next().unwrap_or_else(|| json!({})),
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
        let spec = compile_json(&json!({
            "workflow": workflow,
            "params": params,
            "agents": agents,
        }))
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

/// A loaded tool package: the merged syntax, its import values and the
/// classification used by the task scheduler.
/// A command's doc-comment metadata, mirroring CUE's help derivation.
#[derive(Debug, Clone, Default)]
struct CommandDoc {
    short: Option<String>,
    usage: Option<String>,
    long: Option<String>,
}

struct ToolPackage {
    combined: ostinato_lang::ast::File,
    source_dir: PathBuf,
    imports: BTreeMap<String, Value>,
    non_task_imports: ImportKinds,
    command_docs: BTreeMap<String, CommandDoc>,
}

/// Parse a contiguous doc comment into CUE's short/usage/long shape.
fn parse_command_doc(lines: &[String]) -> CommandDoc {
    let mut collapsed = Vec::new();
    let mut previous_blank = false;
    for line in lines {
        let line = line.trim_end();
        if line.is_empty() {
            if previous_blank {
                continue;
            }
            previous_blank = true;
        } else {
            previous_blank = false;
        }
        collapsed.push(line);
    }
    let text = collapsed.join("\n");
    let text = text.trim();
    if text.is_empty() {
        return CommandDoc::default();
    }
    let (first, mut tail) = match text.split_once('\n') {
        Some((first, tail)) => (first.trim(), tail.trim()),
        None => (text, ""),
    };
    let mut doc = CommandDoc {
        short: (!first.is_empty()).then(|| first.to_owned()),
        usage: None,
        long: None,
    };
    if let Some(rest) = tail.strip_prefix("Usage:") {
        let (usage, rest) = match rest.split_once('\n') {
            Some((usage, rest)) => (usage.trim(), rest.trim()),
            None => (rest.trim(), ""),
        };
        doc.usage = (!usage.is_empty()).then(|| usage.to_owned());
        tail = rest;
    }
    doc.long = (!tail.is_empty()).then(|| tail.to_owned());
    doc
}

/// Command names declared with a preceding doc comment. CUE only applies
/// `$usage`/doc metadata when the command has documentation.
fn command_field_name(text: &str) -> Option<String> {
    let text = text.trim_start();
    if text.starts_with('{')
        || text.starts_with('[')
        || text.starts_with('@')
        || text.starts_with("//")
    {
        return None;
    }
    let (head, _) = text.split_once(':')?;
    let head = head.trim();
    if head.is_empty() {
        return None;
    }
    let head = head.trim_matches('"');
    if head.is_empty()
        || head.contains(char::is_whitespace)
        || head.starts_with('$')
        || head.starts_with('#')
    {
        return None;
    }
    Some(head.to_owned())
}

/// Approximate brace delta for a line, ignoring strings and line comments.
fn brace_delta(line: &str) -> i32 {
    let bytes = line.as_bytes();
    let mut index = 0;
    let mut delta = 0;
    let mut quote: Option<u8> = None;
    let mut triple = false;
    let mut escaped = false;
    while index < bytes.len() {
        let ch = bytes[index];
        if let Some(current) = quote {
            if escaped {
                escaped = false;
                index += 1;
                continue;
            }
            if ch == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if triple {
                if ch == current
                    && index + 2 < bytes.len()
                    && bytes[index + 1] == current
                    && bytes[index + 2] == current
                {
                    quote = None;
                    triple = false;
                    index += 3;
                    continue;
                }
            } else if ch == current {
                quote = None;
            }
            index += 1;
            continue;
        }
        match ch {
            b'"' | b'\'' => {
                if index + 2 < bytes.len() && bytes[index + 1] == ch && bytes[index + 2] == ch {
                    quote = Some(ch);
                    triple = true;
                    index += 3;
                } else {
                    quote = Some(ch);
                    index += 1;
                }
            }
            b'/' if index + 1 < bytes.len() && bytes[index + 1] == b'/' => break,
            b'{' => {
                delta += 1;
                index += 1;
            }
            b'}' => {
                delta -= 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    delta
}

/// Command names declared with a preceding doc comment. CUE only applies
/// `$usage`/doc metadata when the command has documentation. Commands may be
/// declared either directly (`command: name: { ... }`) or inside a
/// `command: { ... }` block.
fn documented_command_docs(text: &str) -> BTreeMap<String, CommandDoc> {
    let mut docs = BTreeMap::new();
    let mut comments: Vec<String> = Vec::new();
    let mut depth = 0i32;
    let mut command_child_depth: Option<i32> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(comment) = trimmed.strip_prefix("//") {
            comments.push(comment.strip_prefix(' ').unwrap_or(comment).to_owned());
            continue;
        }
        if trimmed.is_empty() {
            comments.clear();
            continue;
        }
        let line_depth = depth;
        if line_depth == 0
            && let Some(rest) = trimmed.strip_prefix("command:")
        {
            let rest = rest.trim_start();
            if rest.starts_with('{') {
                command_child_depth = Some(line_depth + 1);
            } else if let Some(name) = command_field_name(rest)
                && !comments.is_empty()
            {
                docs.insert(name, parse_command_doc(&comments));
            }
        } else if let Some(child_depth) = command_child_depth
            && line_depth == child_depth
            && let Some(name) = command_field_name(trimmed)
            && !comments.is_empty()
        {
            docs.insert(name, parse_command_doc(&comments));
        }
        comments.clear();
        depth += brace_delta(trimmed);
        if let Some(child_depth) = command_child_depth
            && depth < child_depth
        {
            command_child_depth = None;
        }
    }
    docs
}

/// Load one or more package instances (directories or `_tool.cue` files) and
/// merge them the way `cue cmd <name> [inputs...]` does.
fn load_tool_package(paths: &[PathBuf]) -> Result<ToolPackage, ostinato_lang::LangError> {
    if paths.is_empty() {
        return Err(ostinato_lang::LangError::Workflow(
            "tool command requires a package path".to_owned(),
        ));
    }
    let mut canonical_paths: Vec<PathBuf> = Vec::new();
    for path in paths {
        let canonical_path = canonical(path);
        if !canonical_paths.contains(&canonical_path) {
            canonical_paths.push(canonical_path);
        }
    }
    let first = canonical_paths
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    let source_dir = if first.is_dir() {
        first.clone()
    } else {
        first
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    };
    let mut combined = ostinato_lang::ast::File {
        package: None,
        imports: Vec::new(),
        items: Vec::new(),
    };
    let mut package_name: Option<String> = None;
    let mut command_docs: BTreeMap<String, CommandDoc> = BTreeMap::new();
    let mut seen_files = std::collections::BTreeSet::new();
    let mut index = 0;
    for path in &canonical_paths {
        for file in collect_package_files(path, true)? {
            if !seen_files.insert(file.clone()) {
                continue;
            }
            let text = std::fs::read_to_string(&file).map_err(|err| {
                ostinato_lang::LangError::Workflow(format!("cannot read {}: {err}", file.display()))
            })?;
            command_docs.extend(documented_command_docs(&text));
            let mut parsed = parse(&text)?;
            rename_file_locals(&mut parsed, index)?;
            index += 1;
            if let Some(package) = &parsed.package
                && package != "_"
            {
                match &package_name {
                    Some(existing) if existing != package => {
                        return Err(ostinato_lang::LangError::Workflow(format!(
                            "package name \"{package}\" conflicts with previous package name \"{existing}\""
                        )));
                    }
                    _ => package_name = Some(package.clone()),
                }
            }
            if combined.package.is_none() {
                combined.package = parsed.package.clone();
            }
            combined.imports.extend(parsed.imports);
            combined.items.extend(parsed.items);
        }
    }

    let mut imports = BTreeMap::new();
    for import in &combined.imports {
        if let Some(module) = tool_module(&import.path) {
            imports.insert(import.alias.clone(), module);
            continue;
        }
        if let Some(module) = builtin_module(&import.path) {
            imports.insert(import.alias.clone(), module);
            continue;
        }
        let imported_path = resolve_import(&source_dir, &import.path)?;
        let mut cache = BTreeMap::new();
        let mut stack = Vec::new();
        let value = evaluate_path(
            &imported_path,
            &BTreeMap::new(),
            EvalExtras::default(),
            EvalOptions {
                include_definitions: true,
                allow_incomplete: true,
                strip_hidden_fields: true,
            },
            &mut cache,
            &mut stack,
        )
        .map_err(|err| ostinato_lang::LangError::Workflow(err.to_string()))?
        .0;
        imports.insert(import.alias.clone(), value);
    }

    let mut non_task_imports = ImportKinds::default();
    for import in &combined.imports {
        if tool_module(&import.path).is_none() {
            non_task_imports.non_task.insert(import.alias.clone());
        }
    }
    for (alias, value) in &imports {
        let mut paths = std::collections::BTreeSet::new();
        collect_task_selector_paths(value, &mut Vec::new(), &mut paths);
        if !paths.is_empty() {
            non_task_imports.task_paths.insert(alias.clone(), paths);
        }
    }

    Ok(ToolPackage {
        combined,
        source_dir,
        imports,
        non_task_imports,
        command_docs,
    })
}

/// Metadata surfaced by `$usage`, `$short` and `$long` on a tool command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCommandInfo {
    pub name: String,
    pub usage: Option<String>,
    pub short: Option<String>,
    pub long: Option<String>,
}

/// Load the `$usage`/`$short`/`$long` metadata for every command in a tool
/// package. Matches CUE's command help lookup: `$usage` (or `usage`),
/// `$short`, and `$long`.
///
/// # Errors
/// Returns a language or workflow error for unreadable packages.
pub fn tool_command_metadata(
    paths: &[PathBuf],
) -> Result<Vec<ToolCommandInfo>, ostinato_lang::LangError> {
    let package = load_tool_package(paths)?;
    let injected = BTreeMap::new();
    let metadata_names = command_names_in_items(&package.combined.items);
    let mut metadata_file = package.combined.clone();
    metadata_file.items = prune_root_commands(&package.combined.items, &metadata_names, true);
    metadata_file.items = prune_unreferenced_lets(&metadata_file.items);
    let aliases: std::collections::BTreeSet<String> = metadata_file
        .imports
        .iter()
        .map(|import| import.alias.clone())
        .collect();
    let used = used_import_aliases(&metadata_file.items, &aliases);
    metadata_file
        .imports
        .retain(|import| used.contains(&import.alias));
    let value = eval_file_with_imports_mode_allow_incomplete(
        &metadata_file,
        &package.imports,
        &injected,
        false,
    )?;
    let Some(root) = value_fields(&value) else {
        return Ok(Vec::new());
    };
    let Some(command) = root.get("command").and_then(value_fields) else {
        return Ok(Vec::new());
    };
    let mut infos = Vec::new();
    for (name, value) in command {
        if name.starts_with('$') || name.starts_with('\u{3}') || name.starts_with('#') {
            continue;
        }
        let Some(fields) = value_fields(value) else {
            continue;
        };
        let lookup = |key: &str| match fields.get(key).map(Value::concretize) {
            Some(Value::String(text)) => Some(text.trim().to_owned()),
            _ => None,
        };
        let doc = package.command_docs.get(name);
        infos.push(ToolCommandInfo {
            name: name.clone(),
            usage: doc.and_then(|doc| {
                lookup("$usage")
                    .or_else(|| lookup("usage"))
                    .or_else(|| doc.usage.clone())
            }),
            short: lookup("$short")
                .or_else(|| doc.and_then(|doc| lookup("short").or_else(|| doc.short.clone()))),
            long: lookup("$long")
                .or_else(|| doc.and_then(|doc| lookup("long").or_else(|| doc.long.clone()))),
        });
    }
    Ok(infos)
}

/// Run a CUE tool command from one or more package instances (the `inputs` of
/// `cue cmd <name> [inputs...]`).
///
/// # Errors
/// Returns a language or workflow error for unreadable packages, unknown
/// commands, malformed tasks or unsupported task kinds.
pub fn run_tool_command_paths(
    paths: &[PathBuf],
    command_name: &str,
    overrides: &BTreeMap<String, String>,
) -> Result<i32, ostinato_lang::LangError> {
    let ToolPackage {
        mut combined,
        source_dir,
        mut imports,
        non_task_imports,
        ..
    } = load_tool_package(paths)?;
    ostinato_lang::assign_field_ids(&mut combined);
    sanitize_after_fields(&mut combined.items, &non_task_imports, &imports);
    let tags = ostinato_lang::parameter_tags(&combined)?;
    let injected = BTreeMap::new();
    for (key, raw) in overrides {
        let Some(infos) = tags.get(key) else {
            eprintln!("no tag for \"{key}\"");
            return Ok(1);
        };
        for info in infos {
            let value = parse_override_value(raw, Some(info.ty))?;
            let expr = override_expr(value)?;
            if !inject_field(&mut combined.items, info.field_id, &expr) {
                return Err(ostinato_lang::LangError::Workflow(format!(
                    "cannot inject @tag `{key}`: declaration not found"
                )));
            }
        }
    }

    let keep_commands = selected_command_scope(&combined.items, command_name);
    combined.items = prune_root_commands(&combined.items, &keep_commands, false);
    combined.items = prune_unreferenced_lets(&combined.items);
    let mut selected_values = Vec::new();
    for item in &combined.items {
        if let StructItem::Field(field) = item
            && matches!(&field.label, Label::Name(name, _) if name == "command")
        {
            command_values_for_name(&field.value, command_name, &mut selected_values);
        }
    }
    if selected_values.iter().any(|value| {
        expr_uses_imported_task_struct(
            value,
            &[],
            &combined.items,
            &imports,
            &non_task_imports,
            &mut std::collections::BTreeSet::new(),
        )
    }) {
        return Err(ostinato_lang::LangError::Workflow(
            "tool command alias from an imported module contains tool/os tasks; this shape is not supported"
                .to_owned(),
        ));
    }
    apply_os_value_schema(&mut combined.items, command_name, &imports);
    // Schema expansion can replace the last reference to an imported value;
    // drop declarations that are no longer referenced by the pruned package.
    let aliases: std::collections::BTreeSet<String> = combined
        .imports
        .iter()
        .map(|import| import.alias.clone())
        .collect();
    let used = used_import_aliases(&combined.items, &aliases);
    combined
        .imports
        .retain(|import| used.contains(&import.alias));
    let task_items = command_task_items(&combined.items, command_name)?;
    let probe =
        eval_file_with_imports_mode_allow_incomplete(&combined, &imports, &injected, false)?;
    let probe_tasks = extract_command_tasks(&probe, command_name)?;
    let mut probe_scheduled = Vec::new();
    let mut probe_scopes = Vec::new();
    collect_schedule(
        &probe_tasks,
        Some(&task_items),
        &combined.items,
        &mut probe_scopes,
        &mut probe_scheduled,
    )?;
    let mut task_name_set: std::collections::HashSet<String> = probe_scheduled
        .iter()
        .map(|(name, _, _)| name.clone())
        .collect();
    // Tasks declared outside the command but referenced through output reads
    // (e.g. `let t = base` or `#base`) are scheduled too.
    let mut external_paths: std::collections::BTreeSet<Vec<String>> =
        std::collections::BTreeSet::new();
    let mut plain_read_paths: std::collections::BTreeSet<Vec<String>> =
        std::collections::BTreeSet::new();
    {
        let mut scopes = Vec::new();
        collect_output_task_paths(
            &Expr::Struct(task_items.clone(), Span::default()),
            &mut scopes,
            &combined.items,
            &mut external_paths,
            &mut plain_read_paths,
            false,
            &mut std::collections::BTreeSet::new(),
            &non_task_imports,
            &imports,
        );
    }
    plain_read_paths.retain(|path| {
        if find_field_expr_at_path(&combined.items, path).is_some_and(|value| {
            is_task_expr(
                value,
                &[combined.items.as_slice()],
                &combined.items,
                &mut std::collections::BTreeSet::new(),
                &non_task_imports,
            )
        }) {
            return true;
        }
        path.split_first().is_some_and(|(root_name, rest)| {
            imports
                .get(root_name)
                .and_then(|value| module_task_value(value, rest))
                .and_then(value_fields)
                .is_some_and(|fields| fields.contains_key("$id"))
        })
    });
    let module_input_candidates = external_paths.clone();
    external_paths.retain(|path| {
        let Some(leaf) = path.last() else {
            return false;
        };
        if path.len() == 1 {
            if probe_scheduled
                .iter()
                .any(|(scheduled_name, _, _)| scheduled_name == leaf)
            {
                return false;
            }
            if root_bare_module_task(&combined.items, leaf, &non_task_imports) {
                return false;
            }
        }
        find_field_expr_at_path(&combined.items, path).is_some_and(|value| {
            let scoped = scopes_for_path(&combined.items, path);
            let refs: Vec<&[StructItem]> = scoped.iter().map(|(items, _)| *items).collect();
            is_task_expr(
                value,
                &refs,
                &combined.items,
                &mut std::collections::BTreeSet::new(),
                &non_task_imports,
            )
        })
    });
    // Command task fields that alias a package or module task (`r: prep`,
    // `r: lib.run`).
    let command_aliases: std::collections::BTreeMap<String, CommandAliasTarget> = {
        let command_scopes: Vec<&[StructItem]> = vec![task_items.as_slice()];
        let mut aliases = std::collections::BTreeMap::new();
        collect_command_task_package_paths(
            &Expr::Struct(task_items.clone(), Span::default()),
            &command_scopes,
            &combined.items,
            &non_task_imports,
            &imports,
            &mut aliases,
        );
        aliases
    };
    let command_claimed: std::collections::BTreeSet<Vec<String>> = command_aliases
        .values()
        .flat_map(|target| target.module.iter().chain(target.local.iter()))
        .cloned()
        .collect();
    let order = package_read_order(
        task_items.as_slice(),
        &combined.items,
        &non_task_imports,
        &command_claimed,
    );
    // A declaration-first task-input read keeps the target non-concrete in
    // CUE, even when a later plain read could be satisfied by the alias
    // instance; plain reads must not materialise the target in that case.
    let declaration_first_targets: std::collections::BTreeSet<Vec<String>> = order
        .iter()
        .filter_map(
            |(path, (declaration, _, task_read))| match (declaration, task_read) {
                (Some(declaration), Some(read)) if read > declaration => Some(path.clone()),
                _ => None,
            },
        )
        .collect();
    plain_read_paths.retain(|path| !declaration_first_targets.contains(path));
    let plain_alias_targets: std::collections::BTreeSet<Vec<String>> = command_claimed
        .intersection(&plain_read_paths)
        .cloned()
        .collect();
    external_paths.retain(|path| {
        if !command_claimed.contains(path) {
            return true;
        }
        match order.get(path) {
            Some((Some(declaration), _, Some(read))) => read <= declaration,
            Some((Some(declaration), Some(read), None)) => read <= declaration,
            _ => true,
        }
    });

    // A command-alias field read (`a.stdout` where `a: p3`) materialises the
    // concrete task at the end of a longer chain, just like a package read.
    let mut command_field_reads = std::collections::BTreeSet::new();
    let mut command_field_read_paths = std::collections::BTreeSet::new();
    for item in &task_items {
        if let StructItem::Field(field) = item {
            collect_command_field_output_reads(
                &field.value,
                &mut command_field_reads,
                &mut command_field_read_paths,
            );
        }
    }
    for (name, target) in &command_aliases {
        let Some(local) = &target.local else {
            continue;
        };
        let read_as_target =
            command_field_reads.contains(name) || command_field_read_paths.contains(local);
        if !read_as_target {
            continue;
        }
        // Direct aliases keep CUE's declaration-first non-concrete behaviour.
        if alias_chain_nodes(&combined.items, local).len() <= 2 {
            continue;
        }
        let root = task_read_root(&combined.items, local);
        if path_is_module_terminal(&combined.items, &imports, &root) {
            continue;
        }
        external_paths.insert(root);
    }

    // A task-input output read materialises the concrete task it reaches
    // through pure alias and `let` hops. CUE runs `\(p5.stdout)` once (at the
    // underlying task), while a composed declaration (`p2: base & {...}`) is
    // its own instance.
    let task_input_read_paths = external_paths.clone();
    let task_input_roots: std::collections::BTreeSet<Vec<String>> = external_paths
        .iter()
        .map(|path| task_read_root(&combined.items, path))
        .collect();
    external_paths = task_input_roots.clone();

    // CUE materialises every other package alias hop as its own task instance,
    // so `p5: p4; p4: p3; p3: p2; p2: base` runs `p4` and `p2` in addition to
    // the command alias task itself.
    let alias_instance_paths: std::collections::BTreeSet<Vec<String>> = command_aliases
        .values()
        .filter_map(|target| target.local.as_ref())
        .flat_map(|local| alias_chain_instances(&combined.items, local))
        .collect();
    let alias_instance_roots: std::collections::BTreeSet<Vec<String>> = alias_instance_paths
        .iter()
        .filter_map(|local| alias_instance_root(&combined.items, local))
        .collect();
    let definition_alias_roots: std::collections::BTreeSet<Vec<String>> = alias_instance_paths
        .iter()
        .filter_map(|local| {
            let root = alias_target_at_path(&combined.items, local)?;
            definition_root(&combined.items, &root).then_some(root)
        })
        .collect();
    external_paths
        .retain(|path| !definition_alias_roots.contains(path) || task_input_roots.contains(path));
    plain_read_paths.extend(alias_instance_roots.iter().cloned());
    external_paths.extend(alias_instance_paths.iter().cloned());

    // CUE rejects direct module task-output reads inside task inputs; fail
    // before evaluation so no task executes.
    let module_backed = |path: &[String]| {
        if path.first().is_some_and(|root| imports.contains_key(root)) {
            return true;
        }
        let Some(first) = path.first() else {
            return false;
        };
        if alias_instance_roots.contains(path) {
            return false;
        }
        if matches!(
            local_binding_location(&[task_items.as_slice()], &combined.items, first),
            Some((false, _))
        ) {
            return false;
        }
        match local_binding_location(&[combined.items.as_slice()], &combined.items, first) {
            Some((true, StructItem::Field(field))) if field.definition => return false,
            Some((true, _)) => {}
            _ => return false,
        }
        // Composing the field with a struct literal makes CUE schedule the
        // instance instead of deadlocking on a bare module reference.
        if root_field_composed(&combined.items, first) {
            return false;
        }
        // A field aliasing another local field is its own instance.
        if find_field_expr_at_path_in_root(&combined.items, path, &combined.items)
            .is_some_and(|value| expr_roots_local_binding(&combined.items, value))
        {
            return false;
        }
        find_field_expr_at_path_in_root(&combined.items, path, &combined.items).is_some_and(
            |value| {
                module_task_path_scoped(
                    value,
                    &[combined.items.as_slice()],
                    &combined.items,
                    &imports,
                    &mut std::collections::BTreeSet::new(),
                )
                .is_some()
            },
        )
    };
    if module_input_candidates
        .iter()
        .any(|path| module_backed(path))
    {
        return Err(ostinato_lang::LangError::Workflow(
            "tool task inputs cannot read module task outputs".to_owned(),
        ));
    }

    // Package-level tasks reachable from command `$after` operands or package
    // output reads (top-level fields, definitions, nested containers).
    let mut package_tasks: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();
    let mut alias_dependencies: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    {
        let mut queue: std::collections::VecDeque<Vec<String>> = std::collections::VecDeque::new();
        {
            let command_scopes: Vec<&[StructItem]> = vec![task_items.as_slice()];
            let mut paths = Vec::new();
            collect_package_after_paths(
                &Expr::Struct(task_items.clone(), Span::default()),
                &command_scopes,
                &combined.items,
                &non_task_imports,
                false,
                &mut paths,
            )?;
            for path in paths {
                queue.push_back(path);
            }
        }
        for path in &external_paths {
            if is_package_root_path(path, &combined.items) {
                queue.push_back(path.clone());
            }
        }
        // A command alias runs the target task instance, so the target is not
        // scheduled again; only its own `$after` prerequisites are.
        for (task_name, target) in &command_aliases {
            let Some(path) = &target.local else {
                continue;
            };
            let Some(expr) = find_field_expr_at_path(&combined.items, path) else {
                continue;
            };
            let scopes = scopes_for_path(&combined.items, path);
            let mut dependency_paths = Vec::new();
            collect_package_task_after_paths(
                expr,
                &scopes,
                &combined.items,
                &non_task_imports,
                true,
                &mut std::collections::BTreeSet::new(),
                &mut dependency_paths,
            )?;
            let mut dependencies = Vec::new();
            for dependency_path in dependency_paths {
                let dependency = package_task_key(&dependency_path);
                if !dependencies.contains(&dependency) {
                    dependencies.push(dependency.clone());
                }
                queue.push_back(dependency_path);
            }
            alias_dependencies.insert(task_name.clone(), dependencies);
        }
        let mut seen = std::collections::BTreeSet::new();
        while let Some(path) = queue.pop_front() {
            let key = package_task_key(&path);
            if !seen.insert(key.clone()) {
                continue;
            }
            let Some(expr) = find_field_expr_at_path(&combined.items, &path) else {
                continue;
            };
            let scopes = scopes_for_path(&combined.items, &path);
            let scope_items = scopes
                .last()
                .map_or(combined.items.as_slice(), |(items, _)| *items);
            if !is_task_expr(
                expr,
                &[scope_items],
                &combined.items,
                &mut std::collections::BTreeSet::new(),
                &non_task_imports,
            ) {
                continue;
            }
            let mut dependency_paths = Vec::new();
            collect_package_task_after_paths(
                expr,
                &scopes,
                &combined.items,
                &non_task_imports,
                true,
                &mut std::collections::BTreeSet::new(),
                &mut dependency_paths,
            )?;
            let mut dependencies = Vec::new();
            for dependency_path in dependency_paths {
                let dependency = package_task_key(&dependency_path);
                if !dependencies.contains(&dependency) {
                    dependencies.push(dependency.clone());
                }
                queue.push_back(dependency_path);
            }
            package_tasks.push((key, path, dependencies));
        }
    }
    for path in external_paths
        .iter()
        .chain(plain_read_paths.iter())
        .chain(package_tasks.iter().map(|(_, path, _)| path))
    {
        if let Some(leaf) = path.last() {
            task_name_set.insert(leaf.clone());
        }
    }
    // A read such as `\(p2.stdout)` may be canonicalised to a definition-rooted
    // path (`#base`). Allow interpolation through the alias hops that actually
    // carry a read, so consumers wait for injection. Hops without a read keep
    // CUE's eager non-concrete error (for example a declaration-first task
    // input through a definition alias).
    let mut package_paths = Vec::new();
    collect_field_paths(&combined.items, &[], &mut package_paths);
    for path in &package_paths {
        let nodes = alias_chain_nodes(&combined.items, path);
        if nodes.len() < 2 {
            continue;
        }
        let carries_read = nodes.iter().any(|node| {
            if plain_read_paths.contains(node) {
                return true;
            }
            if !task_input_read_paths.contains(node) {
                return false;
            }
            !read_through_definition_module(&combined.items, &imports, node)
        });
        if !carries_read {
            continue;
        }
        for node in nodes {
            if let Some(leaf) = node.last() {
                task_name_set.insert(leaf.clone());
            }
        }
    }
    // Command-alias targets that carry a read keep every hop incomplete until
    // injection. Direct declaration-first task inputs and definition-to-module
    // aliases keep CUE's eager non-concrete error.
    for (name, target) in &command_aliases {
        let Some(local) = &target.local else {
            continue;
        };
        if path_is_module_terminal(&combined.items, &imports, local) {
            continue;
        }
        let nodes = alias_chain_nodes(&combined.items, local);
        let package_read = command_field_read_paths.contains(local) && nodes.len() > 2;
        if declaration_first_targets.contains(local)
            && !command_field_reads.contains(name)
            && !package_read
        {
            continue;
        }
        if read_through_definition_module(&combined.items, &imports, local) {
            continue;
        }
        for node in nodes {
            if let Some(leaf) = node.last() {
                task_name_set.insert(leaf.clone());
            }
        }
    }
    // Reads routed through a definition that aliases a plain module-backed
    // field stay non-concrete in CUE; do not let a same-named nested task
    // label keep the interpolation incomplete.
    for path in &package_paths {
        if read_through_definition_module(&combined.items, &imports, path)
            && let Some(leaf) = path.last()
        {
            task_name_set.remove(leaf);
        }
    }
    let value = eval_file_with_imports_mode_allow_outputs(
        &combined,
        &imports,
        &injected,
        false,
        &task_name_set,
    )?;
    let mut tasks = extract_command_tasks(&value, command_name)?;

    let mut scheduled = Vec::new();
    let mut schedule_scopes = Vec::new();
    collect_schedule(
        &tasks,
        Some(&task_items),
        &combined.items,
        &mut schedule_scopes,
        &mut scheduled,
    )?;
    if scheduled.is_empty() {
        return Err(ostinato_lang::LangError::Workflow(format!(
            "tool command `{command_name}` has no tasks"
        )));
    }
    add_alias_dependencies(&mut scheduled, &alias_dependencies);
    let mut external_by_key: BTreeMap<String, Vec<String>> = external_paths
        .iter()
        .map(|path| (external_task_key(path, &combined.items), path.clone()))
        .collect();
    for (key, path, _) in &package_tasks {
        external_by_key.insert(key.clone(), path.clone());
    }
    let mut external_scheduled: Vec<(String, Vec<String>, BTreeMap<String, Value>)> = Vec::new();
    if !external_by_key.is_empty() {
        let full = Some(eval_file_with_imports_mode_allow_outputs(
            &combined,
            &imports,
            &injected,
            true,
            &task_name_set,
        )?);
        let source = full.as_ref().unwrap_or(&value);
        for (key, path) in &external_by_key {
            if scheduled
                .iter()
                .any(|(scheduled_name, _, _)| scheduled_name == key)
            {
                continue;
            }
            let fields = value_find_path(source, path)
                .or_else(|| path.last().and_then(|leaf| value_find_named(source, leaf)))
                .and_then(value_fields);
            if let Some(fields) = fields
                && fields.contains_key("$id")
            {
                let dependencies = package_tasks
                    .iter()
                    .find(|(task_key, _, _)| task_key == key)
                    .map(|(_, _, dependencies)| dependencies.clone())
                    .unwrap_or_default();
                external_scheduled.push((key.clone(), dependencies, fields.clone()));
            }
        }
        let mut merged = external_scheduled.clone();
        merged.extend(scheduled.iter().cloned());
        scheduled = merged;
    }
    let mut task_names = std::collections::BTreeSet::new();
    for (name, _, _) in &scheduled {
        if !task_names.insert(name.clone()) {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "ambiguous tool task name `{name}`"
            )));
        }
    }
    add_output_dependencies(&combined.items, &mut scheduled, &non_task_imports);
    for (name, dependencies, _) in &scheduled {
        for dependency in dependencies {
            if !task_names.contains(dependency) {
                return Err(ostinato_lang::LangError::Workflow(format!(
                    "task `{name}` has unknown `$after` task `{dependency}`"
                )));
            }
        }
    }

    let tool_work_dir = std::env::current_dir().unwrap_or_else(|_| source_dir.clone());
    let mut flat: BTreeMap<String, BTreeMap<String, Value>> = scheduled
        .iter()
        .map(|(name, _, task)| (name.clone(), task.clone()))
        .collect();
    validate_task_inputs(&flat)?;
    let mut done = std::collections::BTreeSet::new();
    let mut failed = std::collections::BTreeSet::new();
    let mut last_reason = None;
    while done.len() < scheduled.len() {
        let mut progress = false;
        let mut updated = false;
        for (name, dependencies, _) in &scheduled {
            if done.contains(name) {
                continue;
            }
            if dependencies
                .iter()
                .any(|dependency| failed.contains(dependency))
            {
                done.insert(name.clone());
                failed.insert(name.clone());
                progress = true;
                continue;
            }
            if !dependencies
                .iter()
                .all(|dependency| done.contains(dependency))
            {
                continue;
            }
            let Some(task) = flat.get(name) else {
                continue;
            };
            match execute_tool_task(task, &tool_work_dir)? {
                ToolTaskOutcome::NotReady(reason) => last_reason = Some(reason.to_owned()),
                ToolTaskOutcome::Done { outputs } => {
                    done.insert(name.clone());
                    progress = true;
                    let mut injected_ok = if let Some(path) = external_by_key.get(name) {
                        if alias_instance_paths.contains(path) {
                            let mut injected =
                                inject_named_outputs_any(&mut combined.items, name, path, &outputs);
                            if let Some(root) = alias_instance_root(&combined.items, path) {
                                injected |= inject_named_outputs_any(
                                    &mut combined.items,
                                    root.last().map_or(name, String::as_str),
                                    &root,
                                    &outputs,
                                );
                            }
                            injected
                        } else {
                            inject_named_outputs(
                                &mut combined.items,
                                command_name,
                                name,
                                path,
                                &outputs,
                                &non_task_imports,
                            )
                        }
                    } else {
                        inject_command_outputs(
                            &mut combined.items,
                            command_name,
                            name,
                            &outputs,
                            &non_task_imports,
                        )
                    };
                    if let Some(target) = command_aliases.get(name) {
                        if let Some(local) = &target.local
                            && plain_alias_targets.contains(local)
                        {
                            let target_name = local.last().cloned().unwrap_or_default();
                            let injected = if alias_instance_paths.contains(local) {
                                let mut injected = inject_named_outputs_any(
                                    &mut combined.items,
                                    &target_name,
                                    local,
                                    &outputs,
                                );
                                if let Some(root) = alias_instance_root(&combined.items, local) {
                                    injected |= inject_named_outputs_any(
                                        &mut combined.items,
                                        root.last().map_or(&target_name, String::as_str),
                                        &root,
                                        &outputs,
                                    );
                                }
                                injected
                            } else {
                                inject_named_outputs(
                                    &mut combined.items,
                                    command_name,
                                    &target_name,
                                    local,
                                    &outputs,
                                    &non_task_imports,
                                )
                            };
                            if injected {
                                injected_ok = true;
                            }
                        }
                        if let Some(module) = &target.module
                            && plain_alias_targets.contains(module)
                            && inject_module_outputs(&mut imports, module, &outputs)
                        {
                            injected_ok = true;
                        }
                    }
                    if !outputs.is_empty() && injected_ok {
                        let value = eval_file_with_imports_mode_allow_outputs(
                            &combined,
                            &imports,
                            &injected,
                            false,
                            &task_name_set,
                        )?;
                        tasks = extract_command_tasks(&value, command_name)?;
                        let mut rescheduled = Vec::new();
                        let mut reschedule_scopes = Vec::new();
                        collect_schedule(
                            &tasks,
                            Some(&task_items),
                            &combined.items,
                            &mut reschedule_scopes,
                            &mut rescheduled,
                        )?;
                        add_alias_dependencies(&mut rescheduled, &alias_dependencies);
                        let mut merged = external_scheduled.clone();
                        merged.extend(rescheduled.iter().cloned());
                        rescheduled = merged;
                        add_output_dependencies(
                            &combined.items,
                            &mut rescheduled,
                            &non_task_imports,
                        );
                        flat = rescheduled
                            .iter()
                            .map(|(name, _, task)| (name.clone(), task.clone()))
                            .collect();
                        validate_task_inputs(&flat)?;
                        scheduled = rescheduled;
                        updated = true;
                        break;
                    }
                }
                ToolTaskOutcome::Failed => {
                    done.insert(name.clone());
                    failed.insert(name.clone());
                    progress = true;
                }
            }
        }
        if updated {
            continue;
        }
        if !progress {
            if !failed.is_empty() {
                return Ok(1);
            }
            return Err(ostinato_lang::LangError::Workflow(last_reason.unwrap_or_else(|| {
                "tool task inputs are not concrete; dependent task outputs are a documented limit"
                    .to_owned()
            })));
        }
    }
    Ok(i32::from(!failed.is_empty()))
}

fn value_fields(value: &Value) -> Option<&BTreeMap<String, Value>> {
    match value {
        Value::Struct(fields) | Value::Definition { fields, .. } => Some(fields),
        _ => None,
    }
}

/// `tool/os` tasks expose `Value: bool | number | *string | null` for every
/// regular field. The constraint is validated per scheduled task (see
/// `validate_task_inputs`) rather than as a marker pattern: CUE's flow only
/// evaluates the requested command, while Ostinato evaluates the package
/// eagerly and an invalid field in an unrelated command must not fail it.
fn os_task_marker(id: &str) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert("$id".to_owned(), Value::String(id.to_owned()));
    Value::Struct(fields)
}

fn http_task_marker(method: Option<&str>) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert("$id".to_owned(), Value::String("tool/http.Do".to_owned()));
    match method {
        Some(method) => {
            fields.insert("method".to_owned(), Value::String(method.to_owned()));
        }
        None => {
            fields.insert(
                "method".to_owned(),
                Value::Type(ostinato_lang::TypeKind::String),
            );
        }
    }
    fields.insert(
        "url".to_owned(),
        Value::Type(ostinato_lang::TypeKind::String),
    );
    fields.insert(
        "followRedirects".to_owned(),
        Value::Disjunction(
            vec![
                Value::Bool(true),
                Value::Type(ostinato_lang::TypeKind::Bool),
            ],
            vec![0],
        ),
    );
    let mut response = BTreeMap::new();
    response.insert(
        "status".to_owned(),
        Value::Type(ostinato_lang::TypeKind::String),
    );
    response.insert(
        "statusCode".to_owned(),
        Value::Type(ostinato_lang::TypeKind::Int),
    );
    response.insert(
        "body".to_owned(),
        Value::Disjunction(
            vec![
                Value::Type(ostinato_lang::TypeKind::Bytes),
                Value::Type(ostinato_lang::TypeKind::String),
            ],
            vec![0],
        ),
    );
    response.insert("header".to_owned(), Value::Struct(BTreeMap::new()));
    response.insert("trailer".to_owned(), Value::Struct(BTreeMap::new()));
    fields.insert("response".to_owned(), Value::Struct(response));
    Value::Struct(fields)
}

fn tool_module(path: &str) -> Option<Value> {
    let mut exports = BTreeMap::new();
    let marker = |id: &str| {
        let mut fields = BTreeMap::new();
        fields.insert("$id".to_owned(), Value::String(id.to_owned()));
        Value::Struct(fields)
    };
    match path {
        "tool/exec" => {
            let mut run = match marker("tool/exec.Run") {
                Value::Struct(fields) => fields,
                _ => unreachable!(),
            };
            run.insert(
                "success".to_owned(),
                Value::Type(ostinato_lang::TypeKind::Bool),
            );
            exports.insert("Run".to_owned(), Value::Struct(run));
        }
        "tool/cli" => {
            exports.insert("Print".to_owned(), marker("tool/cli.Print"));
        }
        "tool/file" => {
            exports.insert("Append".to_owned(), marker("tool/file.Append"));
            exports.insert("Create".to_owned(), marker("tool/file.Create"));
        }
        "tool/os" => {
            exports.insert("Getenv".to_owned(), os_task_marker("tool/os.Getenv"));
            exports.insert("Environ".to_owned(), os_task_marker("tool/os.Environ"));
            exports.insert("Setenv".to_owned(), os_task_marker("tool/os.Setenv"));
            exports.insert("Clearenv".to_owned(), os_task_marker("tool/os.Clearenv"));
        }
        "tool/http" => {
            exports.insert("Do".to_owned(), http_task_marker(None));
            exports.insert("Get".to_owned(), http_task_marker(Some("GET")));
            exports.insert("Post".to_owned(), http_task_marker(Some("POST")));
            exports.insert("Put".to_owned(), http_task_marker(Some("PUT")));
            exports.insert("Delete".to_owned(), http_task_marker(Some("DELETE")));
        }
        _ => return None,
    }
    Some(Value::Struct(exports))
}

/// `tool/os`'s `Value` schema: `bool | number | *string | null`.
fn os_value_schema(include_string_default: bool) -> Expr {
    let span = Span::default();
    let bool_ty = Expr::Ident("bool".to_owned(), span);
    let number_ty = Expr::Ident("number".to_owned(), span);
    let string = Expr::Ident("string".to_owned(), span);
    let string_conjunct = if include_string_default {
        Expr::Default(Box::new(string), span)
    } else {
        string
    };
    let null = Expr::Null(span);
    let scalar = Expr::Disjunction(
        Box::new(Expr::Disjunction(
            Box::new(bool_ty),
            Box::new(number_ty),
            span,
        )),
        Box::new(string_conjunct),
        span,
    );
    Expr::Disjunction(Box::new(scalar), Box::new(null), span)
}

/// CUE resolves a concrete string default (for example `*"d"`) against the
/// schema's `*string` by keeping the concrete value. Model that by omitting
/// the schema's type default for such fields.
fn expr_has_concrete_string_default(expr: &Expr) -> bool {
    match expr {
        Expr::Default(inner, _) => matches!(inner.as_ref(), Expr::Str(..)),
        Expr::Disjunction(left, right, _) | Expr::Unify(left, right, _) => {
            expr_has_concrete_string_default(left) || expr_has_concrete_string_default(right)
        }
        Expr::Paren(inner, _) | Expr::Bound(_, inner, _) => expr_has_concrete_string_default(inner),
        _ => false,
    }
}

fn expr_has_concrete_string_default_resolved(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &BTreeMap<String, Value>,
) -> bool {
    if expr_has_concrete_string_default(expr) {
        return true;
    }
    materialize_expr(
        expr,
        scopes,
        root,
        imports,
        &mut std::collections::BTreeSet::new(),
    )
    .is_some_and(|resolved| expr_has_concrete_string_default(&resolved))
}

fn import_os_env_task(expr: &Expr, imports: &BTreeMap<String, Value>) -> bool {
    selector_names(expr, None).is_some_and(|path| {
        let Some((root, rest)) = path.split_first() else {
            return false;
        };
        let Some(module) = imports.get(root) else {
            return false;
        };
        let mut value = module;
        for field in rest {
            let Some(next) = value_field(value, field) else {
                return false;
            };
            value = next;
        }
        matches!(
            value_fields(value).and_then(|fields| fields.get("$id")),
            Some(Value::String(id))
                if matches!(id.as_str(), "tool/os.Getenv" | "tool/os.Environ")
        )
    })
}

fn value_has_os_env_task(value: &Value) -> bool {
    match value {
        Value::Struct(fields) | Value::Definition { fields, .. } => {
            matches!(
                fields.get("$id"),
                Some(Value::String(id))
                    if matches!(id.as_str(), "tool/os.Getenv" | "tool/os.Environ")
            ) || fields.values().any(value_has_os_env_task)
        }
        Value::Disjunction(alternatives, _) => alternatives.iter().any(value_has_os_env_task),
        _ => false,
    }
}

fn struct_has_os_env_id(items: &[StructItem]) -> bool {
    items.iter().any(|item| {
        let StructItem::Field(field) = item else {
            return false;
        };
        if !matches!(&field.label, Label::Name(name, _) if name == "$id") {
            return false;
        }
        let Expr::Str(parts, _) = &field.value else {
            return false;
        };
        let mut text = String::new();
        for part in parts {
            match part {
                ostinato_lang::ast::StrPart::Text(value) => text.push_str(value),
                ostinato_lang::ast::StrPart::Interp(_) => return false,
            }
        }
        matches!(text.as_str(), "tool/os.Getenv" | "tool/os.Environ")
    })
}

fn expr_has_os_env_task(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &BTreeMap<String, Value>,
    descend_structs: bool,
    visited: &mut std::collections::BTreeSet<String>,
) -> bool {
    match expr {
        Expr::Select(..) | Expr::Index(..) => {
            if import_os_env_task(expr, imports) {
                return true;
            }
            let Some(path) = selector_names(expr, None) else {
                return false;
            };
            let key = format!("sel:{}", path.join("."));
            if !visited.insert(key.clone()) {
                return false;
            }
            let mut found =
                resolve_local_value(scopes, root, expr, &mut std::collections::BTreeSet::new())
                    .is_some_and(|resolved| {
                        expr_has_os_env_task(
                            resolved,
                            scopes,
                            root,
                            imports,
                            descend_structs,
                            visited,
                        )
                    });
            if !found
                && let Some(import_path) = selector_names(expr, None)
                && let Some((root_name, rest)) = import_path.split_first()
                && let Some(module) = imports.get(root_name)
            {
                let mut value = module;
                for field in rest {
                    let Some(next) = value_field(value, field) else {
                        value = module;
                        break;
                    };
                    value = next;
                }
                found = value_has_os_env_task(value);
            }
            visited.remove(&key);
            found
        }
        Expr::Ident(name, _) => {
            if imports.contains_key(name) {
                return false;
            }
            let key = format!("id:{name}");
            if !visited.insert(key.clone()) {
                return false;
            }
            let found = match local_binding(scopes, root, name) {
                Some(StructItem::Field(field)) => expr_has_os_env_task(
                    &field.value,
                    scopes,
                    root,
                    imports,
                    descend_structs,
                    visited,
                ),
                Some(StructItem::Let { value, .. }) => {
                    expr_has_os_env_task(value, scopes, root, imports, descend_structs, visited)
                }
                _ => false,
            };
            visited.remove(&key);
            found
        }
        Expr::Unify(left, right, _) => {
            expr_has_os_env_task(left, scopes, root, imports, descend_structs, visited)
                || expr_has_os_env_task(right, scopes, root, imports, descend_structs, visited)
        }
        Expr::Struct(items, _) if struct_has_os_env_id(items) => true,
        Expr::Struct(items, _) if descend_structs => items.iter().any(|item| match item {
            StructItem::Field(field) => expr_has_os_env_task(
                &field.value,
                scopes,
                root,
                imports,
                descend_structs,
                visited,
            ),
            _ => false,
        }),
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            expr_has_os_env_task(inner, scopes, root, imports, descend_structs, visited)
        }
        _ => false,
    }
}

fn materialize_expr(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &BTreeMap<String, Value>,
    visited: &mut std::collections::BTreeSet<String>,
) -> Option<Expr> {
    match expr {
        Expr::Ident(name, _) => {
            if imports.contains_key(name) {
                return None;
            }
            let key = format!("id:{name}");
            if !visited.insert(key.clone()) {
                return None;
            }
            let resolved = match local_binding(scopes, root, name) {
                Some(StructItem::Field(field)) => {
                    materialize_expr(&field.value, scopes, root, imports, visited)
                }
                Some(StructItem::Let { value, .. }) => {
                    materialize_expr(value, scopes, root, imports, visited)
                }
                _ => None,
            };
            visited.remove(&key);
            resolved
        }
        Expr::Select(..) | Expr::Index(..) => {
            let path_key = selector_names(expr, None).map(|path| format!("sel:{}", path.join(".")));
            if let Some(key) = &path_key
                && !visited.insert(key.clone())
            {
                return None;
            }
            let resolved = if let Some(value) =
                resolve_local_value(scopes, root, expr, &mut std::collections::BTreeSet::new())
            {
                materialize_expr(value, scopes, root, imports, visited)
            } else {
                let path = selector_names(expr, None)?;
                let (root_name, rest) = path.split_first()?;
                let module = imports.get(root_name)?;
                let mut value = module;
                for field in rest {
                    value = value_field(value, field)?;
                }
                if value_fields(value).is_some_and(|fields| fields.contains_key("$id")) {
                    Some(expr.clone())
                } else {
                    output_expr(value)
                }
            };
            if let Some(key) = path_key {
                visited.remove(&key);
            }
            resolved
        }
        Expr::Unify(left, right, span) => Some(Expr::Unify(
            Box::new(materialize_keep(left, scopes, root, imports)),
            Box::new(materialize_keep(right, scopes, root, imports)),
            *span,
        )),
        Expr::Disjunction(left, right, span) => Some(Expr::Disjunction(
            Box::new(materialize_keep(left, scopes, root, imports)),
            Box::new(materialize_keep(right, scopes, root, imports)),
            *span,
        )),
        Expr::Paren(inner, span) => Some(Expr::Paren(
            Box::new(materialize_keep(inner, scopes, root, imports)),
            *span,
        )),
        Expr::Default(inner, span) => Some(Expr::Default(
            Box::new(materialize_keep(inner, scopes, root, imports)),
            *span,
        )),
        Expr::Bound(op, inner, span) => Some(Expr::Bound(
            *op,
            Box::new(materialize_keep(inner, scopes, root, imports)),
            *span,
        )),
        Expr::Struct(..) | Expr::List(..) => Some(expr.clone()),
        _ => None,
    }
}

fn materialize_keep(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &BTreeMap<String, Value>,
) -> Expr {
    materialize_expr(
        expr,
        scopes,
        root,
        imports,
        &mut std::collections::BTreeSet::new(),
    )
    .unwrap_or_else(|| expr.clone())
}

fn materialize_refs(
    expr: &mut Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &BTreeMap<String, Value>,
) {
    match expr {
        Expr::Ident(..) | Expr::Select(..) | Expr::Index(..) => {
            let original = expr.clone();
            if let Some(resolved) = materialize_expr(
                &original,
                scopes,
                root,
                imports,
                &mut std::collections::BTreeSet::new(),
            ) && resolved != original
            {
                let keep_original = matches!(original, Expr::Ident(..))
                    || matches!(original, Expr::Select(..) | Expr::Index(..))
                        && resolve_local_value(
                            scopes,
                            root,
                            &original,
                            &mut std::collections::BTreeSet::new(),
                        )
                        .is_some();
                if keep_original {
                    // Keep the original reference as a conjunct so a `let`
                    // used only here remains referenced; wrap the resolved
                    // clone.
                    let span = original.span();
                    *expr = Expr::Unify(Box::new(original), Box::new(resolved), span);
                } else {
                    // Imported values have no local binding to preserve, and
                    // retaining their unwrapped copy would let an invalid
                    // default win over the schema-constrained clone.
                    *expr = resolved;
                }
            }
        }
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            materialize_refs(left, scopes, root, imports);
            materialize_refs(right, scopes, root, imports);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            materialize_refs(inner, scopes, root, imports);
        }
        _ => {}
    }
}

fn materialize_into(
    expr: &mut Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &BTreeMap<String, Value>,
) {
    materialize_refs(expr, scopes, root, imports);
}

fn wrap_os_task_fields(
    expr: &mut Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &BTreeMap<String, Value>,
) {
    match expr {
        Expr::Struct(items, _) => {
            let scope_items = items.clone();
            let mut nested = scopes.to_vec();
            nested.push(&scope_items);
            for item in items {
                let StructItem::Field(field) = item else {
                    continue;
                };
                let Label::Name(name, _) = &field.label else {
                    continue;
                };
                if name.starts_with('$') {
                    continue;
                }
                // Resolve references only to decide whether a concrete string
                // default is reachable; keep the original expression so `let`
                // bindings remain referenced and shared definitions are not
                // mutated.
                let include_default = !expr_has_concrete_string_default_resolved(
                    &field.value,
                    &nested,
                    root,
                    imports,
                );
                let span = field.value.span();
                field.value = Expr::Unify(
                    Box::new(field.value.clone()),
                    Box::new(os_value_schema(include_default)),
                    span,
                );
            }
        }
        Expr::Unify(left, right, _) => {
            wrap_os_task_fields(left, scopes, root, imports);
            wrap_os_task_fields(right, scopes, root, imports);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            wrap_os_task_fields(inner, scopes, root, imports);
        }
        _ => {}
    }
}

fn apply_os_schema_to_task(
    expr: &mut Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &BTreeMap<String, Value>,
) {
    if !expr_has_os_env_task(
        expr,
        scopes,
        root,
        imports,
        false,
        &mut std::collections::BTreeSet::new(),
    ) {
        return;
    }
    materialize_into(expr, scopes, root, imports);
    wrap_os_task_fields(expr, scopes, root, imports);
}

fn apply_os_schema_to_command_value(
    expr: &mut Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &BTreeMap<String, Value>,
) {
    match expr {
        Expr::Struct(items, _) => {
            let scope_items = items.clone();
            let mut nested = scopes.to_vec();
            nested.push(&scope_items);
            for item in items {
                let StructItem::Field(field) = item else {
                    continue;
                };
                let direct = expr_has_os_env_task(
                    &field.value,
                    &nested,
                    root,
                    imports,
                    false,
                    &mut std::collections::BTreeSet::new(),
                );
                if direct {
                    apply_os_schema_to_task(&mut field.value, &nested, root, imports);
                    continue;
                }
                let grouped = expr_has_os_env_task(
                    &field.value,
                    &nested,
                    root,
                    imports,
                    true,
                    &mut std::collections::BTreeSet::new(),
                );
                if grouped {
                    // A nested task group: recurse instead of treating it as
                    // the env map itself.
                    materialize_into(&mut field.value, &nested, root, imports);
                    apply_os_schema_to_command_value(&mut field.value, &nested, root, imports);
                }
            }
        }
        Expr::Unify(left, right, _) => {
            apply_os_schema_to_command_value(left, scopes, root, imports);
            apply_os_schema_to_command_value(right, scopes, root, imports);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            apply_os_schema_to_command_value(inner, scopes, root, imports);
        }
        _ => {}
    }
}

/// Apply the real `tool/os.Value` constraint to the selected command's
/// `Getenv`/`Environ` task fields before evaluation. CUE's schema is a pattern
/// constraint, but Ostinato evaluates the whole package eagerly; scoping the
/// constraint to the selected command preserves CUE's per-command behaviour
/// and its implicit `*string` default for unset environment variables.
fn apply_os_value_schema(
    items: &mut [StructItem],
    command_name: &str,
    imports: &BTreeMap<String, Value>,
) {
    let root: Vec<StructItem> = items.to_vec();
    for item in items {
        let StructItem::Field(field) = item else {
            continue;
        };
        if !matches!(&field.label, Label::Name(name, _) if name == "command") {
            continue;
        }
        if !expr_has_os_env_task(
            &field.value,
            &[],
            &root,
            imports,
            true,
            &mut std::collections::BTreeSet::new(),
        ) {
            continue;
        }
        materialize_into(&mut field.value, &[], &root, imports);
        apply_os_schema_in_command(&mut field.value, command_name, &[], &root, imports);
    }
}

fn apply_os_schema_in_command(
    expr: &mut Expr,
    command_name: &str,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &BTreeMap<String, Value>,
) {
    match expr {
        Expr::Struct(items, _) => {
            let scope_items = items.clone();
            let mut nested = scopes.to_vec();
            nested.push(&scope_items);
            for item in items {
                let StructItem::Field(field) = item else {
                    continue;
                };
                if matches!(&field.label, Label::Name(name, _) if name == command_name) {
                    materialize_into(&mut field.value, &nested, root, imports);
                    apply_os_schema_to_command_value(&mut field.value, &nested, root, imports);
                }
            }
        }
        Expr::Unify(left, right, _) => {
            apply_os_schema_in_command(left, command_name, scopes, root, imports);
            apply_os_schema_in_command(right, command_name, scopes, root, imports);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            apply_os_schema_in_command(inner, command_name, scopes, root, imports);
        }
        _ => {}
    }
}

fn is_visible_command_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('$')
        && !name.starts_with('#')
        && !name.starts_with('\u{3}')
}

fn command_names_in_expr(expr: &Expr, out: &mut std::collections::BTreeSet<String>) {
    match expr {
        Expr::Struct(items, _) => {
            for item in items {
                if let StructItem::Field(field) = item
                    && let Label::Name(name, _) = &field.label
                    && is_visible_command_name(name)
                {
                    out.insert(name.clone());
                }
            }
        }
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            command_names_in_expr(left, out);
            command_names_in_expr(right, out);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            command_names_in_expr(inner, out);
        }
        _ => {}
    }
}

fn command_names_in_items(items: &[StructItem]) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    for item in items {
        if let StructItem::Field(field) = item
            && matches!(&field.label, Label::Name(name, _) if name == "command")
        {
            command_names_in_expr(&field.value, &mut names);
        }
    }
    names
}

fn command_values_for_name<'a>(expr: &'a Expr, name: &str, out: &mut Vec<&'a Expr>) {
    match expr {
        Expr::Struct(items, _) => {
            for item in items {
                if let StructItem::Field(field) = item
                    && matches!(&field.label, Label::Name(label, _) if label == name)
                {
                    out.push(&field.value);
                }
            }
        }
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            command_values_for_name(left, name, out);
            command_values_for_name(right, name, out);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            command_values_for_name(inner, name, out);
        }
        _ => {}
    }
}

fn command_name_shadowed(
    scopes: &[&[StructItem]],
    bound: &std::collections::BTreeSet<String>,
    name: &str,
) -> bool {
    bound.contains(name) || local_binding(scopes, &[], name).is_some()
}

fn collect_command_references(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    known: &std::collections::BTreeSet<String>,
    bound: &std::collections::BTreeSet<String>,
    out: &mut std::collections::BTreeSet<String>,
) {
    match expr {
        Expr::Ident(name, _) => {
            if known.contains(name) && !command_name_shadowed(scopes, bound, name) {
                out.insert(name.clone());
            }
        }
        Expr::Select(base, _, _, _) => {
            if let Some(path) = selector_names(expr, None) {
                if path.len() >= 2 && path[0] == "command" && known.contains(&path[1]) {
                    out.insert(path[1].clone());
                } else if path.len() >= 2
                    && known.contains(&path[0])
                    && !command_name_shadowed(scopes, bound, &path[0])
                {
                    out.insert(path[0].clone());
                }
            }
            collect_command_references(base, scopes, root, known, bound, out);
        }
        Expr::Index(base, index, _) => {
            collect_command_references(base, scopes, root, known, bound, out);
            collect_command_references(index, scopes, root, known, bound, out);
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            collect_command_references(left, scopes, root, known, bound, out);
            collect_command_references(right, scopes, root, known, bound, out);
        }
        Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Bound(_, inner, _)
        | Expr::Unary(_, inner, _)
        | Expr::OpenList(Some(inner), _) => {
            collect_command_references(inner, scopes, root, known, bound, out)
        }
        Expr::Struct(items, _) => {
            let mut nested = scopes.to_vec();
            nested.push(items);
            for item in items {
                collect_command_references_in_item(item, &nested, root, known, bound, out);
            }
        }
        Expr::List(items, _) => {
            for item in items {
                collect_command_references(item, scopes, root, known, bound, out);
            }
        }
        Expr::OpenListPrefix(items, tail, _) => {
            for item in items {
                collect_command_references(item, scopes, root, known, bound, out);
            }
            if let Some(tail) = tail {
                collect_command_references(tail, scopes, root, known, bound, out);
            }
        }
        Expr::ListComp { clauses, body, .. } => {
            let mut current = bound.clone();
            for clause in clauses {
                match clause {
                    CompClause::For {
                        key, value, source, ..
                    } => {
                        collect_command_references(source, scopes, root, known, &current, out);
                        if let Some(key) = key {
                            current.insert(key.clone());
                        }
                        current.insert(value.clone());
                    }
                    CompClause::If { cond, .. } => {
                        collect_command_references(cond, scopes, root, known, &current, out);
                    }
                }
            }
            collect_command_references(body, scopes, root, known, &current, out);
        }
        Expr::Call(callee, arguments, _) => {
            collect_command_references(callee, scopes, root, known, bound, out);
            for argument in arguments {
                collect_command_references(argument, scopes, root, known, bound, out);
            }
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let StrPart::Interp(inner) = part {
                    collect_command_references(inner, scopes, root, known, bound, out);
                }
            }
        }
        Expr::BytesInterp(parts, _) => {
            for part in parts {
                if let BytePart::Interp(inner) = part {
                    collect_command_references(inner, scopes, root, known, bound, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_command_references_in_item(
    item: &StructItem,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    known: &std::collections::BTreeSet<String>,
    bound: &std::collections::BTreeSet<String>,
    out: &mut std::collections::BTreeSet<String>,
) {
    match item {
        StructItem::Field(field) => {
            if let Label::Dynamic(label, _) = &field.label {
                collect_command_references(label, scopes, root, known, bound, out);
            }
            collect_command_references(&field.value, scopes, root, known, bound, out);
        }
        StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
            collect_command_references(value, scopes, root, known, bound, out);
        }
        StructItem::For {
            key,
            value,
            source,
            body,
            ..
        } => {
            collect_command_references(source, scopes, root, known, bound, out);
            let mut nested = scopes.to_vec();
            nested.push(body);
            let mut current = bound.clone();
            if let Some(key) = key {
                current.insert(key.clone());
            }
            current.insert(value.clone());
            for item in body {
                collect_command_references_in_item(item, &nested, root, known, &current, out);
            }
        }
        StructItem::If { cond, body, .. } => {
            collect_command_references(cond, scopes, root, known, bound, out);
            let mut nested = scopes.to_vec();
            nested.push(body);
            for item in body {
                collect_command_references_in_item(item, &nested, root, known, bound, out);
            }
        }
        StructItem::Ellipsis(_) => {}
    }
}

fn metadata_only_expr(expr: &Expr) -> Expr {
    const KEYS: [&str; 6] = ["$usage", "$short", "$long", "usage", "short", "long"];
    match expr {
        Expr::Struct(items, span) => Expr::Struct(
            items
                .iter()
                .filter_map(|item| {
                    let StructItem::Field(field) = item else {
                        return None;
                    };
                    let Label::Name(name, _) = &field.label else {
                        return None;
                    };
                    KEYS.contains(&name.as_str()).then(|| item.clone())
                })
                .collect(),
            *span,
        ),
        Expr::Unify(left, right, span) => Expr::Unify(
            Box::new(metadata_only_expr(left)),
            Box::new(metadata_only_expr(right)),
            *span,
        ),
        Expr::Paren(inner, span) => Expr::Paren(Box::new(metadata_only_expr(inner)), *span),
        Expr::Default(inner, span) => Expr::Default(Box::new(metadata_only_expr(inner)), *span),
        Expr::Bound(op, inner, span) => {
            Expr::Bound(*op, Box::new(metadata_only_expr(inner)), *span)
        }
        _ => Expr::Struct(Vec::new(), Span::default()),
    }
}

fn prune_command_expr(
    expr: &Expr,
    keep: &std::collections::BTreeSet<String>,
    metadata_only: bool,
) -> Expr {
    match expr {
        Expr::Struct(items, span) => Expr::Struct(
            items
                .iter()
                .filter_map(|item| {
                    let StructItem::Field(field) = item else {
                        return Some(item.clone());
                    };
                    let Label::Name(name, _) = &field.label else {
                        return Some(item.clone());
                    };
                    if !is_visible_command_name(name) {
                        return Some(item.clone());
                    }
                    if !keep.contains(name) {
                        return None;
                    }
                    let mut field = field.clone();
                    if metadata_only {
                        field.value = metadata_only_expr(&field.value);
                    }
                    Some(StructItem::Field(field))
                })
                .collect(),
            *span,
        ),
        Expr::Unify(left, right, span) => Expr::Unify(
            Box::new(prune_command_expr(left, keep, metadata_only)),
            Box::new(prune_command_expr(right, keep, metadata_only)),
            *span,
        ),
        Expr::Paren(inner, span) => Expr::Paren(
            Box::new(prune_command_expr(inner, keep, metadata_only)),
            *span,
        ),
        Expr::Default(inner, span) => Expr::Default(
            Box::new(prune_command_expr(inner, keep, metadata_only)),
            *span,
        ),
        Expr::Bound(op, inner, span) => Expr::Bound(
            *op,
            Box::new(prune_command_expr(inner, keep, metadata_only)),
            *span,
        ),
        _ => expr.clone(),
    }
}

fn prune_root_commands(
    items: &[StructItem],
    keep: &std::collections::BTreeSet<String>,
    metadata_only: bool,
) -> Vec<StructItem> {
    items
        .iter()
        .map(|item| {
            let StructItem::Field(field) = item else {
                return item.clone();
            };
            if !matches!(&field.label, Label::Name(name, _) if name == "command") {
                return item.clone();
            }
            let mut field = field.clone();
            field.value = prune_command_expr(&field.value, keep, metadata_only);
            StructItem::Field(field)
        })
        .collect()
}

fn expr_uses_imports(
    expr: &Expr,
    aliases: &std::collections::BTreeSet<String>,
    used: &mut std::collections::BTreeSet<String>,
) {
    match expr {
        Expr::Ident(name, _) => {
            if aliases.contains(name) {
                used.insert(name.clone());
            }
        }
        Expr::Select(base, _, _, _) | Expr::Index(base, _, _) => {
            expr_uses_imports(base, aliases, used);
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            expr_uses_imports(left, aliases, used);
            expr_uses_imports(right, aliases, used);
        }
        Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Bound(_, inner, _)
        | Expr::Unary(_, inner, _)
        | Expr::OpenList(Some(inner), _) => expr_uses_imports(inner, aliases, used),
        Expr::Struct(items, _) => {
            for item in items {
                item_uses_imports(item, aliases, used);
            }
        }
        Expr::List(items, _) => {
            for item in items {
                expr_uses_imports(item, aliases, used);
            }
        }
        Expr::OpenListPrefix(items, tail, _) => {
            for item in items {
                expr_uses_imports(item, aliases, used);
            }
            if let Some(tail) = tail {
                expr_uses_imports(tail, aliases, used);
            }
        }
        Expr::ListComp { clauses, body, .. } => {
            for clause in clauses {
                match clause {
                    CompClause::For { source, .. } => {
                        expr_uses_imports(source, aliases, used);
                    }
                    CompClause::If { cond, .. } => {
                        expr_uses_imports(cond, aliases, used);
                    }
                }
            }
            expr_uses_imports(body, aliases, used);
        }
        Expr::Call(callee, arguments, _) => {
            expr_uses_imports(callee, aliases, used);
            for argument in arguments {
                expr_uses_imports(argument, aliases, used);
            }
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let StrPart::Interp(inner) = part {
                    expr_uses_imports(inner, aliases, used);
                }
            }
        }
        Expr::BytesInterp(parts, _) => {
            for part in parts {
                if let BytePart::Interp(inner) = part {
                    expr_uses_imports(inner, aliases, used);
                }
            }
        }
        _ => {}
    }
}

fn item_uses_imports(
    item: &StructItem,
    aliases: &std::collections::BTreeSet<String>,
    used: &mut std::collections::BTreeSet<String>,
) {
    match item {
        StructItem::Field(field) => {
            if let Label::Dynamic(label, _) = &field.label {
                expr_uses_imports(label, aliases, used);
            }
            expr_uses_imports(&field.value, aliases, used);
        }
        StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
            expr_uses_imports(value, aliases, used);
        }
        StructItem::For { source, body, .. } => {
            expr_uses_imports(source, aliases, used);
            for item in body {
                item_uses_imports(item, aliases, used);
            }
        }
        StructItem::If { cond, body, .. } => {
            expr_uses_imports(cond, aliases, used);
            for item in body {
                item_uses_imports(item, aliases, used);
            }
        }
        StructItem::Ellipsis(_) => {}
    }
}

fn used_import_aliases(
    items: &[StructItem],
    aliases: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeSet<String> {
    let mut used = std::collections::BTreeSet::new();
    for item in items {
        item_uses_imports(item, aliases, &mut used);
    }
    used
}

fn item_uses_imported_task_struct(
    item: &StructItem,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &BTreeMap<String, Value>,
    non_task_imports: &ImportKinds,
    visited: &mut std::collections::BTreeSet<String>,
) -> bool {
    match item {
        StructItem::Field(field) => {
            if let Label::Dynamic(label, _) = &field.label
                && expr_uses_imported_task_struct(
                    label,
                    scopes,
                    root,
                    imports,
                    non_task_imports,
                    visited,
                )
            {
                return true;
            }
            expr_uses_imported_task_struct(
                &field.value,
                scopes,
                root,
                imports,
                non_task_imports,
                visited,
            )
        }
        StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
            expr_uses_imported_task_struct(value, scopes, root, imports, non_task_imports, visited)
        }
        StructItem::For { source, body, .. } => {
            expr_uses_imported_task_struct(source, scopes, root, imports, non_task_imports, visited)
                || body.iter().any(|item| {
                    item_uses_imported_task_struct(
                        item,
                        scopes,
                        root,
                        imports,
                        non_task_imports,
                        visited,
                    )
                })
        }
        StructItem::If { cond, body, .. } => {
            expr_uses_imported_task_struct(cond, scopes, root, imports, non_task_imports, visited)
                || body.iter().any(|item| {
                    item_uses_imported_task_struct(
                        item,
                        scopes,
                        root,
                        imports,
                        non_task_imports,
                        visited,
                    )
                })
        }
        StructItem::Ellipsis(_) => false,
    }
}

fn value_has_ambiguous_os_default(value: &Value) -> bool {
    match value {
        Value::Struct(fields) | Value::Definition { fields, .. } => {
            if matches!(
                fields.get("$id"),
                Some(Value::String(id))
                    if matches!(id.as_str(), "tool/os.Getenv" | "tool/os.Environ")
            ) {
                return fields.iter().any(|(name, value)| {
                    !name.starts_with('$') && value_default_is_non_string(value)
                });
            }
            fields.values().any(value_has_ambiguous_os_default)
        }
        Value::Disjunction(alternatives, _) => {
            alternatives.iter().any(value_has_ambiguous_os_default)
        }
        _ => false,
    }
}

fn value_default_is_non_string(value: &Value) -> bool {
    match value {
        Value::Disjunction(alternatives, defaults) => {
            if defaults.len() != 1 {
                return !defaults.is_empty();
            }
            alternatives.get(defaults[0]).is_some_and(|value| {
                !matches!(
                    value,
                    Value::String(_) | Value::Type(ostinato_lang::TypeKind::String)
                )
            })
        }
        _ => false,
    }
}

fn expr_uses_imported_task_struct(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &BTreeMap<String, Value>,
    non_task_imports: &ImportKinds,
    visited: &mut std::collections::BTreeSet<String>,
) -> bool {
    match expr {
        Expr::Ident(name, _) => {
            if imports.contains_key(name) {
                return false;
            }
            let key = format!("id:{name}");
            if !visited.insert(key.clone()) {
                return false;
            }
            let found = match local_binding(scopes, root, name) {
                Some(StructItem::Field(field)) => expr_uses_imported_task_struct(
                    &field.value,
                    scopes,
                    root,
                    imports,
                    non_task_imports,
                    visited,
                ),
                Some(StructItem::Let { value, .. }) => expr_uses_imported_task_struct(
                    value,
                    scopes,
                    root,
                    imports,
                    non_task_imports,
                    visited,
                ),
                _ => false,
            };
            visited.remove(&key);
            found
        }
        Expr::Select(..) | Expr::Index(..) => {
            if let Some(path) = selector_names(expr, None)
                && let Some((root_name, rest)) = path.split_first()
                && non_task_imports.non_task.contains(root_name)
                && let Some(module) = imports.get(root_name)
            {
                let mut value = module;
                let mut found = true;
                for field in rest {
                    let Some(next) = value_field(value, field) else {
                        found = false;
                        break;
                    };
                    value = next;
                }
                if found && value_has_ambiguous_os_default(value) {
                    return true;
                }
            }
            let key = selector_names(expr, None).map(|path| format!("sel:{}", path.join(".")));
            if let Some(key) = &key
                && !visited.insert(key.clone())
            {
                return false;
            }
            let found =
                resolve_local_value(scopes, root, expr, &mut std::collections::BTreeSet::new())
                    .is_some_and(|resolved| {
                        expr_uses_imported_task_struct(
                            resolved,
                            scopes,
                            root,
                            imports,
                            non_task_imports,
                            visited,
                        )
                    });
            let base = match expr {
                Expr::Select(base, _, _, _) | Expr::Index(base, _, _) => Some(base.as_ref()),
                _ => None,
            };
            let found = found
                || base.is_some_and(|base| {
                    expr_uses_imported_task_struct(
                        base,
                        scopes,
                        root,
                        imports,
                        non_task_imports,
                        visited,
                    )
                });
            if let Some(key) = key {
                visited.remove(&key);
            }
            found
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            expr_uses_imported_task_struct(left, scopes, root, imports, non_task_imports, visited)
                || expr_uses_imported_task_struct(
                    right,
                    scopes,
                    root,
                    imports,
                    non_task_imports,
                    visited,
                )
        }
        Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Bound(_, inner, _)
        | Expr::Unary(_, inner, _)
        | Expr::OpenList(Some(inner), _) => {
            expr_uses_imported_task_struct(inner, scopes, root, imports, non_task_imports, visited)
        }
        Expr::Struct(items, _) => {
            let mut nested = scopes.to_vec();
            nested.push(items);
            items.iter().any(|item| {
                item_uses_imported_task_struct(
                    item,
                    &nested,
                    root,
                    imports,
                    non_task_imports,
                    visited,
                )
            })
        }
        Expr::List(items, _) => items.iter().any(|item| {
            expr_uses_imported_task_struct(item, scopes, root, imports, non_task_imports, visited)
        }),
        Expr::OpenListPrefix(items, tail, _) => {
            items.iter().any(|item| {
                expr_uses_imported_task_struct(
                    item,
                    scopes,
                    root,
                    imports,
                    non_task_imports,
                    visited,
                )
            }) || tail.as_ref().is_some_and(|tail| {
                expr_uses_imported_task_struct(
                    tail,
                    scopes,
                    root,
                    imports,
                    non_task_imports,
                    visited,
                )
            })
        }
        Expr::ListComp { clauses, body, .. } => {
            clauses.iter().any(|clause| match clause {
                CompClause::For { source, .. } => expr_uses_imported_task_struct(
                    source,
                    scopes,
                    root,
                    imports,
                    non_task_imports,
                    visited,
                ),
                CompClause::If { cond, .. } => expr_uses_imported_task_struct(
                    cond,
                    scopes,
                    root,
                    imports,
                    non_task_imports,
                    visited,
                ),
            }) || expr_uses_imported_task_struct(
                body,
                scopes,
                root,
                imports,
                non_task_imports,
                visited,
            )
        }
        Expr::Call(callee, arguments, _) => {
            expr_uses_imported_task_struct(callee, scopes, root, imports, non_task_imports, visited)
                || arguments.iter().any(|argument| {
                    expr_uses_imported_task_struct(
                        argument,
                        scopes,
                        root,
                        imports,
                        non_task_imports,
                        visited,
                    )
                })
        }
        Expr::Str(parts, _) => parts.iter().any(|part| match part {
            StrPart::Interp(inner) => expr_uses_imported_task_struct(
                inner,
                scopes,
                root,
                imports,
                non_task_imports,
                visited,
            ),
            StrPart::Text(_) => false,
        }),
        Expr::BytesInterp(parts, _) => parts.iter().any(|part| match part {
            BytePart::Interp(inner) => expr_uses_imported_task_struct(
                inner,
                scopes,
                root,
                imports,
                non_task_imports,
                visited,
            ),
            BytePart::Text(_) => false,
        }),
        _ => false,
    }
}

fn prune_unreferenced_lets(items: &[StructItem]) -> Vec<StructItem> {
    let mut current = items.to_vec();
    loop {
        let names: std::collections::BTreeSet<String> = current
            .iter()
            .filter_map(|item| match item {
                StructItem::Let { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        if names.is_empty() {
            return current;
        }
        let used = used_import_aliases(&current, &names);
        let next: Vec<StructItem> = current
            .iter()
            .filter(|item| match item {
                StructItem::Let { name, .. } => used.contains(name),
                _ => true,
            })
            .cloned()
            .collect();
        if next.len() == current.len() {
            return next;
        }
        current = next;
    }
}

fn selected_command_scope(
    items: &[StructItem],
    command_name: &str,
) -> std::collections::BTreeSet<String> {
    let known = command_names_in_items(items);
    let mut keep = std::collections::BTreeSet::from([command_name.to_owned()]);
    loop {
        let mut next = keep.clone();
        for name in &keep {
            let mut values = Vec::new();
            for item in items {
                if let StructItem::Field(field) = item
                    && matches!(&field.label, Label::Name(label, _) if label == "command")
                {
                    command_values_for_name(&field.value, name, &mut values);
                }
            }
            for value in values {
                collect_command_references(
                    value,
                    &[],
                    items,
                    &known,
                    &std::collections::BTreeSet::new(),
                    &mut next,
                );
            }
        }
        if next.len() == keep.len() {
            return keep;
        }
        keep = next;
    }
}

fn command_task_items(
    items: &[StructItem],
    command_name: &str,
) -> Result<Vec<StructItem>, ostinato_lang::LangError> {
    let mut found = false;
    let mut tasks = Vec::new();
    for item in items {
        let StructItem::Field(field) = item else {
            continue;
        };
        let Label::Name(name, _) = &field.label else {
            continue;
        };
        if name != "command" {
            continue;
        }
        let Expr::Struct(command_items, _) = &field.value else {
            continue;
        };
        for command_item in command_items {
            let StructItem::Field(command_field) = command_item else {
                continue;
            };
            let Label::Name(name, _) = &command_field.label else {
                continue;
            };
            if name != command_name {
                continue;
            }
            found = true;
            let mut visited = std::collections::BTreeSet::new();
            resolve_command_items(items, &command_field.value, &mut visited, &mut tasks);
        }
    }
    if !found {
        return Err(ostinato_lang::LangError::Workflow(format!(
            "unknown tool command `{command_name}`"
        )));
    }
    Ok(tasks)
}

/// Extract a literal string index such as `lib["base"]`.
fn index_field_name(expr: &Expr) -> Option<String> {
    if let Expr::Int(value, _) = expr {
        return Some(value.to_string());
    }
    let Expr::Str(parts, _) = expr else {
        return None;
    };
    let mut text = String::new();
    for part in parts {
        match part {
            ostinato_lang::ast::StrPart::Text(value) => text.push_str(value),
            ostinato_lang::ast::StrPart::Interp(_) => return None,
        }
    }
    Some(text)
}

/// Turn an identifier/selector expression into the field path it denotes.
fn selector_path(
    items: &[StructItem],
    expr: &Expr,
    visited: &mut std::collections::BTreeSet<String>,
) -> Option<Vec<String>> {
    match expr {
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return None;
            }
            if let Some(value) = local_definition(items, items, name)
                && matches!(
                    value,
                    Expr::Ident(..)
                        | Expr::Select(..)
                        | Expr::Index(..)
                        | Expr::Paren(..)
                        | Expr::Default(..)
                        | Expr::Bound(..)
                )
                && let Some(path) = selector_path(items, value, visited)
            {
                return Some(path);
            }
            Some(vec![name.clone()])
        }
        Expr::Select(base, field, _, _) => {
            let mut path = selector_path(items, base, visited)?;
            path.push(field.clone());
            Some(path)
        }
        Expr::Index(base, index, _) => {
            let mut path = selector_path(items, base, visited)?;
            path.push(index_field_name(index)?);
            Some(path)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            selector_path(items, inner, visited)
        }
        _ => None,
    }
}

/// Find a field inside a (possibly unified or aliased) struct expression.
fn field_in_expr<'a>(
    items: &'a [StructItem],
    expr: &'a Expr,
    field: &str,
    visited: &mut std::collections::BTreeSet<String>,
) -> Option<&'a Expr> {
    match expr {
        Expr::Struct(struct_items, _) => named_field(struct_items, field),
        Expr::Unify(left, right, _) => field_in_expr(items, left, field, visited)
            .or_else(|| field_in_expr(items, right, field, visited)),
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            field_in_expr(items, inner, field, visited)
        }
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return None;
            }
            local_definition(items, items, name)
                .and_then(|value| field_in_expr(items, value, field, visited))
        }
        Expr::Select(..) | Expr::Index(..) => {
            let path = selector_path(items, expr, &mut std::collections::BTreeSet::new())?;
            let value = navigate_path(items, &path)?;
            field_in_expr(items, value, field, visited)
        }
        _ => None,
    }
}

/// Follow a field path from a top-level declaration.
fn navigate_path<'a>(items: &'a [StructItem], path: &[String]) -> Option<&'a Expr> {
    let (first, rest) = path.split_first()?;
    let mut value = local_definition(items, items, first)?;
    for field in rest {
        value = field_in_expr(items, value, field, &mut std::collections::BTreeSet::new())?;
    }
    Some(value)
}

fn definitions<'a>(items: &'a [StructItem], name: &str) -> Vec<&'a Expr> {
    items
        .iter()
        .filter_map(|item| match item {
            StructItem::Field(field) => match &field.label {
                Label::Name(label, _) if label == name => Some(&field.value),
                _ => None,
            },
            StructItem::Let {
                name: let_name,
                value,
                ..
            } if let_name == name => Some(value),
            _ => None,
        })
        .collect()
}

fn all_field_values<'a>(
    items: &'a [StructItem],
    expr: &'a Expr,
    field: &str,
    visited: &mut std::collections::BTreeSet<String>,
) -> Vec<&'a Expr> {
    match expr {
        Expr::Struct(struct_items, _) => struct_items
            .iter()
            .filter_map(|item| match item {
                StructItem::Field(field_item) => match &field_item.label {
                    Label::Name(label, _) if label == field => Some(&field_item.value),
                    _ => None,
                },
                _ => None,
            })
            .collect(),
        Expr::Unify(left, right, _) => {
            let mut values = all_field_values(items, left, field, visited);
            values.extend(all_field_values(items, right, field, visited));
            values
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            all_field_values(items, inner, field, visited)
        }
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return Vec::new();
            }
            let mut values = Vec::new();
            for value in definitions(items, name) {
                values.extend(all_field_values(items, value, field, visited));
            }
            values
        }
        _ => Vec::new(),
    }
}

fn deref_value<'a>(
    items: &'a [StructItem],
    path: Vec<String>,
    value: &'a Expr,
    visited: &mut std::collections::BTreeSet<String>,
) -> Vec<(Vec<String>, &'a Expr)> {
    match value {
        Expr::Ident(..) | Expr::Select(..) | Expr::Index(..) => {
            let Some(alias_path) =
                selector_path(items, value, &mut std::collections::BTreeSet::new())
            else {
                return vec![(path, value)];
            };
            let Some(alias_value) = navigate_path(items, &alias_path) else {
                return vec![(path, value)];
            };
            let key = alias_path.join("\u{0}");
            if !visited.insert(key) {
                return vec![(path, value)];
            }
            deref_value(items, alias_path, alias_value, visited)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            deref_value(items, path, inner, visited)
        }
        Expr::Unify(left, right, _) => {
            let mut out = deref_value(items, path.clone(), left, visited);
            out.extend(deref_value(items, path, right, visited));
            out
        }
        _ => vec![(path, value)],
    }
}

/// Fully dereference a selector path through aliases at every segment.
fn resolve_path_candidates<'a>(
    items: &'a [StructItem],
    path: &[String],
) -> Vec<(Vec<String>, &'a Expr)> {
    let Some((first, rest)) = path.split_first() else {
        return Vec::new();
    };
    let mut candidates: Vec<(Vec<String>, &Expr)> = definitions(items, first)
        .into_iter()
        .map(|value| (vec![first.clone()], value))
        .collect();
    for field in rest {
        let mut next = Vec::new();
        for (prefix, value) in candidates {
            for (alias_path, alias_value) in
                deref_value(items, prefix, value, &mut std::collections::BTreeSet::new())
            {
                for field_value in all_field_values(
                    items,
                    alias_value,
                    field,
                    &mut std::collections::BTreeSet::new(),
                ) {
                    let mut target = alias_path.clone();
                    target.push(field.clone());
                    next.push((target, field_value));
                }
            }
        }
        candidates = next;
    }
    let mut out = Vec::new();
    for (prefix, value) in candidates {
        out.extend(deref_value(
            items,
            prefix,
            value,
            &mut std::collections::BTreeSet::new(),
        ));
    }
    out
}

/// Resolve a command value into the union of its struct operands.
fn resolve_command_items(
    items: &[StructItem],
    expr: &Expr,
    visited: &mut std::collections::BTreeSet<String>,
    out: &mut Vec<StructItem>,
) {
    match expr {
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return;
            }
            if let Some(value) = local_definition(items, items, name) {
                resolve_command_items(items, value, visited, out);
            }
        }
        Expr::Select(..) | Expr::Index(..) => {
            if let Some(path) = selector_path(items, expr, &mut std::collections::BTreeSet::new()) {
                for (_, value) in resolve_path_candidates(items, &path) {
                    resolve_command_items(items, value, visited, out);
                }
            }
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            resolve_command_items(items, inner, visited, out);
        }
        Expr::Unify(left, right, _) => {
            resolve_command_items(items, left, visited, out);
            resolve_command_items(items, right, visited, out);
        }
        Expr::Struct(task_items, _) => out.extend(task_items.iter().cloned()),
        _ => {}
    }
}

fn push_target(out: &mut Vec<Vec<String>>, path: Vec<String>) {
    if !out.contains(&path) {
        out.push(path);
    }
}

/// Field paths of the declarations (fields or lets) reachable from a command value.
fn command_targets(
    items: &[StructItem],
    expr: &Expr,
    visited: &mut std::collections::BTreeSet<String>,
    out: &mut Vec<Vec<String>>,
) {
    match expr {
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return;
            }
            if let Some(path) = selector_path(items, expr, &mut std::collections::BTreeSet::new()) {
                push_target(out, path);
            }
            if let Some(value) = local_definition(items, items, name) {
                command_targets(items, value, visited, out);
            }
        }
        Expr::Select(..) | Expr::Index(..) => {
            if let Some(path) = selector_path(items, expr, &mut std::collections::BTreeSet::new()) {
                for (target, _) in resolve_path_candidates(items, &path) {
                    push_target(out, target);
                }
            }
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            command_targets(items, inner, visited, out);
        }
        Expr::Unify(left, right, _) => {
            command_targets(items, left, visited, out);
            command_targets(items, right, visited, out);
        }
        _ => {}
    }
}

/// Resolve a selector base to the field label it ultimately denotes.
fn resolved_path<'a>(
    expr: &'a Expr,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    visited: &mut std::collections::BTreeSet<String>,
    allow_field: bool,
) -> Option<Vec<String>> {
    match expr {
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return None;
            }
            if let Some((_, item)) = local_binding_location(scopes, root, name) {
                return match item {
                    StructItem::Let { value, .. } => {
                        resolved_path(value, scopes, root, visited, true)
                    }
                    StructItem::Field(field) if field.definition => match &field.label {
                        Label::Name(label, _) => Some(vec![label.clone()]),
                        _ => None,
                    },
                    StructItem::Field(field) if allow_field => match &field.label {
                        Label::Name(label, _) => Some(vec![label.clone()]),
                        _ => None,
                    },
                    _ => None,
                };
            }
            name.starts_with('#').then(|| vec![name.clone()])
        }
        Expr::Select(base, field, _, _) => {
            let mut path = resolved_path(base, scopes, root, visited, allow_field)?;
            path.push(field.clone());
            Some(path)
        }
        Expr::Index(base, index, _) => {
            let segment = index_field_name(index)?;
            let mut path = resolved_path(base, scopes, root, visited, allow_field)?;
            path.push(segment);
            Some(path)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            resolved_path(inner, scopes, root, visited, allow_field)
        }
        _ => None,
    }
}

fn path_key(path: &[String]) -> String {
    format!("def:{}", path.join("/"))
}

fn resolved_definition_alias_path<'a>(
    expr: &'a Expr,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    visited: &mut std::collections::BTreeSet<String>,
) -> Option<Vec<String>> {
    match expr {
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return None;
            }
            if let Some((_, item)) = local_binding_location(scopes, root, name) {
                return match item {
                    StructItem::Let { value, .. } => {
                        resolved_definition_alias_path(value, scopes, root, visited)
                    }
                    StructItem::Field(field) if field.definition => match &field.label {
                        Label::Name(label, _) => Some(vec![label.clone()]),
                        _ => None,
                    },
                    StructItem::Field(field) => {
                        let path = resolved_read_path(
                            &field.value,
                            scopes,
                            root,
                            visited,
                            &ImportKinds::default(),
                        )?;
                        path.first()
                            .is_some_and(|segment| segment.starts_with('#'))
                            .then_some(path)
                    }
                    _ => None,
                };
            }
            name.starts_with('#').then(|| vec![name.clone()])
        }
        Expr::Select(base, field, _, _) => {
            if let Some(mut path) = resolved_definition_alias_path(base, scopes, root, visited) {
                path.push(field.clone());
                return Some(path);
            }
            let value =
                resolve_local_value(scopes, root, base, &mut std::collections::BTreeSet::new())?;
            let value = select_field_value(
                scopes,
                root,
                value,
                field,
                &mut std::collections::BTreeSet::new(),
            )?;
            resolved_definition_alias_path(value, scopes, root, visited)
        }
        Expr::Index(base, index, _) => {
            let segment = index_field_name(index)?;
            if let Some(mut path) = resolved_definition_alias_path(base, scopes, root, visited) {
                path.push(segment);
                return Some(path);
            }
            let value =
                resolve_local_value(scopes, root, base, &mut std::collections::BTreeSet::new())?;
            let value = if let Expr::List(items, _) = value
                && let Ok(position) = segment.parse::<usize>()
            {
                items.get(position)?
            } else {
                select_field_value(
                    scopes,
                    root,
                    value,
                    &segment,
                    &mut std::collections::BTreeSet::new(),
                )?
            };
            resolved_definition_alias_path(value, scopes, root, visited)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            resolved_definition_alias_path(inner, scopes, root, visited)
        }
        _ => None,
    }
}

fn resolved_read_path<'a>(
    expr: &'a Expr,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    visited: &mut std::collections::BTreeSet<String>,
    imports: &ImportKinds,
) -> Option<Vec<String>> {
    resolved_definition_alias_path(expr, scopes, root, &mut std::collections::BTreeSet::new())
        .or_else(|| resolved_path(expr, scopes, root, visited, false))
        .or_else(|| {
            // Package-root inline tasks (`prep: exec.Run & {...}`) are
            // scheduled by CUE when read directly; import-rooted module tasks
            // are not (CUE deadlocks instead).
            let path = resolved_path(
                expr,
                scopes,
                root,
                &mut std::collections::BTreeSet::new(),
                true,
            )?;
            path_is_local_inline_task(&path, root, imports).then_some(path)
        })
}

fn path_is_local_inline_task(path: &[String], root: &[StructItem], imports: &ImportKinds) -> bool {
    let Some(first) = path.first() else {
        return false;
    };
    if !matches!(
        local_binding_location(&[root], root, first),
        Some((true, _))
    ) {
        return false;
    }
    find_field_expr_at_path(root, path).is_some_and(|expr| local_task_target(expr, root, imports))
}

fn local_task_target(expr: &Expr, root: &[StructItem], imports: &ImportKinds) -> bool {
    if struct_has_task_id(expr) {
        return true;
    }
    match expr {
        Expr::Ident(name, _) => match local_binding_location(&[root], root, name) {
            Some((_, StructItem::Let { value, .. })) => local_task_target(value, root, imports),
            Some((_, StructItem::Field(field))) if !field.definition => {
                local_task_target(&field.value, root, imports)
            }
            _ => false,
        },
        Expr::Unify(left, right, _) => {
            if struct_has_task_id(left) || struct_has_task_id(right) {
                return true;
            }
            local_task_target(left, root, imports) || local_task_target(right, root, imports)
        }
        Expr::Select(..) | Expr::Index(..) => {
            if is_module_task_selector(expr, imports) {
                return false;
            }
            is_task_expr(
                expr,
                &[root],
                root,
                &mut std::collections::BTreeSet::new(),
                imports,
            )
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            local_task_target(inner, root, imports)
        }
        _ => false,
    }
}

fn is_module_task_selector(expr: &Expr, imports: &ImportKinds) -> bool {
    let Some(root) = selector_root_ident(expr) else {
        return false;
    };
    imports.non_task.contains(root) && imports.task_paths.contains_key(root)
}

fn canonical_definition_path(root: &[StructItem], path: &[String]) -> Vec<String> {
    let mut current = path.to_vec();
    while let Some(first) = current.first().cloned() {
        if !first.starts_with('#') {
            break;
        }
        let Some(StructItem::Field(field)) = root
            .iter()
            .find(|item| matches!(item, StructItem::Field(field) if matches!(&field.label, Label::Name(label, _) if label == &first)))
        else {
            break;
        };
        if !field.definition {
            break;
        }
        let mut value = &field.value;
        while let Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) = value
        {
            value = inner;
        }
        let Expr::Ident(target, _) = value else {
            break;
        };
        let Some(StructItem::Field(target_field)) = root
            .iter()
            .find(|item| matches!(item, StructItem::Field(field) if matches!(&field.label, Label::Name(label, _) if label == target)))
        else {
            break;
        };
        if !target_field.definition {
            break;
        }
        let mut next = vec![target.clone()];
        next.extend_from_slice(&current[1..]);
        current = next;
    }
    current
}

/// Package-root fields that only alias a bare local-module task selector.
///
/// CUE does not schedule these through `let`/alias output reads, unlike root
/// inline tasks and command-local fields.
fn root_bare_module_task(items: &[StructItem], name: &str, imports: &ImportKinds) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum TaskKind {
        Neutral,
        Bare,
        Composed,
    }

    fn merge(left: TaskKind, right: TaskKind) -> TaskKind {
        match (left, right) {
            (TaskKind::Composed, _) | (_, TaskKind::Composed) => TaskKind::Composed,
            (TaskKind::Bare, _) | (_, TaskKind::Bare) => TaskKind::Bare,
            _ => TaskKind::Neutral,
        }
    }

    fn classify<'a>(
        items: &'a [StructItem],
        expr: &'a Expr,
        seen: &mut std::collections::BTreeSet<String>,
        imports: &ImportKinds,
    ) -> TaskKind {
        match expr {
            Expr::Ident(name, _) => {
                if !seen.insert(name.clone()) {
                    return TaskKind::Neutral;
                }
                let definition = name.starts_with('#');
                let mut kind = TaskKind::Neutral;
                let mut found = false;
                for item in items {
                    match item {
                        StructItem::Field(field)
                            if field.definition == definition
                                && matches!(&field.label, Label::Name(label, _) if label == name) =>
                        {
                            found = true;
                            kind = merge(kind, classify(items, &field.value, seen, imports));
                        }
                        StructItem::Let {
                            name: label, value, ..
                        } if !definition && label == name => {
                            found = true;
                            kind = merge(kind, classify(items, value, seen, imports));
                        }
                        _ => {}
                    }
                }
                seen.remove(name);
                if found { kind } else { TaskKind::Neutral }
            }
            Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
                classify(items, inner, seen, imports)
            }
            Expr::Unify(left, right, _) => {
                let left = classify(items, left, seen, imports);
                let right = classify(items, right, seen, imports);
                merge(left, right)
            }
            Expr::Disjunction(..) => TaskKind::Composed,
            Expr::Struct(..) | Expr::List(..) => TaskKind::Composed,
            Expr::Select(..) | Expr::Index(..) => {
                if import_task_alias(expr, imports).is_some() {
                    TaskKind::Bare
                } else if let Some(value) = resolve_local_value(
                    &[items],
                    items,
                    expr,
                    &mut std::collections::BTreeSet::new(),
                ) {
                    classify(items, value, seen, imports)
                } else {
                    TaskKind::Neutral
                }
            }
            _ => TaskKind::Neutral,
        }
    }

    let mut kind = TaskKind::Neutral;
    let mut found = false;
    for item in items {
        if let StructItem::Field(field) = item
            && !field.definition
            && matches!(&field.label, Label::Name(label, _) if label == name)
        {
            found = true;
            kind = merge(
                kind,
                classify(
                    items,
                    &field.value,
                    &mut std::collections::BTreeSet::new(),
                    imports,
                ),
            );
        }
    }
    found && kind == TaskKind::Bare
}

/// Field names read as `<name>.stdout`/`.stderr`/`.success` anywhere in the
/// command task tree.
fn collect_command_field_output_reads(
    expr: &Expr,
    out: &mut std::collections::BTreeSet<String>,
    paths: &mut std::collections::BTreeSet<Vec<String>>,
) {
    fn item_reads(
        item: &StructItem,
        out: &mut std::collections::BTreeSet<String>,
        paths: &mut std::collections::BTreeSet<Vec<String>>,
    ) {
        match item {
            StructItem::Field(field) => {
                collect_command_field_output_reads(&field.value, out, paths)
            }
            StructItem::Pattern { value, .. } | StructItem::Let { value, .. } => {
                collect_command_field_output_reads(value, out, paths);
            }
            StructItem::For { source, body, .. } => {
                collect_command_field_output_reads(source, out, paths);
                for item in body {
                    item_reads(item, out, paths);
                }
            }
            StructItem::If { cond, body, .. } => {
                collect_command_field_output_reads(cond, out, paths);
                for item in body {
                    item_reads(item, out, paths);
                }
            }
            StructItem::Ellipsis(_) => {}
        }
    }
    match expr {
        Expr::Select(base, field, _, _) => {
            if matches!(field.as_str(), "stdout" | "stderr" | "success") {
                if let Some(name) = command_field_read_name(base) {
                    out.insert(name);
                }
                if let Some(path) = selector_names(base, None) {
                    paths.insert(path);
                }
            }
            collect_command_field_output_reads(base, out, paths);
        }
        Expr::Index(base, index, _) => {
            collect_command_field_output_reads(base, out, paths);
            collect_command_field_output_reads(index, out, paths);
        }
        Expr::Call(callee, arguments, _) => {
            collect_command_field_output_reads(callee, out, paths);
            for argument in arguments {
                collect_command_field_output_reads(argument, out, paths);
            }
        }
        Expr::Bound(_, inner, _)
        | Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Unary(_, inner, _) => collect_command_field_output_reads(inner, out, paths),
        Expr::Struct(items, _) => {
            for item in items {
                item_reads(item, out, paths);
            }
        }
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                collect_command_field_output_reads(item, out, paths);
            }
        }
        Expr::OpenList(Some(inner), _) => collect_command_field_output_reads(inner, out, paths),
        Expr::OpenList(None, _) | Expr::Null(_) => {}
        Expr::ListComp { clauses, body, .. } => {
            for clause in clauses {
                match clause {
                    CompClause::For { source, .. } => {
                        collect_command_field_output_reads(source, out, paths);
                    }
                    CompClause::If { cond, .. } => {
                        collect_command_field_output_reads(cond, out, paths);
                    }
                }
            }
            collect_command_field_output_reads(body, out, paths);
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            collect_command_field_output_reads(left, out, paths);
            collect_command_field_output_reads(right, out, paths);
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let StrPart::Interp(inner) = part {
                    collect_command_field_output_reads(inner, out, paths);
                }
            }
        }
        Expr::BytesInterp(parts, _) => {
            for part in parts {
                if let BytePart::Interp(inner) = part {
                    collect_command_field_output_reads(inner, out, paths);
                }
            }
        }
        Expr::Bool(..)
        | Expr::Int(..)
        | Expr::BigInt(..)
        | Expr::Float(..)
        | Expr::Bytes(..)
        | Expr::Ident(..) => {}
    }
}

fn command_field_read_name(base: &Expr) -> Option<String> {
    match base {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Select(..) | Expr::Index(..) => selector_leaf(base),
        _ => None,
    }
}

fn path_is_module_terminal(
    items: &[StructItem],
    imports: &BTreeMap<String, Value>,
    path: &[String],
) -> bool {
    find_field_expr_at_path_in_root(items, path, items).is_some_and(|value| {
        module_task_path_scoped(
            value,
            &[items],
            items,
            imports,
            &mut std::collections::BTreeSet::new(),
        )
        .is_some()
    })
}

/// Definition-rooted paths of task values whose outputs the command reads.
#[allow(clippy::too_many_arguments)]
fn collect_output_task_paths<'a>(
    expr: &'a Expr,
    scopes: &mut Vec<&'a [StructItem]>,
    root: &'a [StructItem],
    out: &mut std::collections::BTreeSet<Vec<String>>,
    plain: &mut std::collections::BTreeSet<Vec<String>>,
    in_task: bool,
    visited: &mut std::collections::BTreeSet<String>,
    imports: &ImportKinds,
    module_values: &BTreeMap<String, Value>,
) {
    match expr {
        Expr::Select(base, field, _, _) => {
            if matches!(field.as_str(), "stdout" | "stderr" | "success") {
                let local_path = resolved_read_path(
                    base,
                    scopes,
                    root,
                    &mut std::collections::BTreeSet::new(),
                    imports,
                )
                .filter(|path| {
                    find_field_expr_at_path(root, path).is_some()
                        && path
                            .first()
                            .is_none_or(|first| !command_local_shadows_root(scopes, root, first))
                })
                .or_else(|| local_read_path(base, scopes, root, module_values));
                let module_path = if local_path.is_none() {
                    module_task_path_scoped(
                        base,
                        scopes,
                        root,
                        module_values,
                        &mut std::collections::BTreeSet::new(),
                    )
                } else {
                    None
                };
                if let Some(path) = local_path {
                    if in_task {
                        out.insert(path);
                    } else {
                        plain.insert(path);
                    }
                } else if let Some(path) = module_path {
                    // Command-local task instances are scheduled on their own;
                    // reads through them behave like plain reads for injection.
                    if in_task && !command_local_task_read(base, scopes, root, imports) {
                        out.insert(path);
                    } else {
                        plain.insert(path);
                    }
                }
            }
            collect_output_task_paths(
                base,
                scopes,
                root,
                out,
                plain,
                in_task,
                visited,
                imports,
                module_values,
            );
        }
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return;
            }
            if let Some(item) = local_binding(scopes, root, name) {
                match item {
                    StructItem::Let { value, .. } => {
                        collect_output_task_paths(
                            value,
                            scopes,
                            root,
                            out,
                            plain,
                            in_task,
                            visited,
                            imports,
                            module_values,
                        );
                    }
                    StructItem::Field(field) if !field.definition => {
                        collect_output_task_paths(
                            &field.value,
                            scopes,
                            root,
                            out,
                            plain,
                            in_task,
                            visited,
                            imports,
                            module_values,
                        );
                    }
                    _ => {}
                }
            }
            visited.remove(name);
        }
        Expr::Struct(items, _) => {
            scopes.push(items);
            for item in items {
                match item {
                    StructItem::Field(field) => {
                        let child_in_task = in_task
                            || is_task_expr(
                                &field.value,
                                scopes,
                                root,
                                &mut std::collections::BTreeSet::new(),
                                imports,
                            );
                        collect_output_task_paths(
                            &field.value,
                            scopes,
                            root,
                            out,
                            plain,
                            child_in_task,
                            visited,
                            imports,
                            module_values,
                        );
                    }
                    StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                        collect_output_task_paths(
                            value,
                            scopes,
                            root,
                            out,
                            plain,
                            in_task,
                            visited,
                            imports,
                            module_values,
                        );
                    }
                    StructItem::For { source, body, .. } => {
                        collect_output_task_paths(
                            source,
                            scopes,
                            root,
                            out,
                            plain,
                            in_task,
                            visited,
                            imports,
                            module_values,
                        );
                        for body_item in body {
                            if let StructItem::Field(field) = body_item {
                                collect_output_task_paths(
                                    &field.value,
                                    scopes,
                                    root,
                                    out,
                                    plain,
                                    in_task,
                                    visited,
                                    imports,
                                    module_values,
                                );
                            }
                        }
                    }
                    StructItem::If { cond, body, .. } => {
                        collect_output_task_paths(
                            cond,
                            scopes,
                            root,
                            out,
                            plain,
                            in_task,
                            visited,
                            imports,
                            module_values,
                        );
                        for body_item in body {
                            if let StructItem::Field(field) = body_item {
                                collect_output_task_paths(
                                    &field.value,
                                    scopes,
                                    root,
                                    out,
                                    plain,
                                    in_task,
                                    visited,
                                    imports,
                                    module_values,
                                );
                            }
                        }
                    }
                    StructItem::Ellipsis(_) => {}
                }
            }
            scopes.pop();
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            collect_output_task_paths(
                left,
                scopes,
                root,
                out,
                plain,
                in_task,
                visited,
                imports,
                module_values,
            );
            collect_output_task_paths(
                right,
                scopes,
                root,
                out,
                plain,
                in_task,
                visited,
                imports,
                module_values,
            );
        }
        Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Bound(_, inner, _)
        | Expr::Unary(_, inner, _) => collect_output_task_paths(
            inner,
            scopes,
            root,
            out,
            plain,
            in_task,
            visited,
            imports,
            module_values,
        ),
        Expr::Index(base, index, _) => {
            collect_output_task_paths(
                base,
                scopes,
                root,
                out,
                plain,
                in_task,
                visited,
                imports,
                module_values,
            );
            collect_output_task_paths(
                index,
                scopes,
                root,
                out,
                plain,
                in_task,
                visited,
                imports,
                module_values,
            );
        }
        Expr::Call(callee, arguments, _) => {
            collect_output_task_paths(
                callee,
                scopes,
                root,
                out,
                plain,
                in_task,
                visited,
                imports,
                module_values,
            );
            for argument in arguments {
                collect_output_task_paths(
                    argument,
                    scopes,
                    root,
                    out,
                    plain,
                    in_task,
                    visited,
                    imports,
                    module_values,
                );
            }
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let ostinato_lang::ast::StrPart::Interp(inner) = part {
                    collect_output_task_paths(
                        inner,
                        scopes,
                        root,
                        out,
                        plain,
                        in_task,
                        visited,
                        imports,
                        module_values,
                    );
                }
            }
        }
        Expr::BytesInterp(parts, _) => {
            for part in parts {
                if let ostinato_lang::ast::BytePart::Interp(inner) = part {
                    collect_output_task_paths(
                        inner,
                        scopes,
                        root,
                        out,
                        plain,
                        in_task,
                        visited,
                        imports,
                        module_values,
                    );
                }
            }
        }
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                collect_output_task_paths(
                    item,
                    scopes,
                    root,
                    out,
                    plain,
                    in_task,
                    visited,
                    imports,
                    module_values,
                );
            }
        }
        Expr::OpenList(Some(inner), _) => collect_output_task_paths(
            inner,
            scopes,
            root,
            out,
            plain,
            in_task,
            visited,
            imports,
            module_values,
        ),
        Expr::ListComp { clauses, body, .. } => {
            for clause in clauses {
                match clause {
                    ostinato_lang::ast::CompClause::For { source, .. } => {
                        collect_output_task_paths(
                            source,
                            scopes,
                            root,
                            out,
                            plain,
                            in_task,
                            visited,
                            imports,
                            module_values,
                        );
                    }
                    ostinato_lang::ast::CompClause::If { cond, .. } => {
                        collect_output_task_paths(
                            cond,
                            scopes,
                            root,
                            out,
                            plain,
                            in_task,
                            visited,
                            imports,
                            module_values,
                        );
                    }
                }
            }
            collect_output_task_paths(
                body,
                scopes,
                root,
                out,
                plain,
                in_task,
                visited,
                imports,
                module_values,
            );
        }
        _ => {}
    }
}

fn value_find_named<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    match value {
        Value::Struct(fields) | Value::Definition { fields, .. } => {
            if let Some(found) = fields.get(name) {
                return Some(found);
            }
            for field in fields.values() {
                if let Some(found) = value_find_named(field, name) {
                    return Some(found);
                }
            }
            if let Value::Definition { definitions, .. } = value {
                if let Some(found) = definitions.get(name) {
                    return Some(found);
                }
                for definition in definitions.values() {
                    if let Some(found) = value_find_named(definition, name) {
                        return Some(found);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn value_find_path<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = value_find_named(current, segment)?;
    }
    Some(current)
}

fn find_field_id_by_path(items: &[StructItem], path: &[String]) -> Option<usize> {
    find_field_id_by_path_in_root(items, path, items)
}

fn find_field_id_by_path_in_root(
    items: &[StructItem],
    path: &[String],
    root: &[StructItem],
) -> Option<usize> {
    let (first, rest) = path.split_first()?;
    for item in items {
        if let StructItem::Field(field) = item
            && matches!(&field.label, Label::Name(label, _) if label == first)
        {
            if rest.is_empty() {
                return Some(field.id);
            }
            if let Some(id) = find_field_id_by_path_in(&field.value, rest, root) {
                return Some(id);
            }
        }
    }
    None
}

fn find_field_id_by_path_in(expr: &Expr, path: &[String], root: &[StructItem]) -> Option<usize> {
    match expr {
        Expr::Struct(items, _) => find_field_id_by_path_in_root(items, path, root),
        Expr::Ident(name, _) => {
            local_binding_location(&[root], root, name).and_then(|(_, item)| match item {
                StructItem::Let { value, .. } => find_field_id_by_path_in(value, path, root),
                StructItem::Field(field) => find_field_id_by_path_in(&field.value, path, root),
                _ => None,
            })
        }
        Expr::Unify(left, right, _) => find_field_id_by_path_in(left, path, root)
            .or_else(|| find_field_id_by_path_in(right, path, root)),
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            find_field_id_by_path_in(inner, path, root)
        }
        _ => None,
    }
}

fn find_field_expr_at_path<'a>(items: &'a [StructItem], path: &[String]) -> Option<&'a Expr> {
    find_field_expr_at_path_in_root(items, path, items)
        .or_else(|| path.last().and_then(|leaf| find_field_expr(items, leaf)))
}

fn find_field_expr_at_path_in_root<'a>(
    items: &'a [StructItem],
    path: &[String],
    root: &'a [StructItem],
) -> Option<&'a Expr> {
    let (first, rest) = path.split_first()?;
    for item in items {
        if let StructItem::Field(field) = item
            && matches!(&field.label, Label::Name(label, _) if label == first)
        {
            if rest.is_empty() {
                return Some(&field.value);
            }
            if let Some(found) = find_field_expr_at_path_in(&field.value, rest, root) {
                return Some(found);
            }
        }
    }
    None
}

fn find_field_expr_at_path_in<'a>(
    expr: &'a Expr,
    path: &[String],
    root: &'a [StructItem],
) -> Option<&'a Expr> {
    match expr {
        Expr::Struct(items, _) => find_field_expr_at_path_in_root(items, path, root),
        Expr::Ident(name, _) => {
            local_binding_location(&[root], root, name).and_then(|(_, item)| match item {
                StructItem::Let { value, .. } => find_field_expr_at_path_in(value, path, root),
                StructItem::Field(field) => find_field_expr_at_path_in(&field.value, path, root),
                _ => None,
            })
        }
        Expr::Unify(left, right, _) => find_field_expr_at_path_in(left, path, root)
            .or_else(|| find_field_expr_at_path_in(right, path, root)),
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            find_field_expr_at_path_in(inner, path, root)
        }
        _ => None,
    }
}

fn find_field_id_by_label(items: &[StructItem], name: &str) -> Option<usize> {
    for item in items {
        match item {
            StructItem::Field(field) => {
                if matches!(&field.label, Label::Name(label, _) if label == name) {
                    return Some(field.id);
                }
                if let Some(id) = find_field_id_by_label_expr(&field.value, name) {
                    return Some(id);
                }
            }
            StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                if let Some(id) = find_field_id_by_label_expr(value, name) {
                    return Some(id);
                }
            }
            StructItem::For { body, .. } | StructItem::If { body, .. } => {
                if let Some(id) = find_field_id_by_label(body, name) {
                    return Some(id);
                }
            }
            StructItem::Ellipsis(_) => {}
        }
    }
    None
}

fn find_field_expr<'a>(items: &'a [StructItem], name: &str) -> Option<&'a Expr> {
    for item in items {
        match item {
            StructItem::Field(field) => {
                if matches!(&field.label, Label::Name(label, _) if label == name) {
                    return Some(&field.value);
                }
                if let Some(found) = find_field_expr_in(&field.value, name) {
                    return Some(found);
                }
            }
            StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                if let Some(found) = find_field_expr_in(value, name) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_field_expr_in<'a>(expr: &'a Expr, name: &str) -> Option<&'a Expr> {
    match expr {
        Expr::Struct(items, _) => find_field_expr(items, name),
        Expr::Unify(left, right, _) => {
            find_field_expr_in(left, name).or_else(|| find_field_expr_in(right, name))
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            find_field_expr_in(inner, name)
        }
        _ => None,
    }
}

fn find_field_id_by_label_expr(expr: &Expr, name: &str) -> Option<usize> {
    match expr {
        Expr::Struct(items, _) => find_field_id_by_label(items, name),
        Expr::Unify(left, right, _) => find_field_id_by_label_expr(left, name)
            .or_else(|| find_field_id_by_label_expr(right, name)),
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            find_field_id_by_label_expr(inner, name)
        }
        _ => None,
    }
}

fn inject_named_outputs(
    items: &mut [StructItem],
    command_name: &str,
    name: &str,
    path: &[String],
    outputs: &[(String, Value)],
    imports: &ImportKinds,
) -> bool {
    let id = find_field_id_by_path(items, path).or_else(|| find_field_id_by_label(items, name));
    let Some(id) = id else {
        return false;
    };
    if !active_task_decl_ids(items, command_name, imports).contains(&id) {
        return false;
    }
    let Some(wrapper) = task_output_struct(outputs, Span::default()) else {
        return false;
    };
    inject_field(items, id, &wrapper)
}

fn inject_named_outputs_any(
    items: &mut [StructItem],
    name: &str,
    path: &[String],
    outputs: &[(String, Value)],
) -> bool {
    let id = find_field_id_by_path(items, path).or_else(|| find_field_id_by_label(items, name));
    let Some(id) = id else {
        return false;
    };
    let Some(wrapper) = task_output_struct(outputs, Span::default()) else {
        return false;
    };
    inject_field(items, id, &wrapper)
}

fn named_field<'a>(items: &'a [StructItem], name: &str) -> Option<&'a Expr> {
    items.iter().find_map(|item| match item {
        StructItem::Field(field) => match &field.label {
            Label::Name(label, _) if label == name => Some(&field.value),
            _ => None,
        },
        _ => None,
    })
}

fn all_definitions_scopes<'a>(
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    name: &str,
) -> Vec<&'a Expr> {
    for scope in scopes.iter().rev() {
        let values = definitions(scope, name);
        if !values.is_empty() {
            return values;
        }
    }
    definitions(root, name)
}

fn all_field_values_scoped<'a>(
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    expr: &'a Expr,
    field: &str,
    visited: &mut std::collections::BTreeSet<String>,
    out: &mut Vec<&'a Expr>,
) {
    match expr {
        Expr::Struct(items, _) => {
            for item in items {
                if let StructItem::Field(field_item) = item
                    && matches!(&field_item.label, Label::Name(name, _) if name == field)
                {
                    out.push(&field_item.value);
                }
            }
        }
        Expr::Unify(left, right, _) => {
            all_field_values_scoped(scopes, root, left, field, visited, out);
            all_field_values_scoped(scopes, root, right, field, visited, out);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            all_field_values_scoped(scopes, root, inner, field, visited, out);
        }
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return;
            }
            for value in all_definitions_scopes(scopes, root, name) {
                all_field_values_scoped(scopes, root, value, field, visited, out);
            }
        }
        Expr::Select(..) | Expr::Index(..) => {
            let mut values = Vec::new();
            resolve_after_values(scopes, root, expr, visited, &mut values);
            for value in values {
                all_field_values_scoped(scopes, root, value, field, visited, out);
            }
        }
        _ => {}
    }
}

fn resolve_after_values<'a>(
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    expr: &'a Expr,
    visited: &mut std::collections::BTreeSet<String>,
    out: &mut Vec<&'a Expr>,
) {
    match expr {
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return;
            }
            for value in all_definitions_scopes(scopes, root, name) {
                resolve_after_values(scopes, root, value, visited, out);
            }
        }
        Expr::Select(base, field, _, _) => {
            let mut bases = Vec::new();
            resolve_after_values(scopes, root, base, visited, &mut bases);
            for base_value in bases {
                all_field_values_scoped(scopes, root, base_value, field, visited, out);
            }
        }
        Expr::Index(base, index, _) => {
            let field = index_field_name(index)
                .or_else(|| resolve_string_value(scopes, root, index, visited));
            let Some(field) = field else {
                return;
            };
            let mut bases = Vec::new();
            resolve_after_values(scopes, root, base, visited, &mut bases);
            for base_value in bases {
                all_field_values_scoped(scopes, root, base_value, &field, visited, out);
            }
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            resolve_after_values(scopes, root, inner, visited, out);
        }
        Expr::Unify(left, right, _) => {
            resolve_after_values(scopes, root, left, visited, out);
            resolve_after_values(scopes, root, right, visited, out);
        }
        Expr::Struct(..) => out.push(expr),
        _ => {}
    }
}

fn after_exprs<'a>(
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    expr: &'a Expr,
    visited: &mut std::collections::BTreeSet<String>,
    out: &mut Vec<&'a Expr>,
) {
    match expr {
        Expr::Struct(items, _) => {
            for item in items {
                if let StructItem::Field(field) = item
                    && matches!(&field.label, Label::Name(name, _) if name == "$after")
                {
                    out.push(&field.value);
                }
            }
        }
        Expr::Unify(left, right, _) => {
            after_exprs(scopes, root, left, visited, out);
            after_exprs(scopes, root, right, visited, out);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            after_exprs(scopes, root, inner, visited, out);
        }
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return;
            }
            for value in all_definitions_scopes(scopes, root, name) {
                after_exprs(scopes, root, value, visited, out);
            }
        }
        Expr::Select(..) | Expr::Index(..) => {
            let mut values = Vec::new();
            resolve_after_values(scopes, root, expr, visited, &mut values);
            for value in values {
                after_exprs(scopes, root, value, visited, out);
            }
        }
        _ => {}
    }
}

fn after_names(
    item: &StructItem,
    root: &[StructItem],
    scopes: &[&[StructItem]],
) -> Result<Vec<String>, ostinato_lang::LangError> {
    let StructItem::Field(field) = item else {
        return Ok(Vec::new());
    };
    after_names_of(&field.value, root, scopes)
}

/// Internal schedule key for a task declared outside the command subtree.
fn package_task_key(path: &[String]) -> String {
    format!("@pkg:{}", path.join("/"))
}

fn after_names_of(
    expr: &Expr,
    root: &[StructItem],
    scopes: &[&[StructItem]],
) -> Result<Vec<String>, ostinato_lang::LangError> {
    let mut exprs = Vec::new();
    after_exprs(
        scopes,
        root,
        expr,
        &mut std::collections::BTreeSet::new(),
        &mut exprs,
    );
    let mut names = Vec::new();
    for expr in exprs {
        let mut paths = Vec::new();
        after_paths(
            expr,
            scopes,
            root,
            &mut std::collections::BTreeSet::new(),
            &mut paths,
        )?;
        for (path, root_bound) in paths {
            let name = if root_bound {
                Some(package_task_key(&path))
            } else {
                path.last().cloned()
            };
            if let Some(name) = name
                && !names.contains(&name)
            {
                names.push(name);
            }
        }
    }
    Ok(names)
}

/// Resolve one `$after` operand into task paths and whether the path is rooted
/// outside the current (command) scope.
fn after_paths(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    visited: &mut std::collections::BTreeSet<String>,
    out: &mut Vec<(Vec<String>, bool)>,
) -> Result<(), ostinato_lang::LangError> {
    match expr {
        Expr::List(items, _) => {
            for item in items {
                after_paths(item, scopes, root, visited, out)?;
            }
            Ok(())
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            after_paths(inner, scopes, root, visited, out)
        }
        Expr::Ident(..) | Expr::Select(..) | Expr::Index(..) => {
            if let Some(path) = resolve_after_path(expr, scopes, root, visited) {
                out.push(path);
            }
            Ok(())
        }
        other => Err(ostinato_lang::LangError::Workflow(format!(
            "unsupported $after expression: {other:?}"
        ))),
    }
}

/// Resolve an `$after` reference through lets, aliases, definitions and
/// containers.
fn resolve_after_path(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    visited: &mut std::collections::BTreeSet<String>,
) -> Option<(Vec<String>, bool)> {
    match expr {
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return None;
            }
            let resolved = match local_binding_location(scopes, root, name) {
                Some((root_bound, StructItem::Let { value, .. })) => {
                    let mut found = resolve_after_path(value, scopes, root, visited);
                    if let Some((_, bound)) = found.as_mut() {
                        *bound = *bound || root_bound;
                    }
                    found
                }
                Some((root_bound, StructItem::Field(field))) => {
                    let label = match &field.label {
                        Label::Name(label, _) => label.clone(),
                        _ => return None,
                    };
                    if !(field.definition || root_bound) {
                        // A command-local field is an independent task
                        // instance; do not collapse it into its target.
                        Some((vec![label], false))
                    } else {
                        let mut value = &field.value;
                        while let Expr::Paren(inner, _)
                        | Expr::Default(inner, _)
                        | Expr::Bound(_, inner, _) = value
                        {
                            value = inner;
                        }
                        if matches!(value, Expr::Ident(..) | Expr::Select(..) | Expr::Index(..))
                            && let Some(alias) = resolve_after_path(value, scopes, root, visited)
                        {
                            Some(alias)
                        } else {
                            Some((vec![label], root_bound))
                        }
                    }
                }
                Some((root_bound, _)) => Some((vec![name.clone()], root_bound)),
                None if name.starts_with('#') => Some((vec![name.clone()], true)),
                None => None,
            };
            visited.remove(name);
            resolved
        }
        Expr::Select(base, field, _, _) => {
            let (mut path, root_bound) = resolve_after_path(base, scopes, root, visited)?;
            path.push(field.clone());
            Some((path, root_bound))
        }
        Expr::Index(base, index, _) => {
            let segment = index_field_name(index)?;
            let (mut path, root_bound) = resolve_after_path(base, scopes, root, visited)?;
            path.push(segment);
            Some((path, root_bound))
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            resolve_after_path(inner, scopes, root, visited)
        }
        Expr::Unify(left, right, _) => resolve_after_path(left, scopes, root, visited)
            .or_else(|| resolve_after_path(right, scopes, root, visited)),
        _ => None,
    }
}

/// Drop `$after` operands that are defined but not tasks, matching CUE's
/// "ignore non-task references" behaviour while keeping undefined references
/// so evaluation still reports them.
fn sanitize_after_fields(
    items: &mut [StructItem],
    imports: &ImportKinds,
    imported_values: &BTreeMap<String, Value>,
) {
    let mut plan: std::collections::BTreeMap<usize, Expr> = std::collections::BTreeMap::new();
    {
        let root: &[StructItem] = items;
        let mut scopes: Vec<&[StructItem]> = Vec::new();
        plan_after_fields(
            items,
            root,
            imports,
            imported_values,
            &mut scopes,
            &mut plan,
        );
    }
    apply_after_plan(items, &plan);
}

fn plan_after_fields<'a>(
    items: &'a [StructItem],
    root: &'a [StructItem],
    imports: &ImportKinds,
    imported_values: &BTreeMap<String, Value>,
    scopes: &mut Vec<&'a [StructItem]>,
    plan: &mut std::collections::BTreeMap<usize, Expr>,
) {
    scopes.push(items);
    for item in items {
        match item {
            StructItem::Field(field) => {
                if matches!(&field.label, Label::Name(name, _) if name == "$after") {
                    let kept =
                        filter_after_operands(&field.value, scopes, root, imports, imported_values);
                    plan.insert(field.id, Expr::List(kept, field.value.span()));
                } else {
                    plan_after_expr(&field.value, root, imports, imported_values, scopes, plan);
                }
            }
            StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                plan_after_expr(value, root, imports, imported_values, scopes, plan);
            }
            StructItem::For { source, body, .. } => {
                plan_after_expr(source, root, imports, imported_values, scopes, plan);
                for body_item in body {
                    plan_after_item(body_item, root, imports, imported_values, scopes, plan);
                }
            }
            StructItem::If { cond, body, .. } => {
                plan_after_expr(cond, root, imports, imported_values, scopes, plan);
                for body_item in body {
                    plan_after_item(body_item, root, imports, imported_values, scopes, plan);
                }
            }
            StructItem::Ellipsis(_) => {}
        }
    }
    scopes.pop();
}

fn plan_after_item<'a>(
    item: &'a StructItem,
    root: &'a [StructItem],
    imports: &ImportKinds,
    imported_values: &BTreeMap<String, Value>,
    scopes: &mut Vec<&'a [StructItem]>,
    plan: &mut std::collections::BTreeMap<usize, Expr>,
) {
    match item {
        StructItem::Field(field) => {
            if matches!(&field.label, Label::Name(name, _) if name == "$after") {
                let kept =
                    filter_after_operands(&field.value, scopes, root, imports, imported_values);
                plan.insert(field.id, Expr::List(kept, field.value.span()));
            } else {
                plan_after_expr(&field.value, root, imports, imported_values, scopes, plan);
            }
        }
        StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
            plan_after_expr(value, root, imports, imported_values, scopes, plan);
        }
        _ => {}
    }
}

fn plan_after_expr<'a>(
    expr: &'a Expr,
    root: &'a [StructItem],
    imports: &ImportKinds,
    imported_values: &BTreeMap<String, Value>,
    scopes: &mut Vec<&'a [StructItem]>,
    plan: &mut std::collections::BTreeMap<usize, Expr>,
) {
    match expr {
        Expr::Struct(items, _) => {
            plan_after_fields(items, root, imports, imported_values, scopes, plan);
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            plan_after_expr(left, root, imports, imported_values, scopes, plan);
            plan_after_expr(right, root, imports, imported_values, scopes, plan);
        }
        Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Bound(_, inner, _)
        | Expr::Unary(_, inner, _) => {
            plan_after_expr(inner, root, imports, imported_values, scopes, plan);
        }
        Expr::Index(base, index, _) => {
            plan_after_expr(base, root, imports, imported_values, scopes, plan);
            plan_after_expr(index, root, imports, imported_values, scopes, plan);
        }
        Expr::Call(callee, arguments, _) => {
            plan_after_expr(callee, root, imports, imported_values, scopes, plan);
            for argument in arguments {
                plan_after_expr(argument, root, imports, imported_values, scopes, plan);
            }
        }
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                plan_after_expr(item, root, imports, imported_values, scopes, plan);
            }
        }
        Expr::OpenList(Some(inner), _) => {
            plan_after_expr(inner, root, imports, imported_values, scopes, plan);
        }
        _ => {}
    }
}

fn filter_after_operands(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &ImportKinds,
    imported_values: &BTreeMap<String, Value>,
) -> Vec<Expr> {
    match expr {
        Expr::List(items, _) => items
            .iter()
            .flat_map(|item| filter_after_operands(item, scopes, root, imports, imported_values))
            .collect(),
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            filter_after_operands(inner, scopes, root, imports, imported_values)
        }
        Expr::Unify(left, right, _) => {
            let mut kept = filter_after_operands(left, scopes, root, imports, imported_values);
            kept.extend(filter_after_operands(
                right,
                scopes,
                root,
                imports,
                imported_values,
            ));
            kept
        }
        Expr::Select(..) | Expr::Index(..) => {
            if let Some(prefix) = after_task_prefix(expr, scopes, root, imports) {
                return vec![prefix];
            }
            match classify_after_selector(expr, scopes, root, imported_values) {
                AfterSelector::KeepFull => vec![expr.clone()],
                AfterSelector::KeepRoot(name) => {
                    vec![Expr::Ident(name, expr.span())]
                }
                AfterSelector::Drop => Vec::new(),
            }
        }
        Expr::Ident(..) => {
            if is_task_expr(
                expr,
                scopes,
                root,
                &mut std::collections::BTreeSet::new(),
                imports,
            ) || keep_after_reference(expr, scopes, root)
            {
                vec![expr.clone()]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

enum AfterSelector {
    KeepFull,
    KeepRoot(String),
    Drop,
}

#[derive(Clone, Copy)]
enum AfterVal<'e, 'v> {
    Expr(&'e Expr),
    Value(&'v Value),
}

/// Decide how to keep a non-task selector operand. Missing members of a
/// struct/import are ignored (the root keeps the binding "used"); missing
/// members of concrete values stay so evaluation reports CUE's error.
fn classify_after_selector(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imported_values: &BTreeMap<String, Value>,
) -> AfterSelector {
    let Some(path) = selector_names(expr, None) else {
        return AfterSelector::KeepFull;
    };
    let Some((root_name, rest)) = path.split_first() else {
        return AfterSelector::KeepFull;
    };
    match local_binding_location(scopes, root, root_name) {
        Some((_, StructItem::Field(field))) => {
            if field.definition {
                return AfterSelector::KeepFull;
            }
            walk_after_members(
                AfterVal::Expr(&field.value),
                rest,
                scopes,
                root,
                imported_values,
                AfterSelector::Drop,
                AfterSelector::Drop,
            )
        }
        Some((_, StructItem::Let { value, .. })) => walk_after_members(
            AfterVal::Expr(value),
            rest,
            scopes,
            root,
            imported_values,
            AfterSelector::KeepRoot(root_name.clone()),
            AfterSelector::KeepFull,
        ),
        Some(_) => AfterSelector::KeepFull,
        None => {
            let Some(value) = imported_values.get(root_name) else {
                return AfterSelector::KeepFull;
            };
            walk_after_members(
                AfterVal::Value(value),
                rest,
                scopes,
                root,
                imported_values,
                AfterSelector::KeepRoot(root_name.clone()),
                AfterSelector::KeepFull,
            )
        }
    }
}

fn walk_after_members<'e, 'v>(
    mut current: AfterVal<'e, 'v>,
    rest: &[String],
    scopes: &[&'e [StructItem]],
    root: &'e [StructItem],
    imported_values: &'v BTreeMap<String, Value>,
    on_structish: AfterSelector,
    on_resolved: AfterSelector,
) -> AfterSelector {
    for segment in rest {
        match advance_after_member(current, segment, scopes, root, imported_values) {
            Some(next) => current = next,
            None => {
                let structish = match current {
                    AfterVal::Expr(expr) => ast_is_structish(expr, scopes, root, imported_values),
                    AfterVal::Value(value) => is_struct_value(value),
                };
                return if structish {
                    on_structish
                } else {
                    AfterSelector::KeepFull
                };
            }
        }
    }
    on_resolved
}

fn advance_after_member<'e, 'v>(
    current: AfterVal<'e, 'v>,
    segment: &str,
    scopes: &[&'e [StructItem]],
    root: &'e [StructItem],
    imported_values: &'v BTreeMap<String, Value>,
) -> Option<AfterVal<'e, 'v>> {
    match current {
        AfterVal::Expr(expr) => {
            if let Some(next) = select_field_value(
                scopes,
                root,
                expr,
                segment,
                &mut std::collections::BTreeSet::new(),
            ) {
                return Some(AfterVal::Expr(next));
            }
            let value = imported_value_path(expr, scopes, root, imported_values)?;
            value_field(value, segment).map(AfterVal::Value)
        }
        AfterVal::Value(value) => value_field(value, segment).map(AfterVal::Value),
    }
}

fn imported_value_path<'e, 'v>(
    expr: &'e Expr,
    scopes: &[&'e [StructItem]],
    root: &'e [StructItem],
    imported_values: &'v BTreeMap<String, Value>,
) -> Option<&'v Value> {
    imported_value_path_inner(
        expr,
        scopes,
        root,
        imported_values,
        &mut std::collections::BTreeSet::new(),
    )
}

fn imported_value_path_inner<'e, 'v>(
    expr: &'e Expr,
    scopes: &[&'e [StructItem]],
    root: &'e [StructItem],
    imported_values: &'v BTreeMap<String, Value>,
    visited: &mut std::collections::BTreeSet<String>,
) -> Option<&'v Value> {
    if let Expr::Ident(name, _) = expr
        && !visited.insert(name.clone())
    {
        return None;
    }
    if let Some(value) =
        resolve_local_value(scopes, root, expr, &mut std::collections::BTreeSet::new())
        && !std::ptr::eq(value, expr)
    {
        return imported_value_path_inner(value, scopes, root, imported_values, visited);
    }
    let path = selector_names(expr, None)?;
    let (root_name, rest) = path.split_first()?;
    let mut value = imported_values.get(root_name)?;
    for segment in rest {
        value = value_field(value, segment)?;
    }
    Some(value)
}

fn inject_module_outputs(
    module_values: &mut BTreeMap<String, Value>,
    path: &[String],
    outputs: &[(String, Value)],
) -> bool {
    let Some((root_name, rest)) = path.split_first() else {
        return false;
    };
    let Some(value) = module_values.get_mut(root_name) else {
        return false;
    };
    let Some(target) = module_value_mut(value, rest) else {
        return false;
    };
    let Some(fields) = value_fields_mut(target) else {
        return false;
    };
    for (name, output) in outputs {
        fields.insert(name.clone(), output.clone());
    }
    true
}

fn module_value_mut<'a>(value: &'a mut Value, path: &[String]) -> Option<&'a mut Value> {
    let mut current = value;
    for segment in path {
        current = match current {
            Value::Struct(fields) => fields.get_mut(segment)?,
            Value::Definition {
                fields,
                definitions,
                ..
            } => {
                if fields.contains_key(segment) {
                    fields.get_mut(segment)?
                } else {
                    definitions.get_mut(segment)?
                }
            }
            _ => return None,
        };
    }
    Some(current)
}

fn value_fields_mut(value: &mut Value) -> Option<&mut BTreeMap<String, Value>> {
    match value {
        Value::Struct(fields) => Some(fields),
        Value::Definition { fields, .. } => Some(fields),
        _ => None,
    }
}

fn value_field<'a>(value: &'a Value, field: &str) -> Option<&'a Value> {
    match value {
        Value::Struct(fields) => fields.get(field),
        Value::Definition {
            fields,
            definitions,
            ..
        } => fields.get(field).or_else(|| definitions.get(field)),
        _ => None,
    }
}

fn is_struct_value(value: &Value) -> bool {
    matches!(value, Value::Struct(_) | Value::Definition { .. })
}

fn ast_is_structish(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imported_values: &BTreeMap<String, Value>,
) -> bool {
    ast_is_structish_inner(
        expr,
        scopes,
        root,
        imported_values,
        &mut std::collections::BTreeSet::new(),
    )
}

fn ast_is_structish_inner(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imported_values: &BTreeMap<String, Value>,
    visited: &mut std::collections::BTreeSet<String>,
) -> bool {
    match expr {
        Expr::Struct(..) | Expr::Paren(..) | Expr::Default(..) | Expr::Bound(..) => true,
        Expr::Unify(left, right, _) => {
            ast_is_structish_inner(left, scopes, root, imported_values, visited)
                || ast_is_structish_inner(right, scopes, root, imported_values, visited)
        }
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return false;
            }
            let result = match local_binding_location(scopes, root, name) {
                Some((_, StructItem::Field(field))) => {
                    ast_is_structish_inner(&field.value, scopes, root, imported_values, visited)
                }
                Some((_, StructItem::Let { value, .. })) => {
                    ast_is_structish_inner(value, scopes, root, imported_values, visited)
                }
                _ => imported_values.get(name).is_some_and(is_struct_value),
            };
            visited.remove(name);
            result
        }
        Expr::Select(..) | Expr::Index(..) => {
            if let Some(value) =
                resolve_local_value(scopes, root, expr, &mut std::collections::BTreeSet::new())
            {
                return ast_is_structish_inner(value, scopes, root, imported_values, visited);
            }
            let Some(path) = selector_names(expr, None) else {
                return false;
            };
            let Some((root_name, rest)) = path.split_first() else {
                return false;
            };
            let Some(mut value) = imported_values.get(root_name) else {
                return false;
            };
            for segment in rest {
                match value_field(value, segment) {
                    Some(next) => value = next,
                    None => return false,
                }
            }
            is_struct_value(value)
        }
        _ => false,
    }
}

/// Longest base of a selector that resolves to a task.
///
/// CUE schedules the task that owns the selected field, so `$after:
/// prep.stdout` means "after prep", and `$after: group.prep.stdout` means
/// "after group.prep".
fn after_task_prefix(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &ImportKinds,
) -> Option<Expr> {
    match expr {
        Expr::Select(base, _, _, _) | Expr::Index(base, _, _) => {
            if let Some(value) =
                resolve_local_value(scopes, root, expr, &mut std::collections::BTreeSet::new())
                && is_task_expr(
                    value,
                    scopes,
                    root,
                    &mut std::collections::BTreeSet::new(),
                    imports,
                )
            {
                return Some(expr.clone());
            }
            if let Some(value) =
                resolve_local_value(scopes, root, base, &mut std::collections::BTreeSet::new())
                && is_task_expr(
                    value,
                    scopes,
                    root,
                    &mut std::collections::BTreeSet::new(),
                    imports,
                )
            {
                return Some((**base).clone());
            }
            after_task_prefix(base, scopes, root, imports)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            after_task_prefix(inner, scopes, root, imports)
        }
        _ => None,
    }
}

/// Keep references that must remain visible to the evaluator even though they
/// are not tasks: lets and imports keep their "used" status, and undefined
/// roots still produce CUE's reference error.
fn keep_after_reference(expr: &Expr, scopes: &[&[StructItem]], root: &[StructItem]) -> bool {
    match selector_root_ident(expr) {
        Some(name) => !matches!(
            local_binding_location(scopes, root, name),
            Some((_, StructItem::Field(_)))
        ),
        None => false,
    }
}

fn apply_after_plan(items: &mut [StructItem], plan: &std::collections::BTreeMap<usize, Expr>) {
    for item in items {
        match item {
            StructItem::Field(field) => {
                if let Some(expr) = plan.get(&field.id) {
                    field.value = expr.clone();
                }
                apply_after_expr(&mut field.value, plan);
            }
            StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                apply_after_expr(value, plan);
            }
            StructItem::For { source, body, .. } => {
                apply_after_expr(source, plan);
                for body_item in body {
                    apply_after_plan_item(body_item, plan);
                }
            }
            StructItem::If { cond, body, .. } => {
                apply_after_expr(cond, plan);
                for body_item in body {
                    apply_after_plan_item(body_item, plan);
                }
            }
            StructItem::Ellipsis(_) => {}
        }
    }
}

fn apply_after_plan_item(item: &mut StructItem, plan: &std::collections::BTreeMap<usize, Expr>) {
    match item {
        StructItem::Field(field) => {
            if let Some(expr) = plan.get(&field.id) {
                field.value = expr.clone();
            }
            apply_after_expr(&mut field.value, plan);
        }
        StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
            apply_after_expr(value, plan);
        }
        _ => {}
    }
}

fn apply_after_expr(expr: &mut Expr, plan: &std::collections::BTreeMap<usize, Expr>) {
    match expr {
        Expr::Struct(items, _) => apply_after_plan(items, plan),
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            apply_after_expr(left, plan);
            apply_after_expr(right, plan);
        }
        Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Bound(_, inner, _)
        | Expr::Unary(_, inner, _) => apply_after_expr(inner, plan),
        Expr::Index(base, index, _) => {
            apply_after_expr(base, plan);
            apply_after_expr(index, plan);
        }
        Expr::Call(callee, arguments, _) => {
            apply_after_expr(callee, plan);
            for argument in arguments {
                apply_after_expr(argument, plan);
            }
        }
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                apply_after_expr(item, plan);
            }
        }
        Expr::OpenList(Some(inner), _) => apply_after_expr(inner, plan),
        _ => {}
    }
}

/// Collect package-rooted `$after` paths from command task values.
fn collect_package_after_paths(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &ImportKinds,
    in_task: bool,
    out: &mut Vec<Vec<String>>,
) -> Result<(), ostinato_lang::LangError> {
    match expr {
        Expr::Struct(items, _) => {
            for item in items {
                let StructItem::Field(field) = item else {
                    continue;
                };
                let Label::Name(label, _) = &field.label else {
                    continue;
                };
                if label == "$after" {
                    if in_task {
                        let mut paths = Vec::new();
                        after_paths(
                            &field.value,
                            scopes,
                            root,
                            &mut std::collections::BTreeSet::new(),
                            &mut paths,
                        )?;
                        for (path, root_bound) in paths {
                            if root_bound {
                                out.push(path);
                            }
                        }
                    }
                    continue;
                }
                let child_is_task = is_task_expr(
                    &field.value,
                    scopes,
                    root,
                    &mut std::collections::BTreeSet::new(),
                    imports,
                );
                collect_package_after_paths(
                    &field.value,
                    scopes,
                    root,
                    imports,
                    child_is_task,
                    out,
                )?;
            }
            Ok(())
        }
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            collect_package_after_paths(left, scopes, root, imports, in_task, out)?;
            collect_package_after_paths(right, scopes, root, imports, in_task, out)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            collect_package_after_paths(inner, scopes, root, imports, in_task, out)
        }
        _ => Ok(()),
    }
}

/// Struct scopes (with their container paths) enclosing a task path.
fn scopes_for_path<'a>(
    root: &'a [StructItem],
    path: &[String],
) -> Vec<(&'a [StructItem], Vec<String>)> {
    let mut scopes: Vec<(&'a [StructItem], Vec<String>)> = vec![(root, Vec::new())];
    for segment in path.iter().take(path.len().saturating_sub(1)) {
        let Some((items, prefix)) = scopes.last() else {
            break;
        };
        let Some(value) = items.iter().find_map(|item| match item {
            StructItem::Field(field)
                if matches!(&field.label, Label::Name(label, _) if label == segment) =>
            {
                Some(&field.value)
            }
            _ => None,
        }) else {
            break;
        };
        let mut next_prefix = prefix.clone();
        next_prefix.push(segment.clone());
        let Some(struct_items) = struct_items_of(value) else {
            break;
        };
        scopes.push((struct_items, next_prefix));
    }
    scopes
}

fn struct_items_of(expr: &Expr) -> Option<&[StructItem]> {
    match expr {
        Expr::Struct(items, _) => Some(items),
        Expr::Unify(left, right, _) => struct_items_of(left).or_else(|| struct_items_of(right)),
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            struct_items_of(inner)
        }
        _ => None,
    }
}

fn find_scoped_binding<'a>(
    scopes: &'a [(&'a [StructItem], Vec<String>)],
    name: &str,
) -> Option<(&'a StructItem, &'a [String])> {
    for (items, prefix) in scopes.iter().rev() {
        if let Some(item) = items.iter().find(|item| binding_matches(item, name)) {
            return Some((item, prefix));
        }
    }
    None
}

fn find_scoped_value<'a>(
    scopes: &'a [(&'a [StructItem], Vec<String>)],
    name: &str,
) -> Option<&'a Expr> {
    match find_scoped_binding(scopes, name)?.0 {
        StructItem::Field(field) => Some(&field.value),
        StructItem::Let { value, .. } => Some(value),
        _ => None,
    }
}

/// Resolve a package-scope `$after` operand to its full container path.
fn resolve_package_path(
    expr: &Expr,
    scopes: &[(&[StructItem], Vec<String>)],
    visited: &mut std::collections::BTreeSet<String>,
) -> Option<Vec<String>> {
    match expr {
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return None;
            }
            let resolved = match find_scoped_binding(scopes, name) {
                Some((item, prefix)) => {
                    let label = name.clone();
                    match item {
                        StructItem::Let { value, .. } => {
                            resolve_package_path(value, scopes, visited)
                        }
                        StructItem::Field(field) => {
                            let mut value = &field.value;
                            while let Expr::Paren(inner, _)
                            | Expr::Default(inner, _)
                            | Expr::Bound(_, inner, _) = value
                            {
                                value = inner;
                            }
                            if matches!(value, Expr::Ident(..) | Expr::Select(..) | Expr::Index(..))
                                && let Some(alias) = resolve_package_path(value, scopes, visited)
                            {
                                Some(alias)
                            } else {
                                let mut path = prefix.to_vec();
                                path.push(label);
                                Some(path)
                            }
                        }
                        _ => None,
                    }
                }
                None if name.starts_with('#') => Some(vec![name.clone()]),
                None => None,
            };
            visited.remove(name);
            resolved
        }
        Expr::Select(base, field, _, _) => {
            let mut path = resolve_package_path(base, scopes, visited)?;
            path.push(field.clone());
            Some(path)
        }
        Expr::Index(base, index, _) => {
            let segment = index_field_name(index)?;
            let mut path = resolve_package_path(base, scopes, visited)?;
            path.push(segment);
            Some(path)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            resolve_package_path(inner, scopes, visited)
        }
        _ => None,
    }
}

fn package_after_paths(
    expr: &Expr,
    scopes: &[(&[StructItem], Vec<String>)],
    visited: &mut std::collections::BTreeSet<String>,
    out: &mut Vec<Vec<String>>,
) -> Result<(), ostinato_lang::LangError> {
    match expr {
        Expr::List(items, _) => {
            for item in items {
                package_after_paths(item, scopes, visited, out)?;
            }
            Ok(())
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            package_after_paths(inner, scopes, visited, out)
        }
        Expr::Ident(..) | Expr::Select(..) | Expr::Index(..) => {
            if let Some(path) = resolve_package_path(expr, scopes, visited) {
                out.push(path);
            }
            Ok(())
        }
        other => Err(ostinato_lang::LangError::Workflow(format!(
            "unsupported $after expression: {other:?}"
        ))),
    }
}

/// Collect `$after` paths from a package task value, resolving references in
/// the task's enclosing scopes and through inherited definitions.
#[allow(clippy::too_many_arguments)]
fn collect_package_task_after_paths(
    expr: &Expr,
    scopes: &[(&[StructItem], Vec<String>)],
    root: &[StructItem],
    imports: &ImportKinds,
    in_task: bool,
    visited: &mut std::collections::BTreeSet<String>,
    out: &mut Vec<Vec<String>>,
) -> Result<(), ostinato_lang::LangError> {
    match expr {
        Expr::Struct(items, _) => {
            let scope_items = scopes.last().map_or(root, |(items, _)| *items);
            for item in items {
                let StructItem::Field(field) = item else {
                    continue;
                };
                let Label::Name(label, _) = &field.label else {
                    continue;
                };
                if label == "$after" {
                    if in_task {
                        package_after_paths(
                            &field.value,
                            scopes,
                            &mut std::collections::BTreeSet::new(),
                            out,
                        )?;
                    }
                    continue;
                }
                let child_is_task = is_task_expr(
                    &field.value,
                    &[scope_items],
                    root,
                    &mut std::collections::BTreeSet::new(),
                    imports,
                );
                collect_package_task_after_paths(
                    &field.value,
                    scopes,
                    root,
                    imports,
                    child_is_task,
                    visited,
                    out,
                )?;
            }
            Ok(())
        }
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            collect_package_task_after_paths(left, scopes, root, imports, in_task, visited, out)?;
            collect_package_task_after_paths(right, scopes, root, imports, in_task, visited, out)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            collect_package_task_after_paths(inner, scopes, root, imports, in_task, visited, out)
        }
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return Ok(());
            }
            if let Some(value) = find_scoped_value(scopes, name) {
                collect_package_task_after_paths(
                    value, scopes, root, imports, in_task, visited, out,
                )?;
            }
            visited.remove(name);
            Ok(())
        }
        Expr::Select(..) | Expr::Index(..) => {
            let scope_slices: Vec<&[StructItem]> = scopes.iter().map(|(items, _)| *items).collect();
            let mut values = Vec::new();
            resolve_after_values(
                &scope_slices,
                root,
                expr,
                &mut std::collections::BTreeSet::new(),
                &mut values,
            );
            for value in values {
                collect_package_task_after_paths(
                    value, scopes, root, imports, in_task, visited, out,
                )?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

type PackageReadOrder =
    std::collections::BTreeMap<Vec<String>, (Option<usize>, Option<usize>, Option<usize>)>;

fn package_read_order(
    items: &[StructItem],
    root: &[StructItem],
    imports: &ImportKinds,
    claimed: &std::collections::BTreeSet<Vec<String>>,
) -> PackageReadOrder {
    let scopes: [&[StructItem]; 1] = [items];
    let mut order = PackageReadOrder::new();
    let mut counter = 0usize;
    for item in items {
        let StructItem::Field(field) = item else {
            continue;
        };
        let index = counter;
        counter += 1;
        let visible = !(field.definition
            || field.optional
            || matches!(&field.label, Label::Name(name, _) if !field.quoted && name.starts_with('_')));
        let child_in_task = is_task_expr(
            &field.value,
            &scopes,
            root,
            &mut std::collections::BTreeSet::new(),
            imports,
        );
        scan_package_order_expr(
            &field.value,
            true,
            visible,
            child_in_task,
            index,
            &scopes,
            root,
            imports,
            claimed,
            &mut order,
            &mut counter,
            &mut std::collections::BTreeSet::new(),
        );
    }
    order
}

/// Record package alias declarations, plain output reads and task-input output
/// reads in command source order. Task-input reads matter because CUE keeps a
/// declaration-first task-input read non-concrete even when a later plain read
/// could be satisfied by the alias instance.
#[allow(clippy::too_many_arguments)]
fn scan_package_order_expr(
    expr: &Expr,
    allow_read: bool,
    allow_decl: bool,
    in_task: bool,
    index: usize,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &ImportKinds,
    claimed: &std::collections::BTreeSet<Vec<String>>,
    order: &mut PackageReadOrder,
    counter: &mut usize,
    visited: &mut std::collections::BTreeSet<String>,
) {
    match expr {
        Expr::Select(base, field, _, _) => {
            if matches!(field.as_str(), "stdout" | "stderr" | "success") {
                if allow_read
                    && let Some(path) = resolved_read_path(
                        base,
                        scopes,
                        root,
                        &mut std::collections::BTreeSet::new(),
                        imports,
                    )
                    && claimed.contains(&path)
                {
                    let entry = order.entry(path).or_insert((None, None, None));
                    let slot = if in_task { &mut entry.2 } else { &mut entry.1 };
                    *slot = Some(slot.map_or(index, |existing| existing.min(index)));
                }
            } else {
                record_package_declaration(
                    expr, allow_decl, index, scopes, root, imports, claimed, order,
                );
                if allow_read && let Some(value) = selected_alias_value(expr, scopes, root) {
                    scan_package_order_expr(
                        value, true, false, in_task, index, scopes, root, imports, claimed, order,
                        counter, visited,
                    );
                }
            }
            scan_package_order_expr(
                base, false, false, in_task, index, scopes, root, imports, claimed, order, counter,
                visited,
            );
        }
        Expr::Ident(name, _) => {
            record_package_declaration(
                expr, allow_decl, index, scopes, root, imports, claimed, order,
            );
            if !visited.insert(name.clone()) {
                return;
            }
            match local_binding(scopes, root, name) {
                Some(StructItem::Let { value, .. })
                | Some(StructItem::Field(Field { value, .. }))
                    if !matches!(
                        local_binding(scopes, root, name),
                        Some(StructItem::Field(field)) if field.definition
                    ) =>
                {
                    scan_package_order_expr(
                        value, allow_read, allow_decl, in_task, index, scopes, root, imports,
                        claimed, order, counter, visited,
                    );
                }
                _ => {
                    record_package_declaration(
                        expr, allow_decl, index, scopes, root, imports, claimed, order,
                    );
                }
            }
            visited.remove(name);
        }
        Expr::Index(base, _, _) => {
            record_package_declaration(
                expr, allow_decl, index, scopes, root, imports, claimed, order,
            );
            if allow_read && let Some(value) = selected_alias_value(expr, scopes, root) {
                scan_package_order_expr(
                    value, true, false, in_task, index, scopes, root, imports, claimed, order,
                    counter, visited,
                );
            }
            scan_package_order_expr(
                base, false, false, in_task, index, scopes, root, imports, claimed, order, counter,
                visited,
            );
        }
        Expr::Struct(items, _) => {
            let scope_items = items.clone();
            let mut nested = scopes.to_vec();
            nested.push(&scope_items);
            for item in items {
                let StructItem::Field(field) = item else {
                    continue;
                };
                let field_index = *counter;
                *counter += 1;
                let visible = !(field.definition
                    || field.optional
                    || matches!(&field.label, Label::Name(name, _) if !field.quoted && name.starts_with('_')));
                let child_in_task = in_task
                    || is_task_expr(
                        &field.value,
                        &nested,
                        root,
                        &mut std::collections::BTreeSet::new(),
                        imports,
                    );
                scan_package_order_expr(
                    &field.value,
                    true,
                    visible,
                    child_in_task,
                    field_index,
                    &nested,
                    root,
                    imports,
                    claimed,
                    order,
                    counter,
                    visited,
                );
            }
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            scan_package_order_expr(
                left, allow_read, allow_decl, in_task, index, scopes, root, imports, claimed,
                order, counter, visited,
            );
            scan_package_order_expr(
                right, allow_read, allow_decl, in_task, index, scopes, root, imports, claimed,
                order, counter, visited,
            );
        }
        Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Bound(_, inner, _)
        | Expr::Unary(_, inner, _) => scan_package_order_expr(
            inner, allow_read, allow_decl, in_task, index, scopes, root, imports, claimed, order,
            counter, visited,
        ),
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                scan_package_order_expr(
                    item, allow_read, allow_decl, in_task, index, scopes, root, imports, claimed,
                    order, counter, visited,
                );
            }
        }
        Expr::OpenList(Some(inner), _) => scan_package_order_expr(
            inner, allow_read, allow_decl, in_task, index, scopes, root, imports, claimed, order,
            counter, visited,
        ),
        Expr::Str(parts, _) => {
            for part in parts {
                if let ostinato_lang::ast::StrPart::Interp(inner) = part {
                    scan_package_order_expr(
                        inner, true, true, in_task, index, scopes, root, imports, claimed, order,
                        counter, visited,
                    );
                }
            }
        }
        Expr::BytesInterp(parts, _) => {
            for part in parts {
                if let ostinato_lang::ast::BytePart::Interp(inner) = part {
                    scan_package_order_expr(
                        inner, true, true, in_task, index, scopes, root, imports, claimed, order,
                        counter, visited,
                    );
                }
            }
        }
        Expr::Call(callee, arguments, _) => {
            scan_package_order_expr(
                callee, allow_read, allow_decl, in_task, index, scopes, root, imports, claimed,
                order, counter, visited,
            );
            for argument in arguments {
                scan_package_order_expr(
                    argument, allow_read, allow_decl, in_task, index, scopes, root, imports,
                    claimed, order, counter, visited,
                );
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn record_package_declaration(
    expr: &Expr,
    allow_decl: bool,
    index: usize,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &ImportKinds,
    claimed: &std::collections::BTreeSet<Vec<String>>,
    order: &mut PackageReadOrder,
) {
    if !allow_decl {
        return;
    }
    if is_task_expr(
        expr,
        scopes,
        root,
        &mut std::collections::BTreeSet::new(),
        imports,
    ) && let Some((path, true)) =
        resolve_after_path(expr, scopes, root, &mut std::collections::BTreeSet::new())
        && claimed.contains(&path)
    {
        let entry = order.entry(path).or_insert((None, None, None));
        entry.0 = Some(entry.0.map_or(index, |existing| existing.min(index)));
    }
}

/// Collect every field path in a package, including nested structs.
fn collect_field_paths(items: &[StructItem], prefix: &[String], out: &mut Vec<Vec<String>>) {
    for item in items {
        let StructItem::Field(field) = item else {
            continue;
        };
        let Label::Name(name, _) = &field.label else {
            continue;
        };
        let mut path = prefix.to_vec();
        path.push(name.clone());
        out.push(path.clone());
        if let Some(nested) = struct_items_of(&field.value) {
            collect_field_paths(nested, &path, out);
        }
    }
}

/// Resolve an expression to the package-local field path it aliases.
///
/// `let` bindings are transparent: `p2: l` with `let l = base` resolves to
/// `base`. Unifications and disjunctions keep the first operand that reaches a
/// local field, mirroring CUE's preference for task operands (`base & {...}`).
fn alias_local_target_scoped(
    expr: &Expr,
    scopes: &[(&[StructItem], Vec<String>)],
    root: &[StructItem],
) -> Option<Vec<String>> {
    match expr {
        Expr::Ident(name, _) => {
            let (scope_items, prefix) = scopes.last()?;
            match local_binding_location(&[*scope_items], root, name) {
                Some((_, StructItem::Let { value, .. })) => {
                    alias_local_target_scoped(value, scopes, root)
                }
                Some((false, StructItem::Field(_))) => {
                    let mut path = prefix.clone();
                    path.push(name.clone());
                    Some(path)
                }
                Some((true, StructItem::Field(_))) => Some(vec![name.clone()]),
                _ => None,
            }
        }
        Expr::Select(base, label, _, _) => {
            let mut path = alias_local_target_scoped(base, scopes, root)?;
            path.push(label.clone());
            Some(path)
        }
        Expr::Index(base, index, _) => {
            let mut path = alias_local_target_scoped(base, scopes, root)?;
            if let Some(segment) = index_field_name(index) {
                path.push(segment);
            }
            Some(path)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            alias_local_target_scoped(inner, scopes, root)
        }
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            alias_local_target_scoped(left, scopes, root)
                .or_else(|| alias_local_target_scoped(right, scopes, root))
        }
        _ => None,
    }
}

/// Resolve the alias target of the field at `path`, honouring nested struct
/// scopes (`group.p2: p1`).
fn alias_target_at_path(items: &[StructItem], path: &[String]) -> Option<Vec<String>> {
    let value = find_field_expr_at_path_in_root(items, path, items)?;
    let scopes = scopes_for_path(items, path);
    alias_local_target_scoped(value, &scopes, items)
}

/// Walk a package alias chain from `target` down to the field that declares
/// the task (or the last field aliasing a module task).
///
/// Each entry is a package field path: `["p4", "p3", "p2", "base"]`.
fn alias_chain_nodes(items: &[StructItem], target: &[String]) -> Vec<Vec<String>> {
    let mut nodes = vec![target.to_vec()];
    let mut current = target.to_vec();
    let mut seen = std::collections::BTreeSet::new();
    while seen.insert(current.clone()) {
        let Some(value) = find_field_expr_at_path_in_root(items, &current, items) else {
            break;
        };
        let scopes = scopes_for_path(items, &current);
        let Some(next) = alias_local_target_scoped(value, &scopes, items) else {
            break;
        };
        if next == current || find_field_expr_at_path_in_root(items, &next, items).is_none() {
            break;
        }
        nodes.push(next.clone());
        current = next;
        if nodes.len() > 256 {
            break;
        }
    }
    nodes
}

/// Task instance paths CUE creates for a command alias target.
///
/// CUE materialises the alias field immediately below the target and then every
/// other hop of the chain (`p5: p4; p4: p3; p3: p2; p2: base` yields `p4` and
/// `p2`). The command alias task itself is scheduled separately.
fn alias_chain_instances(items: &[StructItem], target: &[String]) -> Vec<Vec<String>> {
    // `nodes[0]` is the command alias target, which the command task itself
    // executes. CUE materialises the odd hops below it.
    alias_chain_nodes(items, target)
        .iter()
        .skip(1)
        .step_by(2)
        .cloned()
        .collect()
}

/// A value that only references another binding (`p3: p2`, `p3: l`, `#a: #b`).
/// Composed declarations (`base & {...}`, `lib.run & x.y`) are their own task
/// instances.
fn is_pure_alias_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Ident(..)
            | Expr::Select(..)
            | Expr::Index(..)
            | Expr::Paren(..)
            | Expr::Default(..)
            | Expr::Bound(..)
    )
}

/// The concrete task field reached by a task-input output read.
///
/// Pure alias hops (`p3: p2`) and `let` bindings are followed, but a composed
/// declaration (`p2: base & {...}`) is its own task instance, matching CUE.
fn task_read_root(items: &[StructItem], path: &[String]) -> Vec<String> {
    let mut current = path.to_vec();
    let mut seen = std::collections::BTreeSet::new();
    while seen.insert(current.clone()) {
        let Some(value) = find_field_expr_at_path_in_root(items, &current, items) else {
            break;
        };
        if !is_pure_alias_expr(value) {
            break;
        }
        let scopes = scopes_for_path(items, &current);
        let Some(next) = alias_local_target_scoped(value, &scopes, items) else {
            break;
        };
        if next == current || find_field_expr_at_path_in_root(items, &next, items).is_none() {
            break;
        }
        current = next;
        if seen.len() > 256 {
            break;
        }
    }
    current
}

fn alias_instance_root(items: &[StructItem], local: &[String]) -> Option<Vec<String>> {
    let root = alias_target_at_path(items, local)?;
    find_field_expr_at_path_in_root(items, &root, items)?;
    (!definition_root(items, &root)).then_some(root)
}

fn definition_root(items: &[StructItem], path: &[String]) -> bool {
    path.first().is_some_and(|first| {
        matches!(
            local_binding_location(&[items], items, first),
            Some((true, StructItem::Field(field))) if field.definition
        )
    })
}

/// Whether a task-input read passes through a definition that aliases a plain
/// module-backed field. CUE leaves such reads non-concrete (`#a: base` with
/// `base: lib.run`), while a definition alias to another definition
/// (`#a: #b`) still materialises.
fn read_through_definition_module(
    items: &[StructItem],
    imports: &BTreeMap<String, Value>,
    path: &[String],
) -> bool {
    for node in alias_chain_nodes(items, path) {
        if !node.first().is_some_and(|first| first.starts_with('#')) {
            continue;
        }
        let Some(target) = alias_target_at_path(items, &node) else {
            return false;
        };
        if target.first().is_some_and(|first| first.starts_with('#'))
            || definition_root(items, &target)
        {
            return false;
        }
        return find_field_expr_at_path_in_root(items, &target, items).is_some_and(|value| {
            module_task_path_scoped(
                value,
                &[items],
                items,
                imports,
                &mut std::collections::BTreeSet::new(),
            )
            .is_some()
        });
    }
    false
}

fn expr_roots_local_binding(items: &[StructItem], expr: &Expr) -> bool {
    match expr {
        Expr::Ident(name, _) => matches!(
            local_binding_location(&[items], items, name),
            Some((true, StructItem::Field(_)))
        ),
        Expr::Select(base, _, _, _) | Expr::Index(base, _, _) => {
            expr_roots_local_binding(items, base)
        }
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            expr_roots_local_binding(items, left) || expr_roots_local_binding(items, right)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            expr_roots_local_binding(items, inner)
        }
        _ => false,
    }
}

fn is_package_root_path(path: &[String], root: &[StructItem]) -> bool {
    let Some(first) = path.first() else {
        return false;
    };
    matches!(
        local_binding_location(&[root], root, first),
        Some((true, _))
    )
}

fn external_task_key(path: &[String], root: &[StructItem]) -> String {
    if is_package_root_path(path, root) {
        package_task_key(path)
    } else {
        path.last().cloned().unwrap_or_default()
    }
}

/// Resolve a command alias target, preferring unified operands that are
/// themselves tasks.
///
/// `r: aux & prep` must resolve to `prep` (the task), not to the first
/// resolvable non-task operand, so declaration/read ordering treats it like
/// `r: prep`.
#[derive(Clone, Default)]
struct CommandAliasTarget {
    module: Option<Vec<String>>,
    local: Option<Vec<String>>,
}

fn resolve_command_alias_target(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &ImportKinds,
    module_values: &BTreeMap<String, Value>,
) -> Option<CommandAliasTarget> {
    match expr {
        Expr::Unify(left, right, _) => {
            let left = resolve_command_alias_target(left, scopes, root, imports, module_values);
            let right = resolve_command_alias_target(right, scopes, root, imports, module_values);
            match (left, right) {
                (Some(mut left), Some(right)) => {
                    if left.module.is_none() {
                        left.module = right.module;
                    }
                    if left.local.is_none() {
                        left.local = right.local;
                    }
                    Some(left)
                }
                (left, right) => left.or(right),
            }
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            resolve_command_alias_target(inner, scopes, root, imports, module_values)
        }
        _ => {
            let local = local_alias_path(expr, scopes, root, imports);
            let module = module_task_path_scoped(
                expr,
                scopes,
                root,
                module_values,
                &mut std::collections::BTreeSet::new(),
            );
            if local.is_none() && module.is_none() {
                return None;
            }
            Some(CommandAliasTarget { module, local })
        }
    }
}

/// Read through a command-local (non-root) task instance.
fn command_local_task_read(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &ImportKinds,
) -> bool {
    let Some(first) = selector_root_ident(expr) else {
        return false;
    };
    matches!(
        local_binding_location(scopes, root, first),
        Some((false, _))
    ) && is_task_expr(
        expr,
        scopes,
        root,
        &mut std::collections::BTreeSet::new(),
        imports,
    )
}

/// A command-local non-definition field shadows a package-root field.
fn command_local_shadows_root(scopes: &[&[StructItem]], root: &[StructItem], name: &str) -> bool {
    matches!(local_binding_location(&[root], root, name), Some((true, _)))
        && matches!(
            local_binding_location(scopes, root, name),
            Some((false, StructItem::Field(field))) if !field.definition
        )
}

/// Local field path of a read base whose value is module-backed.
fn local_read_path(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    module_values: &BTreeMap<String, Value>,
) -> Option<Vec<String>> {
    let path = selector_names(expr, None)?;
    let first = path.first()?;
    if module_values.contains_key(first) {
        return None;
    }
    // A command-local field shadows a package-root field of the same name.
    // Definitions stay resolvable because their output reads are injectable.
    if command_local_shadows_root(scopes, root, first) {
        return None;
    }
    find_field_expr_at_path(root, &path).map(|_| path)
}

/// Package-root field path of a command alias target.
fn local_alias_path(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &ImportKinds,
) -> Option<Vec<String>> {
    let path = selector_names(expr, None)?;
    let first = path.first()?;
    if !matches!(local_binding_location(scopes, root, first), Some((true, _))) {
        return None;
    }
    let value = find_field_expr_at_path(root, &path)?;
    let scoped = scopes_for_path(root, &path);
    let refs: Vec<&[StructItem]> = scoped.iter().map(|(items, _)| *items).collect();
    is_task_expr(
        value,
        &refs,
        root,
        &mut std::collections::BTreeSet::new(),
        imports,
    )
    .then_some(path)
}

/// Like [`module_task_path`], following local `let`/field bindings first.
fn root_field_composed(items: &[StructItem], name: &str) -> bool {
    let declarations: Vec<&Expr> = items
        .iter()
        .filter_map(|item| match item {
            StructItem::Field(field)
                if matches!(&field.label, Label::Name(label, _) if label == name) =>
            {
                Some(&field.value)
            }
            _ => None,
        })
        .collect();
    declarations
        .iter()
        .any(|value| expr_composed_with_struct(value))
        || (declarations.len() > 1 && declarations.iter().any(|value| expr_has_struct(value)))
}

/// A unified value that includes a struct literal, e.g.
/// `lib.run & {stdout: string}` or `{stdout: string} & lib.run`.
fn expr_composed_with_struct(expr: &Expr) -> bool {
    match expr {
        Expr::Unify(left, right, _) => expr_has_struct(left) || expr_has_struct(right),
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            expr_composed_with_struct(inner)
        }
        _ => false,
    }
}

fn expr_has_struct(expr: &Expr) -> bool {
    match expr {
        Expr::Struct(..) => true,
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            expr_has_struct(left) || expr_has_struct(right)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            expr_has_struct(inner)
        }
        _ => false,
    }
}

fn module_task_path_scoped(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    module_values: &BTreeMap<String, Value>,
    visited: &mut std::collections::BTreeSet<String>,
) -> Option<Vec<String>> {
    match expr {
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return None;
            }
            let resolved = match local_binding_location(scopes, root, name) {
                Some((_, StructItem::Let { value, .. })) => {
                    module_task_path_scoped(value, scopes, root, module_values, visited)
                }
                Some((_, StructItem::Field(field))) if !field.definition => {
                    module_task_path_scoped(&field.value, scopes, root, module_values, visited)
                }
                _ => module_task_path(expr, module_values),
            };
            visited.remove(name);
            resolved
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            module_task_path_scoped(inner, scopes, root, module_values, visited)
        }
        Expr::Select(..) | Expr::Index(..) => {
            if let Some((path, true)) =
                resolve_after_path(expr, scopes, root, &mut std::collections::BTreeSet::new())
                && let Some(value) = find_field_expr_at_path(root, &path)
                && let Some(module) =
                    module_task_path_scoped(value, scopes, root, module_values, visited)
            {
                return Some(module);
            }
            module_task_path(expr, module_values)
        }
        _ => module_task_path(expr, module_values),
    }
}

/// Resolve a selector rooted at an import to a module task path.
fn module_task_path(expr: &Expr, module_values: &BTreeMap<String, Value>) -> Option<Vec<String>> {
    let path = selector_names(expr, None)?;
    let (root_name, rest) = path.split_first()?;
    let mut value = module_values.get(root_name)?;
    for segment in rest {
        value = value_field(value, segment)?;
    }
    value_fields(value)?.contains_key("$id").then_some(path)
}

fn module_task_value<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = value_field(current, segment)?;
    }
    Some(current)
}

/// Package paths that command task fields alias (`r: prep`, `r: group.prep`),
/// keyed by the command task name.
fn collect_command_task_package_paths(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &ImportKinds,
    module_values: &BTreeMap<String, Value>,
    out: &mut std::collections::BTreeMap<String, CommandAliasTarget>,
) {
    match expr {
        Expr::Struct(items, _) => {
            for item in items {
                let StructItem::Field(field) = item else {
                    continue;
                };
                let Label::Name(label, _) = &field.label else {
                    continue;
                };
                if label.starts_with('$')
                    || field.definition
                    || field.optional
                    || (!field.quoted && label.starts_with('_'))
                {
                    continue;
                }
                let child_is_task = is_task_expr(
                    &field.value,
                    scopes,
                    root,
                    &mut std::collections::BTreeSet::new(),
                    imports,
                );
                if child_is_task
                    && let Some(target) = resolve_command_alias_target(
                        &field.value,
                        scopes,
                        root,
                        imports,
                        module_values,
                    )
                {
                    out.entry(label.clone()).or_insert(target);
                }
                collect_command_task_package_paths(
                    &field.value,
                    scopes,
                    root,
                    imports,
                    module_values,
                    out,
                );
            }
        }
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            collect_command_task_package_paths(left, scopes, root, imports, module_values, out);
            collect_command_task_package_paths(right, scopes, root, imports, module_values, out);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            collect_command_task_package_paths(inner, scopes, root, imports, module_values, out);
        }
        _ => {}
    }
}

fn add_alias_dependencies(
    scheduled: &mut [(String, Vec<String>, BTreeMap<String, Value>)],
    alias_dependencies: &std::collections::BTreeMap<String, Vec<String>>,
) {
    for (name, dependencies) in alias_dependencies {
        let Some((_, existing, _)) = scheduled
            .iter_mut()
            .find(|(scheduled_name, _, _)| scheduled_name == name)
        else {
            continue;
        };
        for dependency in dependencies {
            if !existing.contains(dependency) {
                existing.push(dependency.clone());
            }
        }
    }
}

fn collect_schedule<'a>(
    map: &BTreeMap<String, Value>,
    ast: Option<&'a [StructItem]>,
    root: &'a [StructItem],
    scopes: &mut Vec<&'a [StructItem]>,
    out: &mut Vec<(String, Vec<String>, BTreeMap<String, Value>)>,
) -> Result<(), ostinato_lang::LangError> {
    if let Some(items) = ast {
        scopes.push(items);
    }
    for (key, value) in map {
        if key.starts_with('$') || key.starts_with('_') || key.starts_with('#') {
            continue;
        }
        let Some(fields) = value_fields(value) else {
            continue;
        };
        let item = ast.and_then(|items| field_item(items, key));
        if fields.contains_key("$id") {
            let dependencies = match ast {
                Some(items) => after_names_union(items, key, root, scopes)?,
                None => Vec::new(),
            };
            out.push((key.clone(), dependencies, fields.clone()));
            continue;
        }
        let sub_ast = item.and_then(|item| match item {
            StructItem::Field(field) => match &field.value {
                Expr::Struct(items, _) => Some(items.as_slice()),
                _ => None,
            },
            _ => None,
        });
        collect_schedule(fields, sub_ast, root, scopes, out)?;
    }
    if ast.is_some() {
        scopes.pop();
    }
    Ok(())
}

fn after_names_union(
    items: &[StructItem],
    name: &str,
    root: &[StructItem],
    scopes: &[&[StructItem]],
) -> Result<Vec<String>, ostinato_lang::LangError> {
    let mut dependencies = Vec::new();
    for item in items {
        let StructItem::Field(field) = item else {
            continue;
        };
        if !matches!(&field.label, Label::Name(label, _) if label == name) {
            continue;
        }
        for dependency in after_names(item, root, scopes)? {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
        }
    }
    Ok(dependencies)
}

fn field_item<'a>(items: &'a [StructItem], name: &str) -> Option<&'a StructItem> {
    items.iter().find(|item| match item {
        StructItem::Field(field) => matches!(&field.label, Label::Name(label, _) if label == name),
        _ => false,
    })
}

enum ToolTaskOutcome {
    NotReady(&'static str),
    Done { outputs: Vec<(String, Value)> },
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CaptureKind {
    None,
    String,
    Other,
}

fn capture_kind(
    task: &BTreeMap<String, Value>,
    key: &str,
) -> Result<CaptureKind, ostinato_lang::LangError> {
    match task.get(key) {
        None | Some(Value::Null) => Ok(CaptureKind::None),
        Some(Value::String(_)) => Ok(CaptureKind::String),
        Some(Value::Bytes(_)) => Ok(CaptureKind::Other),
        Some(Value::Type(ostinato_lang::TypeKind::String)) => Ok(CaptureKind::String),
        Some(Value::Type(_)) | Some(Value::Bottom) => Ok(CaptureKind::Other),
        Some(Value::Disjunction(alternatives, defaults)) => {
            let default = (defaults.len() == 1)
                .then(|| defaults[0])
                .and_then(|index| alternatives.get(index))
                .cloned();
            match default {
                Some(Value::Null) => Ok(CaptureKind::None),
                Some(Value::String(_)) | Some(Value::Type(ostinato_lang::TypeKind::String)) => {
                    Ok(CaptureKind::String)
                }
                _ => Ok(CaptureKind::Other),
            }
        }
        Some(other) => Err(ostinato_lang::LangError::Workflow(format!(
            "tool/exec.Run `{key}` must be a string, bytes or null, found {}",
            other.type_name()
        ))),
    }
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    true
}

fn resolve_executable(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if name.contains('/') {
        return is_executable_file(path).then(|| path.to_path_buf());
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(name))
            .find(|candidate| is_executable_file(candidate))
    })
}

fn spawn_detail(err: &std::io::Error, executable: &str) -> String {
    if executable.contains('/') && err.kind() == std::io::ErrorKind::NotFound {
        format!("fork/exec {executable}: no such file or directory")
    } else if executable.contains('/') && err.kind() == std::io::ErrorKind::PermissionDenied {
        format!("fork/exec {executable}: permission denied")
    } else {
        err.to_string()
    }
}

fn io_detail(err: &std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::NotFound => "no such file or directory".to_owned(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_owned(),
        _ => err.to_string(),
    }
}

fn exec_failure(status: &std::process::ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("exit status {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            let name = match signal {
                1 => "hangup",
                2 => "interrupt",
                3 => "quit",
                6 => "aborted",
                9 => "killed",
                11 => "segmentation fault",
                13 => "broken pipe",
                15 => "terminated",
                _ => return format!("signal {signal}"),
            };
            return format!("signal: {name}");
        }
    }
    "terminated by signal".to_owned()
}

fn output_expr(value: &Value) -> Option<Expr> {
    match value {
        Value::String(text) => Some(Expr::Str(
            vec![ostinato_lang::ast::StrPart::Text(text.clone())],
            Span::default(),
        )),
        Value::Bool(value) => Some(Expr::Bool(*value, Span::default())),
        Value::Int(value) => Some(Expr::Int(*value, Span::default())),
        Value::BigInt(value) => Some(Expr::BigInt(value.clone(), Span::default())),
        Value::Float(value) => Some(Expr::Float(value.clone(), Span::default())),
        Value::Bytes(value) => Some(Expr::Bytes(value.clone(), Span::default())),
        Value::Null => Some(Expr::Null(Span::default())),
        Value::List(items) => {
            let mut exprs = Vec::with_capacity(items.len());
            for item in items {
                exprs.push(output_expr(item)?);
            }
            Some(Expr::List(exprs, Span::default()))
        }
        Value::Type(ostinato_lang::TypeKind::String) => {
            Some(Expr::Ident("string".to_owned(), Span::default()))
        }
        Value::Type(ostinato_lang::TypeKind::Bool) => {
            Some(Expr::Ident("bool".to_owned(), Span::default()))
        }
        Value::Type(ostinato_lang::TypeKind::Int) => {
            Some(Expr::Ident("int".to_owned(), Span::default()))
        }
        Value::Type(ostinato_lang::TypeKind::Float) => {
            Some(Expr::Ident("float".to_owned(), Span::default()))
        }
        Value::Type(ostinato_lang::TypeKind::Number) => {
            Some(Expr::Ident("number".to_owned(), Span::default()))
        }
        Value::Type(ostinato_lang::TypeKind::Bytes) => {
            Some(Expr::Ident("bytes".to_owned(), Span::default()))
        }
        Value::Disjunction(alternatives, defaults) => {
            let mut exprs = Vec::with_capacity(alternatives.len());
            for (index, alternative) in alternatives.iter().enumerate() {
                let expr = output_expr(alternative)?;
                exprs.push(if defaults.contains(&index) {
                    Expr::Default(Box::new(expr), Span::default())
                } else {
                    expr
                });
            }
            let mut iter = exprs.into_iter();
            let mut expr = iter.next()?;
            for next in iter {
                expr = Expr::Disjunction(Box::new(expr), Box::new(next), Span::default());
            }
            Some(expr)
        }
        Value::Struct(fields) => value_struct_expr(fields),
        Value::Definition { fields, .. } => value_struct_expr(fields),
        _ => None,
    }
}

fn value_struct_expr(fields: &BTreeMap<String, Value>) -> Option<Expr> {
    let span = Span::default();
    let mut items = Vec::with_capacity(fields.len());
    for (name, value) in fields {
        items.push(StructItem::Field(Field {
            id: 0,
            label: Label::Name(name.clone(), span),
            value: output_expr(value)?,
            optional: false,
            definition: false,
            alias: None,
            quoted: false,
            bytes_label_runes: None,
            bytes_label_multiline: false,
            attributes: Vec::new(),
            span,
        }));
    }
    Some(Expr::Struct(items, span))
}

fn inject_into_struct(expr: &mut Expr, field: &str, value: &Expr) -> bool {
    match expr {
        Expr::Struct(items, _) => {
            for item in items {
                let StructItem::Field(existing) = item else {
                    continue;
                };
                if matches!(&existing.label, Label::Name(name, _) if name == field) {
                    existing.value = Expr::Unify(
                        Box::new(existing.value.clone()),
                        Box::new(value.clone()),
                        Span::default(),
                    );
                    return true;
                }
            }
            false
        }
        Expr::Unify(left, right, _) => {
            inject_into_struct(left, field, value) || inject_into_struct(right, field, value)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) => inject_into_struct(inner, field, value),
        _ => false,
    }
}

fn unwrap_ref_base(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            unwrap_ref_base(inner)
        }
        other => other,
    }
}

fn find_definition<'a>(items: &'a [StructItem], name: &str) -> Option<&'a Expr> {
    items.iter().find_map(|item| match item {
        StructItem::Field(field) => match &field.label {
            Label::Name(label, _) if label == name => Some(&field.value),
            _ => None,
        },
        StructItem::Let {
            name: let_name,
            value,
            ..
        } if let_name == name => Some(value),
        _ => None,
    })
}

fn local_definition<'a>(
    scope: &'a [StructItem],
    root: &'a [StructItem],
    name: &str,
) -> Option<&'a Expr> {
    find_definition(scope, name).or_else(|| find_definition(root, name))
}

fn is_let_binding(scopes: &[&[StructItem]], root: &[StructItem], name: &str) -> bool {
    let is_let = |items: &[StructItem]| {
        items
            .iter()
            .any(|item| matches!(item, StructItem::Let { name: let_name, .. } if let_name == name))
    };
    scopes.iter().rev().any(|scope| is_let(scope)) || is_let(root)
}

fn local_definition_scopes<'a>(
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    name: &str,
) -> Option<&'a Expr> {
    scopes
        .iter()
        .rev()
        .find_map(|scope| find_definition(scope, name))
        .or_else(|| find_definition(root, name))
}

/// Import aliases that are concrete values, plus any task paths they export.
#[derive(Default)]
struct ImportKinds {
    non_task: std::collections::BTreeSet<String>,
    task_paths: std::collections::BTreeMap<String, std::collections::BTreeSet<Vec<String>>>,
}

/// Collect selector paths (from the imported root) that resolve to task values.
fn collect_task_selector_paths(
    value: &Value,
    prefix: &mut Vec<String>,
    out: &mut std::collections::BTreeSet<Vec<String>>,
) {
    let (fields, definitions) = match value {
        Value::Struct(fields) => (fields, None),
        Value::Definition {
            fields,
            definitions,
            ..
        } => (fields, Some(definitions)),
        _ => return,
    };
    if fields.contains_key("$id") {
        out.insert(prefix.clone());
        return;
    }
    for (name, field) in fields {
        if name.starts_with('$') || name.starts_with('_') {
            continue;
        }
        prefix.push(name.clone());
        collect_task_selector_paths(field, prefix, out);
        prefix.pop();
    }
    if let Some(definitions) = definitions {
        for (name, field) in definitions {
            if name.starts_with('$') {
                continue;
            }
            prefix.push(name.clone());
            collect_task_selector_paths(field, prefix, out);
            prefix.pop();
        }
    }
}

/// Turn a selector/index expression plus an optional final field into names.
fn selector_names(expr: &Expr, field: Option<&str>) -> Option<Vec<String>> {
    fn walk(expr: &Expr) -> Option<Vec<String>> {
        match expr {
            Expr::Ident(name, _) => Some(vec![name.clone()]),
            Expr::Select(base, name, _, _) => {
                let mut path = walk(base)?;
                path.push(name.clone());
                Some(path)
            }
            Expr::Index(base, index, _) => {
                let name = index_field_name(index)?;
                let mut path = walk(base)?;
                path.push(name);
                Some(path)
            }
            Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
                walk(inner)
            }
            _ => None,
        }
    }
    let mut path = walk(expr)?;
    if let Some(field) = field {
        path.push(field.to_owned());
    }
    Some(path)
}

fn is_import_task_expr(expr: &Expr, imports: &ImportKinds) -> bool {
    match expr {
        Expr::Select(..) | Expr::Index(..) => selector_names(expr, None)
            .and_then(|path| {
                let (root, rest) = path.split_first()?;
                Some(
                    imports
                        .task_paths
                        .get(root)
                        .is_some_and(|tasks| tasks.contains(rest)),
                )
            })
            .unwrap_or(false),
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            is_import_task_expr(inner, imports)
        }
        _ => false,
    }
}

fn is_task_binding<'a>(
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    name: &str,
    visited: &mut std::collections::BTreeSet<String>,
    non_task_imports: &ImportKinds,
) -> bool {
    if !visited.insert(name.to_owned()) {
        return false;
    }
    local_definition_scopes(scopes, root, name)
        .is_some_and(|value| is_task_expr(value, scopes, root, visited, non_task_imports))
}

fn select_field_value<'a>(
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    expr: &'a Expr,
    field: &str,
    visited: &mut std::collections::BTreeSet<String>,
) -> Option<&'a Expr> {
    match expr {
        Expr::Struct(items, _) => find_definition(items, field),
        Expr::Unify(left, right, _) => select_field_value(scopes, root, left, field, visited)
            .or_else(|| select_field_value(scopes, root, right, field, visited)),
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            select_field_value(scopes, root, inner, field, visited)
        }
        Expr::Ident(name, _) => {
            if !visited.insert(name.to_owned()) {
                return None;
            }
            local_definition_scopes(scopes, root, name)
                .and_then(|value| select_field_value(scopes, root, value, field, visited))
        }
        _ => None,
    }
}

fn resolve_local_value<'a>(
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    expr: &'a Expr,
    visited: &mut std::collections::BTreeSet<String>,
) -> Option<&'a Expr> {
    match expr {
        Expr::Ident(name, _) => {
            if !visited.insert(name.to_owned()) {
                return None;
            }
            local_definition_scopes(scopes, root, name)
        }
        Expr::Select(base, field, _, _) => {
            let base_value = resolve_local_value(scopes, root, base, visited)?;
            select_field_value(scopes, root, base_value, field, visited)
        }
        Expr::Index(base, index, _) => {
            let target = index_field_name(index)?;
            let base_value = resolve_local_value(scopes, root, base, visited)?;
            if let Expr::List(items, _) = base_value
                && let Ok(position) = target.parse::<usize>()
            {
                return items.get(position);
            }
            select_field_value(scopes, root, base_value, &target, visited)
        }
        Expr::Struct(..) | Expr::List(..) => Some(expr),
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            resolve_local_value(scopes, root, inner, visited)
        }
        Expr::Unify(left, right, _) => resolve_local_value(scopes, root, left, visited)
            .or_else(|| resolve_local_value(scopes, root, right, visited)),
        _ => None,
    }
}

fn resolve_string_value<'a>(
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    expr: &'a Expr,
    visited: &mut std::collections::BTreeSet<String>,
) -> Option<String> {
    match expr {
        Expr::Str(parts, _) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    ostinato_lang::ast::StrPart::Text(value) => text.push_str(value),
                    ostinato_lang::ast::StrPart::Interp(_) => return None,
                }
            }
            Some(text)
        }
        Expr::Ident(name, _) => {
            if !visited.insert(name.to_owned()) {
                return None;
            }
            local_definition_scopes(scopes, root, name)
                .and_then(|value| resolve_string_value(scopes, root, value, visited))
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            resolve_string_value(scopes, root, inner, visited)
        }
        _ => None,
    }
}

fn expr_contains_task<'a>(
    expr: &'a Expr,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    visited: &mut std::collections::BTreeSet<String>,
    non_task_imports: &ImportKinds,
) -> bool {
    match expr {
        Expr::Struct(items, _) => items
            .iter()
            .any(|item| item_contains_task(item, scopes, root, visited, non_task_imports)),
        Expr::Unify(left, right, _) => {
            expr_contains_task(left, scopes, root, visited, non_task_imports)
                || expr_contains_task(right, scopes, root, visited, non_task_imports)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            expr_contains_task(inner, scopes, root, visited, non_task_imports)
        }
        Expr::Ident(name, _) => {
            if !visited.insert(name.to_owned()) {
                return false;
            }
            local_definition_scopes(scopes, root, name).is_some_and(|value| {
                expr_contains_task(value, scopes, root, visited, non_task_imports)
            })
        }
        _ => false,
    }
}

fn item_contains_task<'a>(
    item: &'a StructItem,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    visited: &mut std::collections::BTreeSet<String>,
    non_task_imports: &ImportKinds,
) -> bool {
    match item {
        StructItem::Field(field) => {
            is_task_expr(&field.value, scopes, root, visited, non_task_imports)
                || expr_contains_task(&field.value, scopes, root, visited, non_task_imports)
        }
        StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
            expr_contains_task(value, scopes, root, visited, non_task_imports)
        }
        StructItem::For { body, .. } | StructItem::If { body, .. } => body
            .iter()
            .any(|item| item_contains_task(item, scopes, root, visited, non_task_imports)),
        StructItem::Ellipsis(_) => false,
    }
}

fn is_task_expr<'a>(
    expr: &'a Expr,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    visited: &mut std::collections::BTreeSet<String>,
    non_task_imports: &ImportKinds,
) -> bool {
    match expr {
        Expr::Select(base, field, _, _) => {
            is_task_selection(base, Some(field), scopes, root, visited, non_task_imports)
        }
        Expr::Index(base, index, _) => {
            let field = index_field_name(index).or_else(|| {
                resolve_string_value(scopes, root, index, &mut std::collections::BTreeSet::new())
            });
            is_task_selection(
                base,
                field.as_deref(),
                scopes,
                root,
                visited,
                non_task_imports,
            )
        }
        Expr::Unify(left, right, _) => {
            is_task_expr(left, scopes, root, visited, non_task_imports)
                || is_task_expr(right, scopes, root, visited, non_task_imports)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            is_task_expr(inner, scopes, root, visited, non_task_imports)
        }
        Expr::Ident(name, _) => is_task_binding(scopes, root, name, visited, non_task_imports),
        _ => false,
    }
}

fn selector_root_ident(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(name, _) => Some(name),
        Expr::Select(base, _, _, _) | Expr::Index(base, _, _) => selector_root_ident(base),
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            selector_root_ident(inner)
        }
        _ => None,
    }
}

fn is_task_selection<'a>(
    base: &'a Expr,
    field: Option<&str>,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    visited: &mut std::collections::BTreeSet<String>,
    non_task_imports: &ImportKinds,
) -> bool {
    // Nested package paths (`group.p4`) resolve inside the container scope,
    // where sibling aliases such as `p3: p2` are visible.
    if let Some(path) = selector_names(base, field)
        && path.len() > 1
        && path.first().is_some_and(|first| {
            matches!(local_binding_location(scopes, root, first), Some((true, _)))
        })
        && let Some(value) = find_field_expr_at_path_in_root(root, &path, root)
    {
        let scoped = scopes_for_path(root, &path);
        let refs: Vec<&[StructItem]> = scoped.iter().map(|(items, _)| *items).collect();
        return is_task_expr(value, &refs, root, visited, non_task_imports);
    }
    let Some(base_value) = resolve_local_value(scopes, root, base, visited) else {
        // Unresolvable selector chain: tool imports are task-bearing, while
        // builtin imports and import-rooted concrete values are not. Local
        // modules may export tasks, so consult the evaluated import paths.
        let Some(path) = selector_names(base, field) else {
            return selector_root_ident(base)
                .is_none_or(|name| !non_task_imports.non_task.contains(name));
        };
        let Some((root_name, rest)) = path.split_first() else {
            return false;
        };
        if let Some(tasks) = non_task_imports.task_paths.get(root_name) {
            return tasks.contains(rest);
        }
        return !non_task_imports.non_task.contains(root_name);
    };
    match field {
        Some(field) => match select_field_value(scopes, root, base_value, field, visited) {
            Some(selected) => is_task_expr(selected, scopes, root, visited, non_task_imports),
            None => is_task_expr(base_value, scopes, root, visited, non_task_imports),
        },
        None => {
            is_task_expr(base_value, scopes, root, visited, non_task_imports)
                || expr_contains_task(base_value, scopes, root, visited, non_task_imports)
        }
    }
}

/// Leaf field name of a selector/index chain, e.g. `a.producer` -> `producer`.
fn selector_leaf(base: &Expr) -> Option<String> {
    match base {
        Expr::Select(_, field, _, _) => Some(field.clone()),
        Expr::Index(_, index, _) => index_field_name(index),
        _ => None,
    }
}

/// Whether a selector chain resolves to a tool task declaration.
fn selector_resolves_to_task<'a>(
    base: &'a Expr,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    non_task_imports: &ImportKinds,
) -> bool {
    resolve_local_value(scopes, root, base, &mut std::collections::BTreeSet::new()).is_some_and(
        |value| {
            is_task_expr(
                value,
                scopes,
                root,
                &mut std::collections::BTreeSet::new(),
                non_task_imports,
            )
        },
    )
}

/// Resolve an identifier chain (lets and fields) to a scheduled task name.
fn ident_task_name<'a>(
    expr: &'a Expr,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    names: &std::collections::BTreeSet<String>,
    imports: &ImportKinds,
    visited: &mut std::collections::BTreeSet<String>,
) -> Option<String> {
    match expr {
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return None;
            }
            match local_binding(scopes, root, name) {
                Some(StructItem::Let { value, .. }) => {
                    ident_task_name(value, scopes, root, names, imports, visited)
                }
                Some(StructItem::Field(field)) => match &field.label {
                    Label::Name(label, _)
                        if names.contains(label)
                            && is_task_expr(
                                &field.value,
                                scopes,
                                root,
                                &mut std::collections::BTreeSet::new(),
                                imports,
                            ) =>
                    {
                        Some(label.clone())
                    }
                    _ => None,
                },
                _ => None,
            }
        }
        Expr::Select(..) | Expr::Index(..) => {
            let leaf = selector_leaf(expr)?;
            names.contains(&leaf).then_some(leaf)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            ident_task_name(inner, scopes, root, names, imports, visited)
        }
        _ => None,
    }
}

fn collect_output_refs<'a>(
    expr: &'a Expr,
    scopes: &mut Vec<&'a [StructItem]>,
    root: &'a [StructItem],
    names: &std::collections::BTreeSet<String>,
    visited: &mut std::collections::BTreeSet<String>,
    out: &mut std::collections::BTreeSet<String>,
    non_task_imports: &ImportKinds,
) {
    match expr {
        Expr::Select(base, field, _, _) => {
            if matches!(field.as_str(), "stdout" | "stderr" | "success") {
                match unwrap_ref_base(base) {
                    Expr::Ident(name, _) => {
                        if names.contains(name)
                            && is_task_binding(
                                scopes,
                                root,
                                name,
                                &mut std::collections::BTreeSet::new(),
                                non_task_imports,
                            )
                        {
                            out.insert(name.clone());
                        } else if let Some(task) = ident_task_name(
                            unwrap_ref_base(base),
                            scopes,
                            root,
                            names,
                            non_task_imports,
                            &mut std::collections::BTreeSet::new(),
                        ) {
                            out.insert(task);
                        }
                    }
                    nested => {
                        // Nested task declarations (`a: producer: exec.Run`)
                        // are scheduled under their leaf name.
                        if let Some(leaf) = selector_leaf(nested)
                            && names.contains(&leaf)
                            && selector_resolves_to_task(nested, scopes, root, non_task_imports)
                        {
                            out.insert(leaf);
                        }
                    }
                }
            }
            collect_output_refs(base, scopes, root, names, visited, out, non_task_imports);
        }
        Expr::Ident(name, _) => {
            if !names.contains(name)
                && visited.insert(name.clone())
                && let Some(value) = local_definition_scopes(scopes, root, name)
            {
                collect_output_refs(value, scopes, root, names, visited, out, non_task_imports);
            }
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            collect_output_refs(left, scopes, root, names, visited, out, non_task_imports);
            collect_output_refs(right, scopes, root, names, visited, out, non_task_imports);
        }
        Expr::Bound(_, inner, _)
        | Expr::Default(inner, _)
        | Expr::Paren(inner, _)
        | Expr::Unary(_, inner, _) => {
            collect_output_refs(inner, scopes, root, names, visited, out, non_task_imports);
        }
        Expr::Index(base, index, _) => {
            collect_output_refs(base, scopes, root, names, visited, out, non_task_imports);
            collect_output_refs(index, scopes, root, names, visited, out, non_task_imports);
        }
        Expr::Call(callee, arguments, _) => {
            collect_output_refs(callee, scopes, root, names, visited, out, non_task_imports);
            for argument in arguments {
                collect_output_refs(
                    argument,
                    scopes,
                    root,
                    names,
                    visited,
                    out,
                    non_task_imports,
                );
            }
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let ostinato_lang::ast::StrPart::Interp(inner) = part {
                    collect_output_refs(inner, scopes, root, names, visited, out, non_task_imports);
                }
            }
        }
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                collect_output_refs(item, scopes, root, names, visited, out, non_task_imports);
            }
        }
        Expr::OpenList(Some(inner), _) => {
            collect_output_refs(inner, scopes, root, names, visited, out, non_task_imports);
        }
        Expr::Struct(items, _) => {
            scopes.push(items);
            for item in items {
                match item {
                    StructItem::Field(field) => {
                        collect_output_refs(
                            &field.value,
                            scopes,
                            root,
                            names,
                            visited,
                            out,
                            non_task_imports,
                        );
                    }
                    StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                        collect_output_refs(
                            value,
                            scopes,
                            root,
                            names,
                            visited,
                            out,
                            non_task_imports,
                        );
                    }
                    StructItem::For { source, body, .. } => {
                        collect_output_refs(
                            source,
                            scopes,
                            root,
                            names,
                            visited,
                            out,
                            non_task_imports,
                        );
                        collect_output_refs_items(
                            body,
                            scopes,
                            root,
                            names,
                            visited,
                            out,
                            non_task_imports,
                        );
                    }
                    StructItem::If { cond, body, .. } => {
                        collect_output_refs(
                            cond,
                            scopes,
                            root,
                            names,
                            visited,
                            out,
                            non_task_imports,
                        );
                        collect_output_refs_items(
                            body,
                            scopes,
                            root,
                            names,
                            visited,
                            out,
                            non_task_imports,
                        );
                    }
                    StructItem::Ellipsis(_) => {}
                }
            }
            scopes.pop();
        }
        _ => {}
    }
}

fn collect_output_refs_items<'a>(
    items: &'a [StructItem],
    scopes: &mut Vec<&'a [StructItem]>,
    root: &'a [StructItem],
    names: &std::collections::BTreeSet<String>,
    visited: &mut std::collections::BTreeSet<String>,
    out: &mut std::collections::BTreeSet<String>,
    non_task_imports: &ImportKinds,
) {
    scopes.push(items);
    for item in items {
        match item {
            StructItem::Field(field) => {
                collect_output_refs(
                    &field.value,
                    scopes,
                    root,
                    names,
                    visited,
                    out,
                    non_task_imports,
                );
            }
            StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                collect_output_refs(value, scopes, root, names, visited, out, non_task_imports);
            }
            StructItem::For { source, body, .. } => {
                collect_output_refs(source, scopes, root, names, visited, out, non_task_imports);
                collect_output_refs_items(
                    body,
                    scopes,
                    root,
                    names,
                    visited,
                    out,
                    non_task_imports,
                );
            }
            StructItem::If { cond, body, .. } => {
                collect_output_refs(cond, scopes, root, names, visited, out, non_task_imports);
                collect_output_refs_items(
                    body,
                    scopes,
                    root,
                    names,
                    visited,
                    out,
                    non_task_imports,
                );
            }
            StructItem::Ellipsis(_) => {}
        }
    }
    scopes.pop();
}

fn collect_refs_for_task<'a>(
    items: &'a [StructItem],
    task_name: &str,
    root: &'a [StructItem],
    names: &std::collections::BTreeSet<String>,
    scopes: &mut Vec<&'a [StructItem]>,
    out: &mut std::collections::BTreeSet<String>,
    non_task_imports: &ImportKinds,
) {
    scopes.push(items);
    for item in items {
        match item {
            StructItem::Field(field) => {
                if matches!(&field.label, Label::Name(name, _) if name == task_name) {
                    let mut visited = std::collections::BTreeSet::new();
                    collect_output_refs(
                        &field.value,
                        scopes,
                        root,
                        names,
                        &mut visited,
                        out,
                        non_task_imports,
                    );
                }
                collect_refs_for_task_expr(
                    &field.value,
                    task_name,
                    root,
                    names,
                    scopes,
                    out,
                    non_task_imports,
                );
            }
            StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                collect_refs_for_task_expr(
                    value,
                    task_name,
                    root,
                    names,
                    scopes,
                    out,
                    non_task_imports,
                );
            }
            StructItem::For { source, body, .. } => {
                collect_refs_for_task_expr(
                    source,
                    task_name,
                    root,
                    names,
                    scopes,
                    out,
                    non_task_imports,
                );
                collect_refs_for_task(body, task_name, root, names, scopes, out, non_task_imports);
            }
            StructItem::If { cond, body, .. } => {
                collect_refs_for_task_expr(
                    cond,
                    task_name,
                    root,
                    names,
                    scopes,
                    out,
                    non_task_imports,
                );
                collect_refs_for_task(body, task_name, root, names, scopes, out, non_task_imports);
            }
            StructItem::Ellipsis(_) => {}
        }
    }
    scopes.pop();
}

fn collect_refs_for_task_expr<'a>(
    expr: &'a Expr,
    task_name: &str,
    root: &'a [StructItem],
    names: &std::collections::BTreeSet<String>,
    scopes: &mut Vec<&'a [StructItem]>,
    out: &mut std::collections::BTreeSet<String>,
    non_task_imports: &ImportKinds,
) {
    match expr {
        Expr::Struct(items, _) => {
            collect_refs_for_task(items, task_name, root, names, scopes, out, non_task_imports)
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            collect_refs_for_task_expr(left, task_name, root, names, scopes, out, non_task_imports);
            collect_refs_for_task_expr(
                right,
                task_name,
                root,
                names,
                scopes,
                out,
                non_task_imports,
            );
        }
        Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Bound(_, inner, _)
        | Expr::Unary(_, inner, _) => {
            collect_refs_for_task_expr(
                inner,
                task_name,
                root,
                names,
                scopes,
                out,
                non_task_imports,
            );
        }
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                collect_refs_for_task_expr(
                    item,
                    task_name,
                    root,
                    names,
                    scopes,
                    out,
                    non_task_imports,
                );
            }
        }
        Expr::OpenList(Some(inner), _) => {
            collect_refs_for_task_expr(
                inner,
                task_name,
                root,
                names,
                scopes,
                out,
                non_task_imports,
            );
        }
        _ => {}
    }
}

fn add_output_dependencies(
    items: &[StructItem],
    scheduled: &mut [(String, Vec<String>, BTreeMap<String, Value>)],
    non_task_imports: &ImportKinds,
) {
    let names: std::collections::BTreeSet<String> =
        scheduled.iter().map(|(name, _, _)| name.clone()).collect();
    let mut refs: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    for (name, _, _) in scheduled.iter() {
        let mut found = std::collections::BTreeSet::new();
        let mut scopes = Vec::new();
        collect_refs_for_task(
            items,
            name,
            items,
            &names,
            &mut scopes,
            &mut found,
            non_task_imports,
        );
        refs.insert(name.clone(), found);
    }
    for (name, dependencies, _) in scheduled.iter_mut() {
        if let Some(found) = refs.get(name) {
            for dependency in found {
                if dependency != name && !dependencies.contains(dependency) {
                    dependencies.push(dependency.clone());
                }
            }
        }
    }
}

fn struct_has_task_id(expr: &Expr) -> bool {
    match expr {
        Expr::Struct(items, _) => items.iter().any(|item| {
            matches!(
                item,
                StructItem::Field(field)
                    if matches!(&field.label, Label::Name(name, _) if name == "$id")
            )
        }),
        Expr::Unify(left, right, _) => struct_has_task_id(left) || struct_has_task_id(right),
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            struct_has_task_id(inner)
        }
        _ => false,
    }
}

fn collect_task_decl_ids_items<'a>(
    items: &'a [StructItem],
    root: &'a [StructItem],
    scopes: &mut Vec<&'a [StructItem]>,
    out: &mut std::collections::BTreeSet<usize>,
    non_task_imports: &ImportKinds,
) {
    scopes.push(items);
    for item in items {
        match item {
            StructItem::Field(field) => {
                let is_task = match &field.value {
                    // Plain field references are not injected by CUE; only
                    // `let` bindings receive captured outputs.
                    Expr::Ident(name, _) => {
                        let import_task =
                            reference_import_key(&field.value, scopes, root, non_task_imports)
                                .is_some();
                        let in_place = is_in_place_reference(&field.value, scopes, root);
                        (field.definition
                            || (is_let_binding(scopes, root, name) && !in_place)
                            || import_task
                            || definition_is_alias(scopes, root, name))
                            && is_task_expr(
                                &field.value,
                                scopes,
                                root,
                                &mut std::collections::BTreeSet::new(),
                                non_task_imports,
                            )
                    }
                    value => {
                        struct_has_task_id(value)
                            || (!is_in_place_reference(value, scopes, root)
                                && is_task_expr(
                                    value,
                                    scopes,
                                    root,
                                    &mut std::collections::BTreeSet::new(),
                                    non_task_imports,
                                ))
                    }
                };
                if is_task {
                    out.insert(field.id);
                }
                collect_task_decl_ids_expr(&field.value, root, scopes, out, non_task_imports);
            }
            StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                collect_task_decl_ids_expr(value, root, scopes, out, non_task_imports);
            }
            StructItem::For { source, body, .. } => {
                collect_task_decl_ids_expr(source, root, scopes, out, non_task_imports);
                collect_task_decl_ids_items(body, root, scopes, out, non_task_imports);
            }
            StructItem::If { cond, body, .. } => {
                collect_task_decl_ids_expr(cond, root, scopes, out, non_task_imports);
                collect_task_decl_ids_items(body, root, scopes, out, non_task_imports);
            }
            StructItem::Ellipsis(_) => {}
        }
    }
    scopes.pop();
}

fn collect_task_decl_ids_expr<'a>(
    expr: &'a Expr,
    root: &'a [StructItem],
    scopes: &mut Vec<&'a [StructItem]>,
    out: &mut std::collections::BTreeSet<usize>,
    non_task_imports: &ImportKinds,
) {
    match expr {
        Expr::Struct(items, _) => {
            collect_task_decl_ids_items(items, root, scopes, out, non_task_imports)
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            collect_task_decl_ids_expr(left, root, scopes, out, non_task_imports);
            collect_task_decl_ids_expr(right, root, scopes, out, non_task_imports);
        }
        Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Bound(_, inner, _)
        | Expr::Unary(_, inner, _) => {
            collect_task_decl_ids_expr(inner, root, scopes, out, non_task_imports);
        }
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                collect_task_decl_ids_expr(item, root, scopes, out, non_task_imports);
            }
        }
        Expr::OpenList(Some(inner), _) => {
            collect_task_decl_ids_expr(inner, root, scopes, out, non_task_imports)
        }
        _ => {}
    }
}

fn collect_task_decl_ids(
    items: &[StructItem],
    root: &[StructItem],
    non_task_imports: &ImportKinds,
) -> std::collections::BTreeSet<usize> {
    let mut out = std::collections::BTreeSet::new();
    let mut scopes = Vec::new();
    collect_task_decl_ids_items(items, root, &mut scopes, &mut out, non_task_imports);
    out
}

fn inject_path_expr(
    expr: &mut Expr,
    path: &[String],
    task_name: &str,
    outputs: &[(String, Value)],
    task_decls: &std::collections::BTreeSet<usize>,
    imports: &ImportKinds,
) -> bool {
    let Some((first, rest)) = path.split_first() else {
        return inject_task_outputs_in_expr(expr, task_name, outputs, task_decls, imports);
    };
    match expr {
        Expr::Struct(items, _) => {
            for item in items.iter_mut() {
                let value = match item {
                    StructItem::Field(field) => match &field.label {
                        Label::Name(label, _) if label == first => Some(&mut field.value),
                        _ => None,
                    },
                    StructItem::Let {
                        name: let_name,
                        value,
                        ..
                    } if let_name == first => Some(value),
                    _ => None,
                };
                if let Some(value) = value {
                    return inject_path_expr(value, rest, task_name, outputs, task_decls, imports);
                }
            }
            false
        }
        Expr::Unify(left, right, _) => {
            inject_path_expr(left, path, task_name, outputs, task_decls, imports)
                || inject_path_expr(right, path, task_name, outputs, task_decls, imports)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            inject_path_expr(inner, path, task_name, outputs, task_decls, imports)
        }
        _ => false,
    }
}

fn inject_into_path(
    items: &mut [StructItem],
    path: &[String],
    task_name: &str,
    outputs: &[(String, Value)],
    task_decls: &std::collections::BTreeSet<usize>,
    imports: &ImportKinds,
) -> bool {
    let Some((first, rest)) = path.split_first() else {
        return false;
    };
    for item in items.iter_mut() {
        let value = match item {
            StructItem::Field(field) => match &field.label {
                Label::Name(label, _) if label == first => Some(&mut field.value),
                _ => None,
            },
            StructItem::Let {
                name: let_name,
                value,
                ..
            } if let_name == first => Some(value),
            _ => None,
        };
        if let Some(value) = value {
            if rest.is_empty() {
                return inject_task_outputs_in_expr(value, task_name, outputs, task_decls, imports);
            }
            return inject_path_expr(value, rest, task_name, outputs, task_decls, imports);
        }
    }
    false
}

/// Root import alias of a task value imported from a local module.
fn import_task_alias(expr: &Expr, imports: &ImportKinds) -> Option<String> {
    match expr {
        Expr::Unify(left, right, _) => {
            import_task_alias(left, imports).or_else(|| import_task_alias(right, imports))
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            import_task_alias(inner, imports)
        }
        _ => {
            if !is_import_task_expr(expr, imports) {
                return None;
            }
            let root = selector_names(expr, None)?.first().cloned()?;
            if imports.non_task.contains(&root) {
                Some(root)
            } else {
                None
            }
        }
    }
}

fn is_task_leaf(expr: &Expr) -> bool {
    match expr {
        Expr::Select(..) | Expr::Index(..) | Expr::Ident(..) => true,
        Expr::Unify(left, right, _) => is_task_leaf(left) || is_task_leaf(right),
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            is_task_leaf(inner)
        }
        Expr::Struct(..) => struct_has_task_id(expr),
        _ => false,
    }
}

fn definition_is_alias(scopes: &[&[StructItem]], root: &[StructItem], name: &str) -> bool {
    let Some(item) = local_binding(scopes, root, name) else {
        return false;
    };
    let StructItem::Field(field) = item else {
        return false;
    };
    if !field.definition {
        return false;
    }
    let mut value = &field.value;
    while let Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) = value {
        value = inner;
    }
    let Expr::Ident(target, _) = value else {
        return false;
    };
    matches!(
        local_binding(scopes, root, target),
        Some(StructItem::Field(target_field)) if target_field.definition
    )
}

fn binding_matches(item: &&StructItem, name: &str) -> bool {
    match item {
        StructItem::Field(field) => {
            matches!(&field.label, Label::Name(label, _) if label == name)
        }
        StructItem::Let { name: label, .. } => label == name,
        _ => false,
    }
}

/// Resolve a binding and whether it lives at package root (outside the command
/// subtree) rather than in a nested struct scope.
fn local_binding_location<'a>(
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    name: &str,
) -> Option<(bool, &'a StructItem)> {
    for (index, scope) in scopes.iter().enumerate().rev() {
        if index == 0 && std::ptr::eq(*scope, root) {
            continue;
        }
        if let Some(item) = scope.iter().find(|item| binding_matches(item, name)) {
            return Some((false, item));
        }
    }
    root.iter()
        .find(|item| binding_matches(item, name))
        .map(|item| (true, item))
}

fn local_binding<'a>(
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    name: &str,
) -> Option<&'a StructItem> {
    local_binding_location(scopes, root, name).map(|(_, item)| item)
}

/// True when a local reference is injected at its declaration rather than at
/// the reference: a non-hidden field declared in the command subtree, or a
/// definition anywhere.
fn is_in_place_reference<'a>(
    expr: &'a Expr,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
) -> bool {
    fn walk<'a>(
        expr: &'a Expr,
        scopes: &[&'a [StructItem]],
        root: &'a [StructItem],
        visited: &mut std::collections::BTreeSet<String>,
    ) -> bool {
        match expr {
            Expr::Ident(name, _) => {
                if !visited.insert(name.clone()) {
                    return false;
                }
                match local_binding_location(scopes, root, name) {
                    Some((true, StructItem::Field(field))) => field.definition,
                    Some((false, StructItem::Field(field))) => {
                        field.definition || !name.starts_with('_')
                    }
                    Some((_, StructItem::Let { value, .. })) => walk(value, scopes, root, visited),
                    _ => false,
                }
            }
            Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
                walk(inner, scopes, root, visited)
            }
            _ => false,
        }
    }
    walk(expr, scopes, root, &mut std::collections::BTreeSet::new())
}

fn reference_import_key<'a>(
    expr: &'a Expr,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    imports: &ImportKinds,
) -> Option<String> {
    if is_in_place_reference(expr, scopes, root) {
        return None;
    }
    ident_import_key(
        expr,
        scopes,
        root,
        imports,
        &mut std::collections::BTreeSet::new(),
    )
}

fn ident_import_key<'a>(
    expr: &'a Expr,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    imports: &ImportKinds,
    visited: &mut std::collections::BTreeSet<String>,
) -> Option<String> {
    match expr {
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return None;
            }
            match local_binding(scopes, root, name)? {
                StructItem::Let { value, .. } => {
                    ident_import_key(value, scopes, root, imports, visited)
                }
                StructItem::Field(field) => {
                    ident_import_key(&field.value, scopes, root, imports, visited)
                }
                _ => None,
            }
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            ident_import_key(inner, scopes, root, imports, visited)
        }
        Expr::Unify(..) => None,
        _ => import_task_alias(expr, imports).map(|alias| format!("import:{alias}")),
    }
}

fn definition_binding_name<'a>(
    expr: &'a Expr,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    visited: &mut std::collections::BTreeSet<String>,
) -> Option<String> {
    match expr {
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return None;
            }
            match local_binding(scopes, root, name)? {
                StructItem::Let { value, .. } => {
                    definition_binding_name(value, scopes, root, visited)
                }
                StructItem::Field(field) if field.definition => match &field.label {
                    Label::Name(label, _) => Some(label.clone()),
                    _ => None,
                },
                _ => None,
            }
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            definition_binding_name(inner, scopes, root, visited)
        }
        _ => None,
    }
}

/// Identity of the value a task declaration draws its outputs from.
///
/// Imports are shared by alias and let bindings by name, matching CUE's
/// behaviour of failing to inject when one source feeds several tasks.
fn task_value_key<'a>(
    expr: &'a Expr,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
    imports: &ImportKinds,
) -> Option<String> {
    match expr {
        Expr::Ident(..) => {
            definition_binding_name(expr, scopes, root, &mut std::collections::BTreeSet::new())
                .map(|label| format!("def:{label}"))
                .or_else(|| reference_import_key(expr, scopes, root, imports))
        }
        Expr::Select(..) | Expr::Index(..) => {
            if let Some(path) = resolved_path(
                expr,
                scopes,
                root,
                &mut std::collections::BTreeSet::new(),
                false,
            ) {
                let path = canonical_definition_path(root, &path);
                if path.first().is_some_and(|segment| segment.starts_with('#')) {
                    return Some(path_key(&path));
                }
            }
            import_task_alias(expr, imports).map(|alias| format!("import:{alias}"))
        }
        // Composed task values (`lib.run & {...}`) are distinct per
        // declaration, so CUE injects them independently.
        Expr::Unify(..) => None,
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            task_value_key(inner, scopes, root, imports)
        }
        _ => import_task_alias(expr, imports).map(|alias| format!("import:{alias}")),
    }
}

fn collect_import_task_fields<'a>(
    value: &'a Expr,
    scopes: &mut Vec<&'a [StructItem]>,
    root: &'a [StructItem],
    imports: &ImportKinds,
    visited: &mut std::collections::BTreeSet<String>,
    ids: &mut std::collections::BTreeMap<String, Vec<usize>>,
) {
    match value {
        Expr::Struct(items, _) => {
            scopes.push(items);
            for item in items {
                let StructItem::Field(field) = item else {
                    continue;
                };
                if field.optional
                    || matches!(&field.label, Label::Name(name, _) if name.starts_with('_'))
                {
                    continue;
                }
                let key = if field.definition {
                    match &field.label {
                        Label::Name(name, _) if name.starts_with('#') => {
                            Some(format!("def:{name}"))
                        }
                        _ => None,
                    }
                } else if is_task_leaf(&field.value)
                    && is_task_expr(
                        &field.value,
                        scopes,
                        root,
                        &mut std::collections::BTreeSet::new(),
                        imports,
                    )
                {
                    task_value_key(&field.value, scopes, root, imports)
                } else {
                    None
                };
                if let Some(key) = key {
                    ids.entry(key).or_default().push(field.id);
                }
                if !field.definition {
                    collect_import_task_fields(&field.value, scopes, root, imports, visited, ids);
                }
            }
            scopes.pop();
        }
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return;
            }
            if let Some(value) = local_definition_scopes(scopes, root, name) {
                collect_import_task_fields(value, scopes, root, imports, visited, ids);
            }
        }
        Expr::Unify(left, right, _) => {
            collect_import_task_fields(left, scopes, root, imports, visited, ids);
            collect_import_task_fields(right, scopes, root, imports, visited, ids);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            collect_import_task_fields(inner, scopes, root, imports, visited, ids);
        }
        _ => {}
    }
}

fn collect_definition_fields(
    items: &[StructItem],
    imports: &ImportKinds,
    ids: &mut std::collections::BTreeMap<String, Vec<usize>>,
) {
    for item in items {
        let StructItem::Field(field) = item else {
            continue;
        };
        if matches!(&field.label, Label::Name(name, _) if name == "command") {
            continue;
        }
        if field.definition
            && let Label::Name(name, _) = &field.label
            && name.starts_with('#')
        {
            if !definition_is_alias(&[items], items, name) {
                ids.entry(format!("def:{name}")).or_default().push(field.id);
            }
            collect_definition_leaves(
                &field.value,
                std::slice::from_ref(name),
                items,
                imports,
                ids,
            );
        }
        collect_definition_fields_expr(&field.value, imports, ids);
    }
}

fn collect_definition_leaves(
    expr: &Expr,
    prefix: &[String],
    root: &[StructItem],
    imports: &ImportKinds,
    ids: &mut std::collections::BTreeMap<String, Vec<usize>>,
) {
    let Expr::Struct(items, _) = expr else {
        return;
    };
    for item in items {
        let StructItem::Field(field) = item else {
            continue;
        };
        let Label::Name(name, _) = &field.label else {
            continue;
        };
        let mut path = prefix.to_vec();
        path.push(name.clone());
        if field.definition {
            collect_definition_leaves(&field.value, &path, root, imports, ids);
            continue;
        }
        if is_task_leaf(&field.value)
            && is_task_expr(
                &field.value,
                &[items],
                root,
                &mut std::collections::BTreeSet::new(),
                imports,
            )
        {
            ids.entry(path_key(&path)).or_default().push(field.id);
        }
        if matches!(&field.value, Expr::Struct(..)) {
            collect_definition_leaves(&field.value, &path, root, imports, ids);
        }
    }
}

fn collect_definition_fields_expr(
    expr: &Expr,
    imports: &ImportKinds,
    ids: &mut std::collections::BTreeMap<String, Vec<usize>>,
) {
    match expr {
        Expr::Struct(items, _) => {
            for item in items {
                if let StructItem::Field(field) = item
                    && field.definition
                    && let Label::Name(name, _) = &field.label
                    && name.starts_with('#')
                {
                    ids.entry(format!("def:{name}")).or_default().push(field.id);
                    collect_definition_leaves(
                        &field.value,
                        std::slice::from_ref(name),
                        items,
                        imports,
                        ids,
                    );
                }
            }
        }
        Expr::Unify(left, right, _) => {
            collect_definition_fields_expr(left, imports, ids);
            collect_definition_fields_expr(right, imports, ids);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            collect_definition_fields_expr(inner, imports, ids);
        }
        _ => {}
    }
}

fn definition_key_order(
    command_items: &[StructItem],
    root: &[StructItem],
    imports: &ImportKinds,
) -> std::collections::BTreeMap<String, (Option<usize>, Option<usize>)> {
    let scopes: [&[StructItem]; 1] = [command_items];
    let mut info = std::collections::BTreeMap::new();
    let mut counter = 0usize;
    let mut visited = std::collections::BTreeSet::new();
    for item in command_items {
        let StructItem::Field(field) = item else {
            continue;
        };
        if field.definition
            || field.optional
            || matches!(&field.label, Label::Name(name, _) if !field.quoted && name.starts_with('_'))
        {
            continue;
        }
        let index = counter;
        counter += 1;
        let allow_read = !is_direct_output_read(&field.value);
        scan_order_expr(
            &field.value,
            allow_read,
            true,
            index,
            &scopes,
            root,
            imports,
            &mut info,
            &mut counter,
            &mut visited,
        );
    }
    info
}

fn is_direct_output_read(expr: &Expr) -> bool {
    let mut value = expr;
    while let Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) = value {
        value = inner;
    }
    matches!(value, Expr::Select(_, field, _, _) if matches!(field.as_str(), "stdout" | "stderr" | "success"))
}

fn alias_operand<'a>(
    expr: &'a Expr,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
) -> Option<&'a Expr> {
    match expr {
        Expr::Ident(name, _) => match local_binding(scopes, root, name)? {
            StructItem::Let { value, .. } => Some(value),
            StructItem::Field(field) if !field.definition => Some(&field.value),
            _ => None,
        },
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            alias_operand(inner, scopes, root)
        }
        other => Some(other),
    }
}

fn selected_field_value<'a>(value: &'a Expr, field: &str) -> Option<&'a Expr> {
    match value {
        Expr::Struct(items, _) => items.iter().find_map(|item| match item {
            StructItem::Field(field_item)
                if matches!(&field_item.label, Label::Name(label, _) if label == field) =>
            {
                Some(&field_item.value)
            }
            _ => None,
        }),
        Expr::Unify(left, right, _) => {
            selected_field_value(left, field).or_else(|| selected_field_value(right, field))
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            selected_field_value(inner, field)
        }
        _ => None,
    }
}

fn selected_alias_value<'a>(
    expr: &'a Expr,
    scopes: &[&'a [StructItem]],
    root: &'a [StructItem],
) -> Option<&'a Expr> {
    match expr {
        Expr::Select(base, field, _, _) => {
            let value = alias_operand(base, scopes, root)?;
            selected_field_value(value, field)
        }
        Expr::Index(base, index, _) => {
            let target = index_field_name(index)?;
            let value = alias_operand(base, scopes, root)?;
            if let Expr::List(items, _) = value
                && let Ok(position) = target.parse::<usize>()
            {
                return items.get(position);
            }
            selected_field_value(value, &target)
        }
        _ => None,
    }
}

fn definition_declaration_key(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &ImportKinds,
) -> Option<String> {
    if let Some(key) = task_value_key(expr, scopes, root, imports)
        && key.starts_with("def:")
    {
        return Some(key);
    }
    let (base, field) = match expr {
        Expr::Select(base, field, _, _) => (base, field.clone()),
        Expr::Index(base, index, _) => (base, index_field_name(index)?),
        _ => return None,
    };
    let mut path = resolved_read_path(
        base,
        scopes,
        root,
        &mut std::collections::BTreeSet::new(),
        imports,
    )?;
    path.push(field);
    let path = canonical_definition_path(root, &path);
    path.first()
        .is_some_and(|segment| segment.starts_with('#'))
        .then(|| path_key(&path))
}

#[allow(clippy::too_many_arguments)]
fn scan_order_expr(
    expr: &Expr,
    allow_read: bool,
    allow_decl: bool,
    index: usize,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &ImportKinds,
    info: &mut std::collections::BTreeMap<String, (Option<usize>, Option<usize>)>,
    counter: &mut usize,
    visited: &mut std::collections::BTreeSet<String>,
) {
    match expr {
        Expr::Select(base, field, _, _) => {
            if matches!(field.as_str(), "stdout" | "stderr" | "success") {
                if allow_read
                    && let Some(path) = resolved_read_path(
                        base,
                        scopes,
                        root,
                        &mut std::collections::BTreeSet::new(),
                        imports,
                    )
                {
                    let path = canonical_definition_path(root, &path);
                    if path.first().is_some_and(|segment| segment.starts_with('#')) {
                        let entry = info.entry(path_key(&path)).or_insert((None, None));
                        entry.1 = Some(entry.1.map_or(index, |existing| existing.min(index)));
                    }
                }
            } else {
                if allow_decl
                    && let Some(key) = definition_declaration_key(expr, scopes, root, imports)
                {
                    let entry = info.entry(key).or_insert((None, None));
                    entry.0 = Some(entry.0.map_or(index, |existing| existing.min(index)));
                }
                if allow_read && let Some(value) = selected_alias_value(expr, scopes, root) {
                    scan_order_expr(
                        value, true, false, index, scopes, root, imports, info, counter, visited,
                    );
                }
            }
            scan_order_expr(
                base, false, false, index, scopes, root, imports, info, counter, visited,
            );
        }
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return;
            }
            match local_binding(scopes, root, name) {
                Some(StructItem::Let { value, .. })
                | Some(StructItem::Field(Field { value, .. }))
                    if !matches!(
                        local_binding(scopes, root, name),
                        Some(StructItem::Field(field)) if field.definition
                    ) =>
                {
                    scan_order_expr(
                        value, allow_read, allow_decl, index, scopes, root, imports, info, counter,
                        visited,
                    );
                }
                _ => {
                    if allow_decl
                        && let Some(key) = task_value_key(expr, scopes, root, imports)
                        && key.starts_with("def:")
                    {
                        let entry = info.entry(key).or_insert((None, None));
                        entry.0 = Some(entry.0.map_or(index, |existing| existing.min(index)));
                    }
                }
            }
            visited.remove(name);
        }
        Expr::Index(base, _, _) => {
            if allow_decl && let Some(key) = definition_declaration_key(expr, scopes, root, imports)
            {
                let entry = info.entry(key).or_insert((None, None));
                entry.0 = Some(entry.0.map_or(index, |existing| existing.min(index)));
            }
            if allow_read && let Some(value) = selected_alias_value(expr, scopes, root) {
                scan_order_expr(
                    value, true, false, index, scopes, root, imports, info, counter, visited,
                );
            }
            scan_order_expr(
                base, false, false, index, scopes, root, imports, info, counter, visited,
            );
        }
        Expr::Struct(items, _) => {
            let scope_items = items.clone();
            let mut nested = scopes.to_vec();
            nested.push(&scope_items);
            for item in items {
                let StructItem::Field(field) = item else {
                    continue;
                };
                if field.definition
                    || field.optional
                    || matches!(&field.label, Label::Name(name, _) if !field.quoted && name.starts_with('_'))
                {
                    continue;
                }
                let field_index = *counter;
                *counter += 1;
                let field_read = !is_direct_output_read(&field.value);
                scan_order_expr(
                    &field.value,
                    field_read,
                    allow_decl,
                    field_index,
                    &nested,
                    root,
                    imports,
                    info,
                    counter,
                    visited,
                );
            }
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            scan_order_expr(
                left, allow_read, allow_decl, index, scopes, root, imports, info, counter, visited,
            );
            scan_order_expr(
                right, allow_read, allow_decl, index, scopes, root, imports, info, counter, visited,
            );
        }
        Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Bound(_, inner, _)
        | Expr::Unary(_, inner, _) => scan_order_expr(
            inner, allow_read, allow_decl, index, scopes, root, imports, info, counter, visited,
        ),
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                scan_order_expr(
                    item, allow_read, allow_decl, index, scopes, root, imports, info, counter,
                    visited,
                );
            }
        }
        Expr::OpenList(Some(inner), _) => scan_order_expr(
            inner, allow_read, allow_decl, index, scopes, root, imports, info, counter, visited,
        ),
        Expr::Str(parts, _) => {
            for part in parts {
                if let ostinato_lang::ast::StrPart::Interp(inner) = part {
                    scan_order_expr(
                        inner, true, true, index, scopes, root, imports, info, counter, visited,
                    );
                }
            }
        }
        Expr::BytesInterp(parts, _) => {
            for part in parts {
                if let ostinato_lang::ast::BytePart::Interp(inner) = part {
                    scan_order_expr(
                        inner, true, true, index, scopes, root, imports, info, counter, visited,
                    );
                }
            }
        }
        Expr::Call(callee, arguments, _) => {
            scan_order_expr(
                callee, allow_read, allow_decl, index, scopes, root, imports, info, counter,
                visited,
            );
            for argument in arguments {
                scan_order_expr(
                    argument, allow_read, allow_decl, index, scopes, root, imports, info, counter,
                    visited,
                );
            }
        }
        _ => {}
    }
}

fn active_task_decl_ids(
    items: &[StructItem],
    command_name: &str,
    non_task_imports: &ImportKinds,
) -> std::collections::BTreeSet<usize> {
    let task_decls = collect_task_decl_ids(items, items, non_task_imports);
    // CUE injects outputs for an imported task only when the import alias
    // provides a single task value; multiple task declarations from the same
    // module are left non-concrete. Mirror that behaviour.
    let mut import_task_fields: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    for item in items.iter() {
        let StructItem::Field(field) = item else {
            continue;
        };
        if !matches!(&field.label, Label::Name(name, _) if name == "command") {
            continue;
        }
        let Expr::Struct(command_items, _) = &field.value else {
            continue;
        };
        for command_item in command_items {
            let StructItem::Field(command_field) = command_item else {
                continue;
            };
            if matches!(&command_field.label, Label::Name(name, _) if name == command_name) {
                // Resolve referenced/let/parenthesised command values so the
                // suppression pass sees the same declarations that get
                // scheduled.
                let mut resolved = Vec::new();
                resolve_command_items(
                    items,
                    &command_field.value,
                    &mut std::collections::BTreeSet::new(),
                    &mut resolved,
                );
                let resolved_expr = Expr::Struct(resolved, command_field.value.span());
                let mut scopes = Vec::new();
                collect_import_task_fields(
                    &resolved_expr,
                    &mut scopes,
                    items,
                    non_task_imports,
                    &mut std::collections::BTreeSet::new(),
                    &mut import_task_fields,
                );
            }
        }
    }
    collect_definition_fields(items, non_task_imports, &mut import_task_fields);
    let mut ordered_definitions = std::collections::BTreeSet::new();
    for item in items.iter() {
        let StructItem::Field(field) = item else {
            continue;
        };
        if !matches!(&field.label, Label::Name(name, _) if name == "command") {
            continue;
        }
        let Expr::Struct(command_items, _) = &field.value else {
            continue;
        };
        for command_item in command_items {
            let StructItem::Field(command_field) = command_item else {
                continue;
            };
            if matches!(&command_field.label, Label::Name(name, _) if name == command_name) {
                let mut resolved = Vec::new();
                resolve_command_items(
                    items,
                    &command_field.value,
                    &mut std::collections::BTreeSet::new(),
                    &mut resolved,
                );
                for (key, (decl, read)) in definition_key_order(&resolved, items, non_task_imports)
                {
                    if let (Some(decl), Some(read)) = (decl, read)
                        && decl < read
                    {
                        ordered_definitions.insert(key);
                    }
                }
            }
        }
    }
    let mut suppressed = std::collections::BTreeSet::new();
    for (key, ids) in &import_task_fields {
        if key.starts_with("def:") {
            if ordered_definitions.contains(key) {
                suppressed.extend(ids.iter().copied());
            }
        } else if ids.len() > 1 {
            suppressed.extend(ids.iter().copied());
        }
    }
    task_decls
        .into_iter()
        .filter(|id| !suppressed.contains(id))
        .collect()
}

fn inject_command_outputs(
    items: &mut [StructItem],
    command_name: &str,
    task_name: &str,
    outputs: &[(String, Value)],
    non_task_imports: &ImportKinds,
) -> bool {
    let task_decls = active_task_decl_ids(items, command_name, non_task_imports);
    // Collect referenced declarations before taking a mutable borrow.
    let mut targets: Vec<Vec<String>> = Vec::new();
    for item in items.iter() {
        let StructItem::Field(field) = item else {
            continue;
        };
        if !matches!(&field.label, Label::Name(name, _) if name == "command") {
            continue;
        }
        let Expr::Struct(command_items, _) = &field.value else {
            continue;
        };
        for command_item in command_items {
            let StructItem::Field(command_field) = command_item else {
                continue;
            };
            if !matches!(&command_field.label, Label::Name(name, _) if name == command_name) {
                continue;
            }
            let mut visited = std::collections::BTreeSet::new();
            command_targets(items, &command_field.value, &mut visited, &mut targets);
        }
    }

    let mut injected = false;
    // Inject into literal struct/unified command values.
    for item in items.iter_mut() {
        let StructItem::Field(field) = item else {
            continue;
        };
        if !matches!(&field.label, Label::Name(name, _) if name == "command") {
            continue;
        }
        let Expr::Struct(command_items, _) = &mut field.value else {
            continue;
        };
        for command_item in command_items {
            let StructItem::Field(command_field) = command_item else {
                continue;
            };
            if !matches!(&command_field.label, Label::Name(name, _) if name == command_name) {
                continue;
            }
            if inject_task_outputs_in_expr(
                &mut command_field.value,
                task_name,
                outputs,
                &task_decls,
                non_task_imports,
            ) {
                injected = true;
            }
        }
    }
    // Inject into referenced declarations (`command: c: base`, selectors, lets).
    for target in targets {
        if inject_into_path(
            items,
            &target,
            task_name,
            outputs,
            &task_decls,
            non_task_imports,
        ) {
            injected = true;
        }
    }
    injected
}

fn task_output_struct(outputs: &[(String, Value)], span: Span) -> Option<Expr> {
    let mut items = Vec::new();
    for (name, value) in outputs {
        let Some(expr) = output_expr(value) else {
            continue;
        };
        items.push(StructItem::Field(Field {
            id: 0,
            label: Label::Name(name.clone(), span),
            value: expr,
            optional: false,
            definition: false,
            alias: None,
            quoted: false,
            bytes_label_runes: None,
            bytes_label_multiline: false,
            attributes: Vec::new(),
            span,
        }));
    }
    if items.is_empty() {
        None
    } else {
        Some(Expr::Struct(items, span))
    }
}

fn inject_task_outputs_value(
    expr: &mut Expr,
    outputs: &[(String, Value)],
    imports: &ImportKinds,
) -> bool {
    match expr {
        Expr::Unify(..) => {
            let span = expr.span();
            let Some(wrapper) = task_output_struct(outputs, span) else {
                return false;
            };
            let current = expr.clone();
            *expr = Expr::Unify(Box::new(current), Box::new(wrapper), span);
            true
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            inject_task_outputs_value(inner, outputs, imports)
        }
        Expr::Select(..) | Expr::Index(..) => {
            if !is_import_task_expr(expr, imports) {
                return false;
            }
            let span = expr.span();
            let Some(wrapper) = task_output_struct(outputs, span) else {
                return false;
            };
            let current = expr.clone();
            *expr = Expr::Unify(Box::new(current), Box::new(wrapper), span);
            true
        }
        Expr::Struct(..) => {
            let mut injected = false;
            for (name, value) in outputs {
                if let Some(value_expr) = output_expr(value)
                    && inject_into_struct(expr, name, &value_expr)
                {
                    injected = true;
                }
            }
            injected
        }
        _ => false,
    }
}

fn inject_task_outputs(
    items: &mut [StructItem],
    task_name: &str,
    outputs: &[(String, Value)],
    task_decls: &std::collections::BTreeSet<usize>,
    imports: &ImportKinds,
) -> bool {
    for item in items {
        match item {
            StructItem::Field(field) => {
                let is_task = matches!(&field.label, Label::Name(name, _) if name == task_name)
                    && task_decls.contains(&field.id);
                if is_task {
                    let injected = if matches!(&field.value, Expr::Ident(..)) {
                        let span = field.value.span();
                        match task_output_struct(outputs, span) {
                            Some(wrapper) => {
                                let current = field.value.clone();
                                field.value =
                                    Expr::Unify(Box::new(current), Box::new(wrapper), span);
                                true
                            }
                            None => false,
                        }
                    } else {
                        inject_task_outputs_value(&mut field.value, outputs, imports)
                    };
                    if injected {
                        return true;
                    }
                }
                if inject_task_outputs_in_expr(
                    &mut field.value,
                    task_name,
                    outputs,
                    task_decls,
                    imports,
                ) {
                    return true;
                }
            }
            StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                if inject_task_outputs_in_expr(value, task_name, outputs, task_decls, imports) {
                    return true;
                }
            }
            StructItem::For { source, body, .. } => {
                if inject_task_outputs_in_expr(source, task_name, outputs, task_decls, imports)
                    || inject_task_outputs(body, task_name, outputs, task_decls, imports)
                {
                    return true;
                }
            }
            StructItem::If { cond, body, .. } => {
                if inject_task_outputs_in_expr(cond, task_name, outputs, task_decls, imports)
                    || inject_task_outputs(body, task_name, outputs, task_decls, imports)
                {
                    return true;
                }
            }
            StructItem::Ellipsis(_) => {}
        }
    }
    false
}

fn inject_task_outputs_in_expr(
    expr: &mut Expr,
    task_name: &str,
    outputs: &[(String, Value)],
    task_decls: &std::collections::BTreeSet<usize>,
    imports: &ImportKinds,
) -> bool {
    match expr {
        Expr::Struct(items, _) => {
            inject_task_outputs(items, task_name, outputs, task_decls, imports)
        }
        Expr::Unify(left, right, _) => {
            inject_task_outputs_in_expr(left, task_name, outputs, task_decls, imports)
                || inject_task_outputs_in_expr(right, task_name, outputs, task_decls, imports)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) => {
            inject_task_outputs_in_expr(inner, task_name, outputs, task_decls, imports)
        }
        _ => false,
    }
}

fn extract_command_tasks(
    value: &Value,
    command_name: &str,
) -> Result<BTreeMap<String, Value>, ostinato_lang::LangError> {
    let Some(root) = value_fields(value) else {
        return Err(ostinato_lang::LangError::Workflow(
            "tool package did not evaluate to a struct".to_owned(),
        ));
    };
    let Some(command) = root.get("command").and_then(value_fields) else {
        return Err(ostinato_lang::LangError::Workflow(
            "no `command` field in tool files".to_owned(),
        ));
    };
    let Some(tasks) = command.get(command_name).and_then(value_fields) else {
        return Err(ostinato_lang::LangError::Workflow(format!(
            "unknown tool command `{command_name}`"
        )));
    };
    if tasks.contains_key("$id") {
        let mut wrapped = BTreeMap::new();
        wrapped.insert(command_name.to_owned(), Value::Struct(tasks.clone()));
        return Ok(wrapped);
    }
    Ok(tasks.clone())
}

fn has_compatible(value: &Value, compatible: fn(&Value) -> bool) -> bool {
    match value {
        Value::Disjunction(alternatives, _) => alternatives
            .iter()
            .any(|alternative| has_compatible(alternative, compatible)),
        value => compatible(value),
    }
}

fn validate_task_inputs(
    tasks: &BTreeMap<String, BTreeMap<String, Value>>,
) -> Result<(), ostinato_lang::LangError> {
    for task in tasks.values() {
        let Some(Value::String(id)) = task.get("$id") else {
            continue;
        };
        match id.as_str() {
            "tool/exec.Run" => {
                if let Some(cmd) = task.get("cmd") {
                    if matches!(cmd, Value::Disjunction(..)) {
                        if !has_compatible(cmd, |value| {
                            matches!(
                                value,
                                Value::String(_) | Value::List(_) | Value::Type(_) | Value::Bottom
                            )
                        }) {
                            return Err(ostinato_lang::LangError::Workflow(
                                "tool/exec.Run `cmd` must be a string or list of strings"
                                    .to_owned(),
                            ));
                        }
                    } else {
                        match cmd {
                            Value::String(text) => {
                                if text.split_whitespace().next().is_none() {
                                    return Err(ostinato_lang::LangError::Workflow(
                                        "tool/exec.Run `cmd` must not be empty".to_owned(),
                                    ));
                                }
                            }
                            Value::List(items) => {
                                if items.is_empty() {
                                    return Err(ostinato_lang::LangError::Workflow(
                                        "tool/exec.Run `cmd` must not be empty".to_owned(),
                                    ));
                                }
                                for item in items {
                                    if !has_compatible(item, |value| {
                                        matches!(
                                            value,
                                            Value::String(_)
                                                | Value::Type(_)
                                                | Value::Bottom
                                                | Value::Recursive(_)
                                                | Value::Deferred(..)
                                        )
                                    }) {
                                        return Err(ostinato_lang::LangError::Workflow(
                                            "tool/exec.Run `cmd` element must be a string"
                                                .to_owned(),
                                        ));
                                    }
                                }
                            }
                            Value::Type(_) | Value::Bottom => {}
                            _ => {
                                return Err(ostinato_lang::LangError::Workflow(
                                    "tool/exec.Run `cmd` must be a string or list of strings"
                                        .to_owned(),
                                ));
                            }
                        }
                    }
                }
                if let Some(value) = task.get("dir")
                    && !has_compatible(value, |value| {
                        matches!(value, Value::String(_) | Value::Type(_) | Value::Bottom)
                    })
                {
                    return Err(ostinato_lang::LangError::Workflow(
                        "tool/exec.Run `dir` must be a string".to_owned(),
                    ));
                }
                if let Some(value) = task.get("env")
                    && !has_compatible(value, |value| {
                        matches!(
                            value,
                            Value::Struct(_)
                                | Value::Definition { .. }
                                | Value::List(_)
                                | Value::Type(_)
                                | Value::Bottom
                        )
                    })
                {
                    return Err(ostinato_lang::LangError::Workflow(
                        "tool/exec.Run `env` must be a struct or list".to_owned(),
                    ));
                }
                match task.get("env") {
                    None
                    | Some(Value::Type(_))
                    | Some(Value::Bottom)
                    | Some(Value::Disjunction(..)) => {}
                    Some(Value::Struct(env)) | Some(Value::Definition { fields: env, .. }) => {
                        for value in env.values() {
                            if !has_compatible(value, |value| {
                                matches!(value, Value::String(_) | Value::Type(_) | Value::Bottom)
                            }) {
                                return Err(ostinato_lang::LangError::Workflow(
                                    "tool/exec.Run `env` values must be strings".to_owned(),
                                ));
                            }
                        }
                    }
                    Some(Value::List(entries)) => {
                        for entry in entries {
                            if !has_compatible(entry, |value| {
                                matches!(value, Value::String(_) | Value::Type(_) | Value::Bottom)
                            }) {
                                return Err(ostinato_lang::LangError::Workflow(
                                    "tool/exec.Run `env` list entries must be `NAME=value` strings"
                                        .to_owned(),
                                ));
                            }
                        }
                    }
                    Some(_) => {
                        return Err(ostinato_lang::LangError::Workflow(
                            "tool/exec.Run `env` must be a struct or list".to_owned(),
                        ));
                    }
                }
                if let Some(value) = task.get("stdin")
                    && !has_compatible(value, |value| {
                        matches!(
                            value,
                            Value::Null
                                | Value::String(_)
                                | Value::Bytes(_)
                                | Value::Type(_)
                                | Value::Bottom
                        )
                    })
                {
                    return Err(ostinato_lang::LangError::Workflow(
                        "tool/exec.Run `stdin` must be a string or bytes".to_owned(),
                    ));
                }
                for key in ["stdout", "stderr"] {
                    match capture_kind(task, key) {
                        Ok(CaptureKind::Other) => {
                            return Err(ostinato_lang::LangError::Workflow(format!(
                                "invalid {key}: conflicting values bytes and string"
                            )));
                        }
                        Ok(CaptureKind::None | CaptureKind::String) => {}
                        Err(err) => return Err(err),
                    }
                }
                if let Some(value) = task.get("mustSucceed")
                    && !has_compatible(value, |value| {
                        matches!(value, Value::Bool(_) | Value::Type(_) | Value::Bottom)
                    })
                {
                    return Err(ostinato_lang::LangError::Workflow(
                        "tool/exec.Run `mustSucceed` must be a bool".to_owned(),
                    ));
                }
            }
            "tool/cli.Print" => {
                if let Some(value) = task.get("text")
                    && !has_compatible(value, |value| {
                        matches!(value, Value::String(_) | Value::Type(_) | Value::Bottom)
                    })
                {
                    return Err(ostinato_lang::LangError::Workflow(
                        "tool/cli.Print `text` must be a string".to_owned(),
                    ));
                }
            }
            "tool/os.Getenv" | "tool/os.Environ" | "tool/os.Setenv" | "tool/os.Clearenv" => {
                for (name, value) in task {
                    if name.starts_with('$') {
                        continue;
                    }
                    if !env_constraint_allowed(value) {
                        return Err(ostinato_lang::LangError::Workflow(format!(
                            "invalid type for environment variable {name}"
                        )));
                    }
                }
            }
            "tool/file.Append" | "tool/file.Create" => {
                if let Some(value) = task.get("filename") {
                    match value.concretize() {
                        Value::String(filename) if filename.is_empty() => {
                            return Err(ostinato_lang::LangError::Workflow(
                                "tool/file task `filename` must not be empty".to_owned(),
                            ));
                        }
                        Value::String(_) | Value::Type(_) | Value::Bottom => {}
                        _ => {
                            return Err(ostinato_lang::LangError::Workflow(
                                "tool/file task `filename` must be a string".to_owned(),
                            ));
                        }
                    }
                }
                if let Some(value) = task.get("contents")
                    && !matches!(
                        value.concretize(),
                        Value::String(_) | Value::Bytes(_) | Value::Type(_) | Value::Bottom
                    )
                {
                    return Err(ostinato_lang::LangError::Workflow(
                        "tool/file task `contents` must be a string or bytes".to_owned(),
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn execute_tool_task(
    task: &BTreeMap<String, Value>,
    package_dir: &Path,
) -> Result<ToolTaskOutcome, ostinato_lang::LangError> {
    let Some(Value::String(id)) = task.get("$id") else {
        return Err(ostinato_lang::LangError::Workflow(
            "tool task is missing a `$id`; use a tool package task such as `exec.Run`".to_owned(),
        ));
    };
    match id.as_str() {
        "tool/exec.Run" => execute_exec(task, package_dir),
        "tool/cli.Print" => execute_print(task),
        "tool/file.Append" => execute_file(task, package_dir, false),
        "tool/file.Create" => execute_file(task, package_dir, true),
        "tool/os.Getenv" => execute_os_getenv(task),
        "tool/os.Environ" => execute_os_environ(task),
        "tool/http.Do" => execute_http(task),
        other => Err(ostinato_lang::LangError::Workflow(format!(
            "tool task `{other}` is not implemented"
        ))),
    }
}

fn execute_os_getenv(
    task: &BTreeMap<String, Value>,
) -> Result<ToolTaskOutcome, ostinato_lang::LangError> {
    let mut outputs = Vec::new();
    for (name, constraint) in task {
        if name.starts_with('$') {
            continue;
        }
        if let Some(raw) = std::env::var_os(name) {
            let value = env_parse(name, &raw.to_string_lossy(), constraint)?;
            outputs.push((name.clone(), value));
        }
        // Unset variables are left untouched: CUE does not overwrite the
        // declared field or its default.
    }
    Ok(ToolTaskOutcome::Done { outputs })
}

fn execute_os_environ(
    task: &BTreeMap<String, Value>,
) -> Result<ToolTaskOutcome, ostinato_lang::LangError> {
    let mut outputs = Vec::new();
    for (name, raw) in std::env::vars_os() {
        let name = name.to_string_lossy().into_owned();
        if name.starts_with('$') {
            continue;
        }
        let value = match task.get(&name) {
            Some(constraint) => env_parse(&name, &raw.to_string_lossy(), constraint)?,
            None => Value::String(raw.to_string_lossy().into_owned()),
        };
        outputs.push((name, value));
    }
    Ok(ToolTaskOutcome::Done { outputs })
}

const ENV_STRING: u8 = 1;
const ENV_BOOL: u8 = 2;
const ENV_INT: u8 = 4;
const ENV_FLOAT: u8 = 8;
const ENV_NULL: u8 = 16;
const ENV_OTHER: u8 = 32;
const ENV_NUMBER: u8 = ENV_INT | ENV_FLOAT;

/// The kinds accepted by `tool/os`'s `Value: bool | number | *string | null`.
fn env_value_kinds(value: &Value) -> u8 {
    match value {
        Value::Type(ostinato_lang::TypeKind::String) | Value::String(_) => ENV_STRING,
        Value::Type(ostinato_lang::TypeKind::Bool) | Value::Bool(_) => ENV_BOOL,
        Value::Type(ostinato_lang::TypeKind::Int) | Value::Int(_) | Value::BigInt(_) => ENV_INT,
        Value::Type(ostinato_lang::TypeKind::Float) | Value::Float(_) => ENV_FLOAT,
        Value::Type(ostinato_lang::TypeKind::Number) => ENV_NUMBER,
        Value::Bounds(bounds) => {
            // Bounds are conjuncts: start from the declared type (or the
            // unbounded `string | bool | number` domain) and intersect every
            // operand's kind. `!=null` therefore does not add a boolean
            // alternative; it only removes null from the domain.
            let mut kinds = match bounds.ty {
                Some(ostinato_lang::TypeKind::String) => ENV_STRING,
                Some(ostinato_lang::TypeKind::Bool) => ENV_BOOL,
                Some(ostinato_lang::TypeKind::Int) => ENV_INT,
                Some(ostinato_lang::TypeKind::Float) => ENV_FLOAT,
                Some(ostinato_lang::TypeKind::Number) => ENV_NUMBER,
                Some(ostinato_lang::TypeKind::Bytes) => ENV_OTHER,
                None => ENV_STRING | ENV_BOOL | ENV_NUMBER,
            };
            for bound in &bounds.items {
                let operand = match &bound.operand {
                    Value::String(_) => ENV_STRING,
                    Value::Bool(_) => ENV_BOOL,
                    Value::Int(_) | Value::BigInt(_) | Value::Float(_) => ENV_NUMBER,
                    Value::Null => ENV_STRING | ENV_BOOL | ENV_NUMBER,
                    other => env_value_kinds(other),
                };
                kinds &= operand;
            }
            if kinds == 0 { ENV_OTHER } else { kinds }
        }
        Value::Null => ENV_NULL,
        Value::Disjunction(alternatives, _) => alternatives
            .iter()
            .fold(0, |kinds, alternative| kinds | env_value_kinds(alternative)),
        _ => ENV_OTHER,
    }
}

fn env_constraint_allowed(value: &Value) -> bool {
    env_value_kinds(value) & ENV_OTHER == 0
}

fn bool_env_value(text: &str) -> Option<bool> {
    match text.trim() {
        "1" | "t" | "T" | "true" | "TRUE" | "True" => Some(true),
        "0" | "f" | "F" | "false" | "FALSE" | "False" => Some(false),
        _ => None,
    }
}

struct EnvNumber {
    is_int: bool,
    int: Option<BigInt>,
    float: Option<BigDecimal>,
}

/// Parse the CUE number literal grammar used by `ParseValue` (decimal,
/// hexadecimal, octal, binary, underscores, fractions, exponents and
/// K/M/G/T/P multipliers).
fn parse_env_number(text: &str) -> Result<EnvNumber, String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut negative = false;
    if let Some(sign) = bytes.first()
        && (*sign == b'-' || *sign == b'+')
    {
        negative = *sign == b'-';
        index += 1;
    }
    if index >= bytes.len() {
        return Err(format!("illegal number start {text:?}"));
    }

    if bytes[index] == b'0'
        && let Some(prefix) = bytes.get(index + 1)
    {
        let (radix, skip) = match prefix {
            b'x' | b'X' => (16, 2),
            b'b' => (2, 2),
            b'o' => (8, 2),
            _ => (0, 0),
        };
        if radix != 0 {
            index += skip;
            let (digits, next) = scan_env_digits(bytes, index, radix)?;
            if digits.is_empty() {
                return Err(format!("illegal number start {text:?}"));
            }
            if next != bytes.len() {
                return Err(format!("illegal number {text:?}"));
            }
            let mut value = BigInt::parse_bytes(digits.as_bytes(), radix)
                .ok_or_else(|| format!("invalid number {text:?}"))?;
            if negative {
                value = -value;
            }
            return Ok(EnvNumber {
                is_int: true,
                int: Some(value),
                float: None,
            });
        }
    }

    let mut is_float = false;
    let mut int_digits = String::new();
    let mut frac_digits = String::new();
    let mut started_with_zero = false;

    if bytes[index] == b'.' {
        is_float = true;
        index += 1;
        let (digits, next) = scan_env_digits(bytes, index, 10)?;
        if digits.is_empty() {
            return Err(format!("illegal fraction {text:?}"));
        }
        frac_digits = digits;
        index = next;
    } else if bytes[index] == b'0' {
        started_with_zero = true;
        index += 1;
        let (digits, next) = scan_env_digits(bytes, index, 10)?;
        int_digits = digits;
        index = next;
    } else {
        let (digits, next) = scan_env_digits(bytes, index, 10)?;
        if digits.is_empty() {
            return Err(format!("illegal number start {text:?}"));
        }
        int_digits = digits;
        index = next;
    }

    if index < bytes.len() && bytes[index] == b'.' {
        is_float = true;
        index += 1;
        let (digits, next) = scan_env_digits(bytes, index, 10)?;
        frac_digits = digits;
        index = next;
    }

    let mut exponent: i64 = 0;
    let mut saw_exponent = false;
    if index < bytes.len() && matches!(bytes[index], b'e' | b'E') {
        is_float = true;
        saw_exponent = true;
        index += 1;
        let mut exp_negative = false;
        if index < bytes.len() && matches!(bytes[index], b'+' | b'-') {
            exp_negative = bytes[index] == b'-';
            index += 1;
        }
        let (digits, next) = scan_env_digits(bytes, index, 10)?;
        if digits.is_empty() {
            return Err(format!("illegal exponent {text:?}"));
        }
        exponent = digits
            .parse::<i64>()
            .map_err(|_| format!("invalid number {text:?}"))?;
        if exp_negative {
            exponent = -exponent;
        }
        index = next;
    }

    let mut multiplier: Option<BigInt> = None;
    if !saw_exponent
        && index < bytes.len()
        && let Some(power) = match bytes[index] {
            b'K' => Some(3u32),
            b'M' => Some(6),
            b'G' => Some(9),
            b'T' => Some(12),
            b'P' => Some(15),
            _ => None,
        }
    {
        if started_with_zero && !int_digits.is_empty() && !is_float {
            return Err(format!("illegal integer number {text:?}"));
        }
        index += 1;
        let binary = index < bytes.len() && bytes[index] == b'i';
        if binary {
            index += 1;
        }
        multiplier = Some(if binary {
            BigInt::from(2u32).pow(power * 10 / 3)
        } else {
            BigInt::from(10u32).pow(power)
        });
    }
    if index != bytes.len() {
        return Err(format!("illegal number {text:?}"));
    }
    if started_with_zero && !is_float && multiplier.is_none() && !int_digits.is_empty() {
        return Err(format!("illegal integer number {text:?}"));
    }
    let int_part = if int_digits.is_empty() {
        "0"
    } else {
        int_digits.as_str()
    };
    if is_float {
        let mut literal = if frac_digits.is_empty() {
            int_part.to_owned()
        } else {
            format!("{int_part}.{frac_digits}")
        };
        if exponent != 0 {
            literal.push('e');
            literal.push_str(&exponent.to_string());
        }
        let mut value =
            BigDecimal::from_str(&literal).map_err(|_| format!("invalid number {text:?}"))?;
        if negative {
            value = -value;
        }
        if let Some(multiplier) = multiplier {
            value *= BigDecimal::from(multiplier);
            let integer = value
                .to_bigint()
                .ok_or_else(|| format!("number cannot be represented as int: {text:?}"))?;
            if BigDecimal::from(integer.clone()) != value {
                return Err(format!("number cannot be represented as int: {text:?}"));
            }
            return Ok(EnvNumber {
                is_int: true,
                int: Some(integer),
                float: None,
            });
        }
        return Ok(EnvNumber {
            is_int: false,
            int: None,
            float: Some(value),
        });
    }

    let mut value = BigInt::parse_bytes(int_part.as_bytes(), 10)
        .ok_or_else(|| format!("invalid number {text:?}"))?;
    if let Some(multiplier) = multiplier {
        value *= multiplier;
    }
    if negative {
        value = -value;
    }
    Ok(EnvNumber {
        is_int: true,
        int: Some(value),
        float: None,
    })
}

fn scan_env_digits(bytes: &[u8], start: usize, radix: u32) -> Result<(String, usize), String> {
    let mut index = start;
    let mut digits = String::new();
    let mut previous_underscore = false;
    while index < bytes.len() {
        let ch = bytes[index] as char;
        let valid = match radix {
            2 => matches!(ch, '0' | '1'),
            8 => ('0'..='7').contains(&ch),
            10 => ch.is_ascii_digit(),
            16 => ch.is_ascii_hexdigit(),
            _ => false,
        };
        if ch == '_' {
            if previous_underscore {
                return Err("illegal '_' in number".to_owned());
            }
            previous_underscore = true;
            index += 1;
            continue;
        }
        if !valid {
            break;
        }
        digits.push(ch);
        previous_underscore = false;
        index += 1;
    }
    if previous_underscore {
        return Err("illegal '_' in number".to_owned());
    }
    Ok((digits, index))
}

fn env_number_value(name: &str, text: &str, number: EnvNumber, kinds: u8) -> Result<Value, String> {
    if number.is_int {
        let value = number
            .int
            .ok_or_else(|| format!("invalid number for injection tag \"{name}\": {text:?}"))?;
        return match i64::try_from(&value) {
            Ok(value) => Ok(Value::Int(value)),
            Err(_) => Ok(Value::BigInt(value)),
        };
    }
    if kinds & ENV_FLOAT == 0 {
        return Err(format!("invalid int {text:?} for injection tag \"{name}\""));
    }
    number
        .float
        .map(Value::Float)
        .ok_or_else(|| format!("invalid number for injection tag \"{name}\": {text:?}"))
}

/// Parse an environment value using the type constraints of the task field
/// that requests it, mirroring CUE's `internal/cli.ParseValue`.
fn env_parse(
    name: &str,
    text: &str,
    constraint: &Value,
) -> Result<Value, ostinato_lang::LangError> {
    let kinds = env_value_kinds(constraint);
    if kinds & ENV_OTHER != 0 || kinds == 0 {
        return Err(ostinato_lang::LangError::Workflow(format!(
            "invalid type for injection tag \"{name}\""
        )));
    }
    let mut parts = Vec::new();
    let mut defaults = Vec::new();
    let mut single = None;
    let mut first_error: Option<String> = None;

    if kinds & ENV_NUMBER != 0 {
        match parse_env_number(text) {
            Ok(number) => match env_number_value(name, text, number, kinds) {
                Ok(value) => {
                    if kinds & (ENV_BOOL | ENV_STRING) != 0 {
                        parts.push(value);
                    } else {
                        single = Some(value);
                    }
                }
                Err(err) => first_error = Some(err),
            },
            Err(err) => {
                first_error = Some(format!(
                    "invalid number for injection tag \"{name}\": {err}"
                ))
            }
        }
    }

    if kinds & ENV_BOOL != 0 {
        match bool_env_value(text) {
            Some(value) => {
                if parts.is_empty() && kinds & ENV_STRING == 0 {
                    single = Some(Value::Bool(value));
                } else {
                    parts.push(Value::Bool(value));
                }
            }
            None => {
                let err = format!("invalid boolean {text:?} for injection tag \"{name}\"");
                first_error.get_or_insert(err);
            }
        }
    }

    if kinds & ENV_STRING != 0 {
        // CUE trims the value when a boolean alternative is present, and the
        // package schema's `*string` makes the string alternative the default
        // of a mixed disjunction.
        let string_value = if kinds & ENV_BOOL != 0 {
            text.trim().to_owned()
        } else {
            text.to_owned()
        };
        if parts.is_empty() {
            single = Some(Value::String(string_value));
        } else {
            parts.push(Value::String(string_value));
            defaults.push(parts.len() - 1);
        }
    }

    if !parts.is_empty() {
        if parts.len() == 1 {
            return Ok(parts.remove(0));
        }
        return Ok(Value::Disjunction(parts, defaults));
    }
    if let Some(value) = single {
        return Ok(value);
    }
    if let Some(error) = first_error {
        return Err(ostinato_lang::LangError::Workflow(error));
    }
    Err(ostinato_lang::LangError::Workflow(format!(
        "invalid type for injection tag \"{name}\""
    )))
}

#[derive(Debug)]
enum HttpInputError {
    NotReady(&'static str),
    Invalid(String),
}

fn http_string_field(
    task: &BTreeMap<String, Value>,
    key: &str,
) -> Result<Option<String>, HttpInputError> {
    match task.get(key).map(Value::concretize) {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(Value::Type(_)) | Some(Value::Bottom) => Err(HttpInputError::NotReady(
            "invalid string argument: non-concrete value string",
        )),
        Some(other) => Err(HttpInputError::Invalid(format!(
            "invalid string argument: found {}",
            other.type_name()
        ))),
    }
}

fn http_headers(value: &Value, what: &str) -> Result<Vec<(String, String)>, HttpInputError> {
    let fields = match value {
        Value::Struct(fields) | Value::Definition { fields, .. } => fields,
        Value::Type(_) | Value::Bottom => {
            return Err(HttpInputError::NotReady(
                "invalid input: non-concrete value",
            ));
        }
        other => {
            return Err(HttpInputError::Invalid(format!(
                "tool/http `{what}` must be a struct, found {}",
                other.type_name()
            )));
        }
    };
    let mut headers = Vec::new();
    for (name, value) in fields {
        match value.concretize() {
            Value::String(text) => headers.push((name.clone(), text.clone())),
            Value::List(items) => {
                for item in items {
                    match item.concretize() {
                        Value::String(text) => headers.push((name.clone(), text.clone())),
                        Value::Type(_) | Value::Bottom => {
                            return Err(HttpInputError::NotReady(
                                "invalid input: non-concrete value",
                            ));
                        }
                        other => {
                            return Err(HttpInputError::Invalid(format!(
                                "tool/http `{what}` values must be strings, found {}",
                                other.type_name()
                            )));
                        }
                    }
                }
            }
            Value::Type(_) | Value::Bottom => {
                return Err(HttpInputError::NotReady(
                    "invalid input: non-concrete value",
                ));
            }
            other => {
                return Err(HttpInputError::Invalid(format!(
                    "tool/http `{what}` values must be strings or lists of strings, found {}",
                    other.type_name()
                )));
            }
        }
    }
    Ok(headers)
}

/// Decode bytes the way Go's `strings.ToValidUTF8(s, "\uFFFD")` does: each
/// maximal run of invalid UTF-8 bytes becomes a single replacement character.
///
/// CUE stores `tool/http` response bodies as Go strings, so this matches its
/// observable `len`/interpolation behaviour for non-UTF-8 payloads.
fn utf8_lossy_go(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                out.push_str(text);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if let Ok(text) = std::str::from_utf8(&rest[..valid]) {
                    out.push_str(text);
                }
                out.push('\u{FFFD}');
                let mut index = valid + 1;
                while index < rest.len() {
                    match std::str::from_utf8(&rest[index..]) {
                        Ok(_) => break,
                        Err(next) if next.valid_up_to() > 0 => break,
                        Err(_) => index += 1,
                    }
                }
                rest = &rest[index..];
            }
        }
    }
    out
}

fn http_canonical_header(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = true;
    for ch in name.chars() {
        if ch == '-' {
            out.push(ch);
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

fn execute_http(
    task: &BTreeMap<String, Value>,
) -> Result<ToolTaskOutcome, ostinato_lang::LangError> {
    let result = (|| -> Result<ToolTaskOutcome, HttpInputError> {
        let method = match http_string_field(task, "method")? {
            Some(method) if !method.is_empty() => method,
            // Go's http.NewRequest treats an empty method as GET.
            Some(_) | None => "GET".to_owned(),
        };
        let Some(url) = http_string_field(task, "url")? else {
            return Err(HttpInputError::NotReady(
                "invalid string argument: could not find field \"url\"",
            ));
        };
        let follow_redirects = match task.get("followRedirects").map(Value::concretize) {
            None => true,
            Some(Value::Bool(value)) => *value,
            Some(Value::Type(_)) | Some(Value::Bottom) => {
                return Err(HttpInputError::NotReady(
                    "invalid bool value: non-concrete value bool",
                ));
            }
            Some(other) => {
                return Err(HttpInputError::Invalid(format!(
                    "invalid bool value: found {}",
                    other.type_name()
                )));
            }
        };
        let mut verify = true;
        let mut ca_cert = None;
        if let Some(tls) = task.get("tls") {
            match tls.concretize() {
                Value::Struct(fields) | Value::Definition { fields, .. } => {
                    if let Some(value) = fields.get("verify") {
                        match value.concretize() {
                            Value::Bool(value) => verify = *value,
                            Value::Type(_) | Value::Bottom => {
                                return Err(HttpInputError::NotReady(
                                    "invalid bool value: non-concrete value bool",
                                ));
                            }
                            other => {
                                return Err(HttpInputError::Invalid(format!(
                                    "invalid bool value: found {}",
                                    other.type_name()
                                )));
                            }
                        }
                    }
                    if let Some(value) = fields.get("caCert") {
                        match value.concretize() {
                            Value::String(text) => ca_cert = Some(text.clone().into_bytes()),
                            Value::Bytes(bytes) => ca_cert = Some(bytes.clone()),
                            Value::Type(_) | Value::Bottom => {
                                return Err(HttpInputError::NotReady(
                                    "invalid bytes value: non-concrete value",
                                ));
                            }
                            other => {
                                return Err(HttpInputError::Invalid(format!(
                                    "invalid bytes value: found {}",
                                    other.type_name()
                                )));
                            }
                        }
                    }
                }
                Value::Type(_) | Value::Bottom => {
                    return Err(HttpInputError::NotReady(
                        "invalid input: non-concrete value",
                    ));
                }
                other => {
                    return Err(HttpInputError::Invalid(format!(
                        "tool/http `tls` must be a struct, found {}",
                        other.type_name()
                    )));
                }
            }
        }

        let mut body = None;
        let mut headers = Vec::new();
        let mut trailers = Vec::new();
        if let Some(request) = task.get("request") {
            match request.concretize() {
                Value::Struct(fields) | Value::Definition { fields, .. } => {
                    if let Some(value) = fields.get("body") {
                        match value.concretize() {
                            Value::String(text) => body = Some(text.clone().into_bytes()),
                            Value::Bytes(bytes) => body = Some(bytes.clone()),
                            Value::Type(_) | Value::Bottom => {
                                return Err(HttpInputError::NotReady(
                                    "invalid input: non-concrete value",
                                ));
                            }
                            other => {
                                return Err(HttpInputError::Invalid(format!(
                                    "invalid input: found {}",
                                    other.type_name()
                                )));
                            }
                        }
                    }
                    if let Some(value) = fields.get("header") {
                        headers = http_headers(value, "request.header")?;
                    }
                    if let Some(value) = fields.get("trailer") {
                        trailers = http_headers(value, "request.trailer")?;
                    }
                }
                Value::Type(_) | Value::Bottom => {
                    return Err(HttpInputError::NotReady(
                        "invalid input: non-concrete value",
                    ));
                }
                other => {
                    return Err(HttpInputError::Invalid(format!(
                        "tool/http `request` must be a struct, found {}",
                        other.type_name()
                    )));
                }
            }
        }
        if !trailers.is_empty() {
            return Err(HttpInputError::Invalid(
                "tool/http request trailers are not supported".to_owned(),
            ));
        }

        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|err| HttpInputError::Invalid(format!("invalid method: {err}")))?;
        let mut builder = reqwest::blocking::Client::builder().redirect(if follow_redirects {
            reqwest::redirect::Policy::limited(10)
        } else {
            reqwest::redirect::Policy::none()
        });
        if !verify {
            builder = builder.danger_accept_invalid_certs(true);
        }
        if let Some(ca_cert) = ca_cert {
            let certificate = reqwest::Certificate::from_pem(&ca_cert)
                .map_err(|err| HttpInputError::Invalid(format!("failed to parse caCert: {err}")))?;
            builder = builder.add_root_certificate(certificate);
        }
        let client = builder
            .build()
            .map_err(|err| HttpInputError::Invalid(format!("cannot build HTTP client: {err}")))?;
        let mut request = client.request(method, &url);
        if let Some(body) = body {
            request = request.body(body);
        }
        let mut header_map = reqwest::header::HeaderMap::new();
        for (name, value) in headers {
            let canonical = http_canonical_header(&name);
            let name = reqwest::header::HeaderName::from_bytes(canonical.as_bytes())
                .map_err(|err| HttpInputError::Invalid(format!("invalid header name: {err}")))?;
            let value = reqwest::header::HeaderValue::from_str(&value)
                .map_err(|err| HttpInputError::Invalid(format!("invalid header value: {err}")))?;
            header_map.append(name, value);
        }
        request = request.headers(header_map);

        let response = match request.send() {
            Ok(response) => response,
            Err(err) => {
                eprintln!("task failed: {err}");
                return Ok(ToolTaskOutcome::Failed);
            }
        };
        let status_code = i64::from(response.status().as_u16());
        let status = match response.status().canonical_reason() {
            Some(reason) => format!("{status_code} {reason}"),
            None => status_code.to_string(),
        };
        let response_headers = response.headers().clone();
        let bytes = match response.bytes() {
            Ok(bytes) => bytes,
            Err(err) => {
                eprintln!("task failed: {err}");
                return Ok(ToolTaskOutcome::Failed);
            }
        };
        let mut header_fields: BTreeMap<String, Value> = BTreeMap::new();
        for (name, value) in response_headers.iter() {
            let text = value
                .to_str()
                .map_or_else(|_| utf8_lossy_go(value.as_bytes()), str::to_owned);
            let entry = header_fields
                .entry(http_canonical_header(name.as_str()))
                .or_insert_with(|| Value::List(Vec::new()));
            if let Value::List(items) = entry {
                items.push(Value::String(text));
            }
        }
        let response_value = Value::Struct(BTreeMap::from([
            ("status".to_owned(), Value::String(status)),
            ("statusCode".to_owned(), Value::Int(status_code)),
            ("body".to_owned(), Value::String(utf8_lossy_go(&bytes))),
            ("header".to_owned(), Value::Struct(header_fields)),
            ("trailer".to_owned(), Value::Struct(BTreeMap::new())),
        ]));
        Ok(ToolTaskOutcome::Done {
            outputs: vec![("response".to_owned(), response_value)],
        })
    })();
    match result {
        Ok(outcome) => Ok(outcome),
        Err(HttpInputError::NotReady(reason)) => Ok(ToolTaskOutcome::NotReady(reason)),
        Err(HttpInputError::Invalid(message)) => Err(ostinato_lang::LangError::Workflow(message)),
    }
}

fn execute_print(
    task: &BTreeMap<String, Value>,
) -> Result<ToolTaskOutcome, ostinato_lang::LangError> {
    let text = match task.get("text").map(Value::concretize) {
        Some(Value::String(text)) => text,
        Some(Value::Bytes(_)) => {
            return Err(ostinato_lang::LangError::Workflow(
                "invalid string argument: non-concrete value".to_owned(),
            ));
        }
        Some(Value::Type(_)) | Some(Value::Bottom) | None => {
            return Ok(ToolTaskOutcome::NotReady(
                "invalid string argument: non-concrete value string",
            ));
        }
        Some(other) => {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "tool/cli.Print `text` must be a string, found {}",
                other.type_name()
            )));
        }
    };
    let mut stdout = std::io::stdout();
    stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.write_all(b"\n"))
        .and_then(|()| stdout.flush())
        .map_err(|err| {
            ostinato_lang::LangError::Workflow(format!("tool/cli.Print failed: {err}"))
        })?;
    Ok(ToolTaskOutcome::Done {
        outputs: Vec::new(),
    })
}

fn execute_file(
    task: &BTreeMap<String, Value>,
    package_dir: &Path,
    truncate: bool,
) -> Result<ToolTaskOutcome, ostinato_lang::LangError> {
    let filename = match task.get("filename").map(Value::concretize) {
        Some(Value::String(filename)) => filename.clone(),
        Some(Value::Type(_)) | Some(Value::Bottom) | None => {
            return Ok(ToolTaskOutcome::NotReady(
                "tool/file task inputs are not concrete",
            ));
        }
        Some(other) => {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "tool/file task `filename` must be a string, found {}",
                other.type_name()
            )));
        }
    };
    let contents = match task.get("contents").map(Value::concretize) {
        Some(Value::String(text)) => text.clone().into_bytes(),
        Some(Value::Bytes(bytes)) => bytes.clone(),
        Some(Value::Type(_)) | Some(Value::Bottom) | None => {
            return Ok(ToolTaskOutcome::NotReady(
                "tool/file task inputs are not concrete",
            ));
        }
        Some(other) => {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "tool/file task `contents` must be a string or bytes, found {}",
                other.type_name()
            )));
        }
    };
    let path = Path::new(&filename);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        package_dir.join(path)
    };
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true);
    if truncate {
        options.truncate(true);
    } else {
        options.append(true);
    }
    let mut file = match options.open(&path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("task failed: open {filename}: {}", io_detail(&err));
            return Ok(ToolTaskOutcome::Failed);
        }
    };
    if let Err(err) = file.write_all(&contents) {
        eprintln!("task failed: write {filename}: {}", io_detail(&err));
        return Ok(ToolTaskOutcome::Failed);
    }
    Ok(ToolTaskOutcome::Done {
        outputs: Vec::new(),
    })
}

fn execute_exec(
    task: &BTreeMap<String, Value>,
    package_dir: &Path,
) -> Result<ToolTaskOutcome, ostinato_lang::LangError> {
    let Some(cmd) = task.get("cmd") else {
        return Err(ostinato_lang::LangError::Workflow(
            "tool/exec.Run task is missing `cmd`".to_owned(),
        ));
    };
    let argv = match cmd {
        Value::String(text) => text
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<String>>(),
        Value::List(items) => {
            let mut argv = Vec::with_capacity(items.len());
            for item in items {
                match item.concretize() {
                    Value::String(text) => argv.push(text.clone()),
                    Value::Type(_) | Value::Bottom | Value::Disjunction(..) => {
                        return Ok(ToolTaskOutcome::NotReady(
                            "tool/exec.Run `cmd` is not concrete",
                        ));
                    }
                    other => {
                        return Err(ostinato_lang::LangError::Workflow(format!(
                            "tool/exec.Run `cmd` element must be a string, found {}",
                            other.type_name()
                        )));
                    }
                }
            }
            argv
        }
        Value::Type(_) | Value::Bottom | Value::Disjunction(..) => {
            return Ok(ToolTaskOutcome::NotReady(
                "tool/exec.Run `cmd` is not concrete",
            ));
        }
        other => {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "tool/exec.Run `cmd` must be a string or list of strings, found {}",
                other.type_name()
            )));
        }
    };
    if argv.is_empty() {
        return Err(ostinato_lang::LangError::Workflow(
            "tool/exec.Run `cmd` must not be empty".to_owned(),
        ));
    }

    let mut process = Command::new(&argv[0]);
    process.args(&argv[1..]);
    process.current_dir(package_dir);
    let mut resolved_dir = None;
    let mut raw_dir = None;
    match task.get("dir").map(Value::concretize) {
        None => {}
        Some(Value::String(dir)) => {
            if !dir.is_empty() {
                let path = Path::new(dir);
                let resolved = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    package_dir.join(path)
                };
                process.current_dir(&resolved);
                raw_dir = Some(dir.clone());
                resolved_dir = Some(resolved);
            }
        }
        Some(Value::Type(_)) | Some(Value::Bottom) => {
            return Ok(ToolTaskOutcome::NotReady(
                "tool/exec.Run `dir` is not concrete",
            ));
        }
        Some(other) => {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "tool/exec.Run `dir` must be a string, found {}",
                other.type_name()
            )));
        }
    }
    match task.get("env").map(Value::concretize) {
        None => {}
        Some(Value::Struct(env)) if env.is_empty() => {}
        Some(Value::Struct(env)) | Some(Value::Definition { fields: env, .. }) => {
            process.env_clear();
            for (key, value) in env {
                match value.concretize() {
                    Value::String(value) => {
                        process.env(key, value);
                    }
                    Value::Type(_) | Value::Bottom => {
                        return Ok(ToolTaskOutcome::NotReady(
                            "tool/exec.Run `env` is not concrete",
                        ));
                    }
                    other => {
                        return Err(ostinato_lang::LangError::Workflow(format!(
                            "tool/exec.Run `env` values must be strings, found {}",
                            other.type_name()
                        )));
                    }
                }
            }
        }
        Some(Value::List(entries)) => {
            if !entries.is_empty() {
                process.env_clear();
            }
            for entry in entries {
                match entry.concretize() {
                    Value::String(entry) => {
                        let Some((key, value)) = entry.split_once('=') else {
                            return Err(ostinato_lang::LangError::Workflow(
                                "tool/exec.Run `env` list entries must be `NAME=value` strings"
                                    .to_owned(),
                            ));
                        };
                        process.env(key, value);
                    }
                    Value::Type(_) | Value::Bottom | Value::Disjunction(..) => {
                        return Ok(ToolTaskOutcome::NotReady(
                            "tool/exec.Run `env` is not concrete",
                        ));
                    }
                    other => {
                        return Err(ostinato_lang::LangError::Workflow(format!(
                            "tool/exec.Run `env` list entries must be `NAME=value` strings, found {}",
                            other.type_name()
                        )));
                    }
                }
            }
        }
        Some(Value::Type(_)) | Some(Value::Bottom) => {
            return Ok(ToolTaskOutcome::NotReady(
                "tool/exec.Run `env` is not concrete",
            ));
        }
        Some(other) => {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "tool/exec.Run `env` must be a struct or list, found {}",
                other.type_name()
            )));
        }
    }
    let stdin = match task.get("stdin") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone().into_bytes()),
        Some(Value::Bytes(bytes)) => Some(bytes.clone()),
        Some(Value::Disjunction(alternatives, defaults)) => {
            let default = (defaults.len() == 1)
                .then(|| defaults[0])
                .and_then(|index| alternatives.get(index))
                .cloned();
            let has_null = alternatives
                .iter()
                .any(|value| matches!(value, Value::Null));
            match default {
                Some(Value::Null) => None,
                Some(Value::String(text)) if !has_null => Some(text.into_bytes()),
                Some(Value::Bytes(bytes)) if !has_null => Some(bytes),
                Some(Value::Type(_)) | Some(Value::Bottom) | None
                    if alternatives
                        .iter()
                        .any(|value| matches!(value, Value::Type(_) | Value::Bottom)) =>
                {
                    return Ok(ToolTaskOutcome::NotReady(
                        "invalid input: non-concrete value",
                    ));
                }
                _ => {
                    return Err(ostinato_lang::LangError::Workflow(
                        "invalid input: non-concrete value".to_owned(),
                    ));
                }
            }
        }
        Some(Value::Type(_)) | Some(Value::Bottom) => {
            return Ok(ToolTaskOutcome::NotReady(
                "invalid input: non-concrete value",
            ));
        }
        Some(other) => {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "tool/exec.Run `stdin` must be a string or bytes, found {}",
                other.type_name()
            )));
        }
    };

    let capture_stdout = capture_kind(task, "stdout")?;
    let capture_stderr = capture_kind(task, "stderr")?;
    if capture_stdout == CaptureKind::Other {
        return Err(ostinato_lang::LangError::Workflow(
            "invalid stdout: conflicting values bytes and string".to_owned(),
        ));
    }
    if capture_stderr == CaptureKind::Other {
        return Err(ostinato_lang::LangError::Workflow(
            "invalid stderr: conflicting values bytes and string".to_owned(),
        ));
    }
    let capture_stdout = capture_stdout == CaptureKind::String;
    let capture_stderr = capture_stderr == CaptureKind::String;
    process.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::inherit()
    });
    process.stdout(if capture_stdout {
        Stdio::piped()
    } else {
        Stdio::inherit()
    });
    process.stderr(if capture_stderr {
        Stdio::piped()
    } else {
        Stdio::inherit()
    });
    let must_succeed = match task.get("mustSucceed") {
        None => true,
        Some(Value::Bool(value)) => *value,
        Some(Value::Type(_)) | Some(Value::Bottom) => {
            return Ok(ToolTaskOutcome::NotReady(
                "invalid mustSucceed value: non-concrete value",
            ));
        }
        Some(Value::Disjunction(alternatives, defaults)) => {
            let default = (defaults.len() == 1)
                .then(|| defaults[0])
                .and_then(|index| alternatives.get(index));
            match default {
                Some(Value::Bool(true)) => true,
                _ => {
                    return Err(ostinato_lang::LangError::Workflow(
                        "invalid mustSucceed value: non-concrete value".to_owned(),
                    ));
                }
            }
        }
        Some(other) => {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "tool/exec.Run `mustSucceed` must be a bool, found {}",
                other.type_name()
            )));
        }
    };
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(err) => {
            let printable = argv
                .iter()
                .map(|arg| format!("{arg:?}"))
                .collect::<Vec<_>>()
                .join(" ");
            let executable = resolve_executable(&argv[0]);
            let detail = if executable.is_none() && !argv[0].contains('/') {
                format!("exec: \"{}\": executable file not found in $PATH", argv[0])
            } else if let Some(dir) = &resolved_dir {
                let raw = raw_dir.as_deref().unwrap_or_default();
                if !dir.exists() {
                    format!("chdir {raw}: no such file or directory")
                } else if !dir.is_dir() {
                    format!(
                        "fork/exec {}: not a directory",
                        executable
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| argv[0].clone())
                    )
                } else {
                    spawn_detail(&err, &argv[0])
                }
            } else {
                spawn_detail(&err, &argv[0])
            };
            if !must_succeed {
                // CUE reports `success: false` and, when stderr is captured,
                // the spawn error text; stdout, when captured, stays empty.
                let mut outputs = Vec::new();
                if capture_stdout {
                    outputs.push(("stdout".to_owned(), Value::String(String::new())));
                }
                if capture_stderr {
                    outputs.push(("stderr".to_owned(), Value::String(detail)));
                }
                outputs.push(("success".to_owned(), Value::Bool(false)));
                return Ok(ToolTaskOutcome::Done { outputs });
            }
            eprintln!("task failed: command [{printable}] failed: {detail}");
            return Ok(ToolTaskOutcome::Failed);
        }
    };
    let writer = stdin.map(|bytes| {
        let mut pipe = child.stdin.take();
        std::thread::spawn(move || {
            if let Some(pipe) = pipe.as_mut() {
                let _ = pipe.write_all(&bytes);
            }
        })
    });
    let (status, output) = if capture_stdout || capture_stderr {
        let output = child.wait_with_output().map_err(|err| {
            ostinato_lang::LangError::Workflow(format!("tool/exec.Run wait failed: {err}"))
        })?;
        (output.status, Some(output))
    } else {
        let status = child.wait().map_err(|err| {
            ostinato_lang::LangError::Workflow(format!("tool/exec.Run wait failed: {err}"))
        })?;
        (status, None)
    };
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    if !status.success() && must_succeed {
        let printable = argv
            .iter()
            .map(|arg| format!("{arg:?}"))
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!(
            "task failed: command [{printable}] failed: {}",
            exec_failure(&status)
        );
        return Ok(ToolTaskOutcome::Failed);
    }
    let mut outputs = Vec::new();
    if let Some(output) = &output
        && capture_stdout
    {
        outputs.push((
            "stdout".to_owned(),
            Value::String(String::from_utf8_lossy(&output.stdout).into_owned()),
        ));
    }
    if capture_stderr && !status.success() {
        // CUE only injects stderr when the command fails. When stdout is not
        // captured it runs `cmd.Run()`, whose `ExitError` does not carry
        // stderr, so the injected value is empty.
        let stderr = if capture_stdout {
            output.as_ref().map_or_else(String::new, |output| {
                String::from_utf8_lossy(&output.stderr).into_owned()
            })
        } else {
            String::new()
        };
        outputs.push(("stderr".to_owned(), Value::String(stderr)));
    }
    outputs.push(("success".to_owned(), Value::Bool(status.success())));
    Ok(ToolTaskOutcome::Done { outputs })
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
