use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use ostinato_application::{LoadRequest, LoadedWorkflow, PortError, PortResult, SpecLoader};
use ostinato_lang::ast::{Expr, Field, Label, Span, StructItem};
use ostinato_lang::{
    Value, builtin_module, compile_json, compile_value, eval_file_with_imports_mode,
    eval_file_with_imports_mode_allow_incomplete, eval_file_with_imports_mode_allow_outputs, parse,
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
        let root = evaluate_path(
            &absolute,
            &request.params,
            &mut cache,
            &mut stack,
            false,
            false,
        )
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
    allow_incomplete: bool,
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

    let files = collect_package_files(&canonical_path, false)?;
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
        if let Some(module) = tool_module(&import.path) {
            imports.insert(import.alias.clone(), module);
            continue;
        }
        if let Some(module) = builtin_module(&import.path) {
            imports.insert(import.alias.clone(), module);
            continue;
        }
        let imported_path = resolve_import(&source_dir, &import.path)?;
        let value = evaluate_path(&imported_path, &BTreeMap::new(), cache, stack, true, true)?;
        imports.insert(import.alias.clone(), value);
    }

    ostinato_lang::assign_field_ids(&mut combined);
    let tags = ostinato_lang::parameter_tags(&combined)?;
    let mut injected = BTreeMap::new();
    for (key, raw) in overrides {
        if let Some(infos) = tags.get(key) {
            for info in infos {
                let value = parse_override_value(raw, Some(info.ty))?;
                let expr = override_expr(value)?;
                if !inject_field(&mut combined.items, info.field_id, &expr) {
                    return Err(ostinato_lang::LangError::Workflow(format!(
                        "cannot inject @tag `{key}`: declaration not found"
                    )));
                }
            }
        } else {
            injected.insert(key.clone(), parse_override_value(raw, None)?);
        }
    }
    let value = if allow_incomplete {
        eval_file_with_imports_mode_allow_incomplete(
            &combined,
            &imports,
            &injected,
            include_definitions,
        )?
    } else {
        eval_file_with_imports_mode(&combined, &imports, &injected, include_definitions)?
    };
    let value = if include_definitions {
        ostinato_lang::strip_hidden(value)
    } else {
        value
    };
    cache.insert(key, value.clone());
    stack.pop();
    Ok(value)
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

fn collect_parameter_tags(
    path: &Path,
) -> Result<BTreeMap<String, Vec<ostinato_lang::TagInfo>>, ostinato_lang::LangError> {
    let files = collect_package_files(path, false)?;
    let mut combined = ostinato_lang::ast::File {
        package: None,
        imports: Vec::new(),
        items: Vec::new(),
    };
    for (index, file) in files.iter().enumerate() {
        let text = std::fs::read_to_string(file).map_err(|err| {
            ostinato_lang::LangError::Workflow(format!("cannot read {}: {err}", file.display()))
        })?;
        let mut parsed = ostinato_lang::parse(&text)?;
        ostinato_lang::rename_file_locals(&mut parsed, index)?;
        if combined.package.is_none() {
            combined.package = parsed.package.clone();
        }
        combined.imports.extend(parsed.imports);
        combined.items.extend(parsed.items);
    }
    ostinato_lang::parameter_tags(&combined)
}

fn parameters_json(
    root: &Value,
    overrides: &BTreeMap<String, String>,
    tags: &BTreeMap<String, Vec<ostinato_lang::TagInfo>>,
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
            let known = tags.contains_key(key);
            if !known && !params.contains_key(key) {
                return Err(ostinato_lang::LangError::Workflow(format!(
                    "unknown parameter `{key}`; declare it under `params`"
                )));
            }
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
        let agents = match self.export(&directory, &instance, "agents", &request.params) {
            Ok(agents) => agents,
            Err(PortError::Loader(message))
                if message.contains("reference \"agents\" not found") =>
            {
                json!({})
            }
            Err(err) => return Err(err),
        };
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

/// Run a CUE tool command declared in `*_tool.cue` files.
///
/// Implements the `command: <name>: { <task>: tool/exec.Run & {...} }` surface,
/// including `@tag` injection, `$after` ordering, `cmd` string/argv, `env`,
/// `dir`, `stdin`, `stdout`/`stderr` capture fields and `mustSucceed`. Tasks
/// that consume other task outputs and other `tool/...` packages are documented
/// limits (see `docs/SCOPE.md`).
///
/// # Errors
/// Returns a language or workflow error for unreadable packages, unknown
/// commands, malformed tasks or unsupported task kinds.
pub fn run_tool_command(
    path: &Path,
    command_name: &str,
    overrides: &BTreeMap<String, String>,
) -> Result<i32, ostinato_lang::LangError> {
    let canonical_path = canonical(path);
    let files = collect_package_files(&canonical_path, true)?;
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
            &mut cache,
            &mut stack,
            true,
            true,
        )?;
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
    ostinato_lang::assign_field_ids(&mut combined);
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
    {
        let mut scopes = Vec::new();
        collect_output_task_paths(
            &Expr::Struct(task_items.clone(), Span::default()),
            &mut scopes,
            &combined.items,
            &mut external_paths,
            &mut std::collections::BTreeSet::new(),
            &non_task_imports,
        );
    }
    external_paths.retain(|path| {
        let Some(leaf) = path.last() else {
            return false;
        };
        if probe_scheduled
            .iter()
            .any(|(scheduled_name, _, _)| scheduled_name == leaf)
        {
            return false;
        }
        if path.len() == 1 && root_bare_module_task(&combined.items, leaf, &non_task_imports) {
            return false;
        }
        find_field_expr_at_path(&combined.items, path).is_some_and(|value| {
            is_task_expr(
                value,
                &[combined.items.as_slice()],
                &combined.items,
                &mut std::collections::BTreeSet::new(),
                &non_task_imports,
            )
        })
    });
    // A command task that aliases a package task (`r: prep`) makes direct
    // package-root reads of that task non-concrete in CUE.
    let command_claimed: std::collections::BTreeSet<Vec<String>> = {
        let command_scopes: Vec<&[StructItem]> = vec![task_items.as_slice()];
        let mut claimed = std::collections::BTreeSet::new();
        collect_command_task_package_paths(
            &Expr::Struct(task_items.clone(), Span::default()),
            &command_scopes,
            &combined.items,
            &non_task_imports,
            &mut claimed,
        );
        claimed
    };
    {
        let order = package_read_order(
            task_items.as_slice(),
            &combined.items,
            &non_task_imports,
            &command_claimed,
        );
        external_paths.retain(|path| {
            if !command_claimed.contains(path) {
                return true;
            }
            match order.get(path) {
                Some((Some(declaration), Some(read))) => read <= declaration,
                _ => true,
            }
        });
    }

    // Package-level tasks reachable from command `$after` operands or package
    // output reads (top-level fields, definitions, nested containers).
    let mut package_tasks: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();
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
        .chain(package_tasks.iter().map(|(_, path, _)| path))
    {
        if let Some(leaf) = path.last() {
            task_name_set.insert(leaf.clone());
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
                    let injected_ok = if let Some(path) = external_by_key.get(name) {
                        inject_named_outputs(
                            &mut combined.items,
                            command_name,
                            name,
                            path,
                            &outputs,
                            &non_task_imports,
                        )
                    } else {
                        inject_command_outputs(
                            &mut combined.items,
                            command_name,
                            name,
                            &outputs,
                            &non_task_imports,
                        )
                    };
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
        _ => return None,
    }
    Some(Value::Struct(exports))
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

/// Definition-rooted paths of task values whose outputs the command reads.
fn collect_output_task_paths<'a>(
    expr: &'a Expr,
    scopes: &mut Vec<&'a [StructItem]>,
    root: &'a [StructItem],
    out: &mut std::collections::BTreeSet<Vec<String>>,
    visited: &mut std::collections::BTreeSet<String>,
    imports: &ImportKinds,
) {
    match expr {
        Expr::Select(base, field, _, _) => {
            if matches!(field.as_str(), "stdout" | "success")
                && let Some(path) = resolved_read_path(
                    base,
                    scopes,
                    root,
                    &mut std::collections::BTreeSet::new(),
                    imports,
                )
            {
                out.insert(path);
            }
            collect_output_task_paths(base, scopes, root, out, visited, imports);
        }
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return;
            }
            if let Some(item) = local_binding(scopes, root, name) {
                match item {
                    StructItem::Let { value, .. } => {
                        collect_output_task_paths(value, scopes, root, out, visited, imports);
                    }
                    StructItem::Field(field) if !field.definition => {
                        collect_output_task_paths(
                            &field.value,
                            scopes,
                            root,
                            out,
                            visited,
                            imports,
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
                        collect_output_task_paths(
                            &field.value,
                            scopes,
                            root,
                            out,
                            visited,
                            imports,
                        );
                    }
                    StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                        collect_output_task_paths(value, scopes, root, out, visited, imports);
                    }
                    StructItem::For { source, body, .. } => {
                        collect_output_task_paths(source, scopes, root, out, visited, imports);
                        for body_item in body {
                            if let StructItem::Field(field) = body_item {
                                collect_output_task_paths(
                                    &field.value,
                                    scopes,
                                    root,
                                    out,
                                    visited,
                                    imports,
                                );
                            }
                        }
                    }
                    StructItem::If { cond, body, .. } => {
                        collect_output_task_paths(cond, scopes, root, out, visited, imports);
                        for body_item in body {
                            if let StructItem::Field(field) = body_item {
                                collect_output_task_paths(
                                    &field.value,
                                    scopes,
                                    root,
                                    out,
                                    visited,
                                    imports,
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
            collect_output_task_paths(left, scopes, root, out, visited, imports);
            collect_output_task_paths(right, scopes, root, out, visited, imports);
        }
        Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Bound(_, inner, _)
        | Expr::Unary(_, inner, _) => {
            collect_output_task_paths(inner, scopes, root, out, visited, imports)
        }
        Expr::Index(base, index, _) => {
            collect_output_task_paths(base, scopes, root, out, visited, imports);
            collect_output_task_paths(index, scopes, root, out, visited, imports);
        }
        Expr::Call(callee, arguments, _) => {
            collect_output_task_paths(callee, scopes, root, out, visited, imports);
            for argument in arguments {
                collect_output_task_paths(argument, scopes, root, out, visited, imports);
            }
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let ostinato_lang::ast::StrPart::Interp(inner) = part {
                    collect_output_task_paths(inner, scopes, root, out, visited, imports);
                }
            }
        }
        Expr::BytesInterp(parts, _) => {
            for part in parts {
                if let ostinato_lang::ast::BytePart::Interp(inner) = part {
                    collect_output_task_paths(inner, scopes, root, out, visited, imports);
                }
            }
        }
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                collect_output_task_paths(item, scopes, root, out, visited, imports);
            }
        }
        Expr::OpenList(Some(inner), _) => {
            collect_output_task_paths(inner, scopes, root, out, visited, imports)
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
        _ => None,
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

fn package_read_order(
    items: &[StructItem],
    root: &[StructItem],
    imports: &ImportKinds,
    claimed: &std::collections::BTreeSet<Vec<String>>,
) -> std::collections::BTreeMap<Vec<String>, (Option<usize>, Option<usize>)> {
    let scopes: [&[StructItem]; 1] = [items];
    let mut order: std::collections::BTreeMap<Vec<String>, (Option<usize>, Option<usize>)> =
        std::collections::BTreeMap::new();
    let mut counter = 0usize;
    for item in items {
        let StructItem::Field(field) = item else {
            continue;
        };
        let index = counter;
        counter += 1;
        scan_package_declarations(
            &field.value,
            index,
            &scopes,
            root,
            imports,
            claimed,
            &mut order,
        );
        scan_package_reads(
            &field.value,
            index,
            &scopes,
            root,
            imports,
            claimed,
            &mut order,
        );
    }
    order
}

fn scan_package_declarations(
    expr: &Expr,
    index: usize,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &ImportKinds,
    claimed: &std::collections::BTreeSet<Vec<String>>,
    order: &mut std::collections::BTreeMap<Vec<String>, (Option<usize>, Option<usize>)>,
) {
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
        let entry = order.entry(path).or_insert((None, None));
        entry.0 = Some(entry.0.map_or(index, |existing| existing.min(index)));
    }
    match expr {
        Expr::Struct(items, _) => {
            for item in items {
                let StructItem::Field(field) = item else {
                    continue;
                };
                if matches!(&field.label, Label::Name(name, _) if name.starts_with('$')) {
                    continue;
                }
                scan_package_declarations(
                    &field.value,
                    index,
                    scopes,
                    root,
                    imports,
                    claimed,
                    order,
                );
            }
        }
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            scan_package_declarations(left, index, scopes, root, imports, claimed, order);
            scan_package_declarations(right, index, scopes, root, imports, claimed, order);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            scan_package_declarations(inner, index, scopes, root, imports, claimed, order);
        }
        _ => {}
    }
}

fn scan_package_reads(
    expr: &Expr,
    index: usize,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &ImportKinds,
    claimed: &std::collections::BTreeSet<Vec<String>>,
    order: &mut std::collections::BTreeMap<Vec<String>, (Option<usize>, Option<usize>)>,
) {
    match expr {
        Expr::Select(base, field, _, _) => {
            if matches!(field.as_str(), "stdout" | "success")
                && let Some(path) = resolved_read_path(
                    base,
                    scopes,
                    root,
                    &mut std::collections::BTreeSet::new(),
                    imports,
                )
                && claimed.contains(&path)
            {
                let entry = order.entry(path).or_insert((None, None));
                entry.1 = Some(entry.1.map_or(index, |existing| existing.min(index)));
            }
            scan_package_reads(base, index, scopes, root, imports, claimed, order);
        }
        Expr::Struct(items, _) => {
            let mut nested = scopes.to_vec();
            nested.push(items);
            for item in items {
                match item {
                    StructItem::Field(field) => {
                        if field.definition
                            || field.optional
                            || matches!(&field.label, Label::Name(name, _) if !field.quoted && name.starts_with('_'))
                        {
                            continue;
                        }
                        scan_package_reads(
                            &field.value,
                            index,
                            &nested,
                            root,
                            imports,
                            claimed,
                            order,
                        );
                    }
                    StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                        scan_package_reads(value, index, &nested, root, imports, claimed, order);
                    }
                    StructItem::For { body, .. } | StructItem::If { body, .. } => {
                        for body_item in body {
                            if let StructItem::Field(field) = body_item {
                                scan_package_reads(
                                    &field.value,
                                    index,
                                    &nested,
                                    root,
                                    imports,
                                    claimed,
                                    order,
                                );
                            }
                        }
                    }
                    StructItem::Ellipsis(_) => {}
                }
            }
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            scan_package_reads(left, index, scopes, root, imports, claimed, order);
            scan_package_reads(right, index, scopes, root, imports, claimed, order);
        }
        Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Bound(_, inner, _)
        | Expr::Unary(_, inner, _) => {
            scan_package_reads(inner, index, scopes, root, imports, claimed, order);
        }
        Expr::Index(base, index_expr, _) => {
            scan_package_reads(base, index, scopes, root, imports, claimed, order);
            scan_package_reads(index_expr, index, scopes, root, imports, claimed, order);
        }
        Expr::Call(callee, arguments, _) => {
            scan_package_reads(callee, index, scopes, root, imports, claimed, order);
            for argument in arguments {
                scan_package_reads(argument, index, scopes, root, imports, claimed, order);
            }
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let ostinato_lang::ast::StrPart::Interp(inner) = part {
                    scan_package_reads(inner, index, scopes, root, imports, claimed, order);
                }
            }
        }
        Expr::BytesInterp(parts, _) => {
            for part in parts {
                if let ostinato_lang::ast::BytePart::Interp(inner) = part {
                    scan_package_reads(inner, index, scopes, root, imports, claimed, order);
                }
            }
        }
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                scan_package_reads(item, index, scopes, root, imports, claimed, order);
            }
        }
        Expr::OpenList(Some(inner), _) => {
            scan_package_reads(inner, index, scopes, root, imports, claimed, order);
        }
        _ => {}
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

/// Package paths that command task fields alias (`r: prep`, `r: group.prep`).
fn collect_command_task_package_paths(
    expr: &Expr,
    scopes: &[&[StructItem]],
    root: &[StructItem],
    imports: &ImportKinds,
    out: &mut std::collections::BTreeSet<Vec<String>>,
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
                    && let Some((path, true)) = resolve_after_path(
                        &field.value,
                        scopes,
                        root,
                        &mut std::collections::BTreeSet::new(),
                    )
                {
                    out.insert(path);
                }
                collect_command_task_package_paths(&field.value, scopes, root, imports, out);
            }
        }
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            collect_command_task_package_paths(left, scopes, root, imports, out);
            collect_command_task_package_paths(right, scopes, root, imports, out);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            collect_command_task_package_paths(inner, scopes, root, imports, out);
        }
        _ => {}
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
        _ => None,
    }
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
    let fields = match value {
        Value::Struct(fields) => fields,
        Value::Definition { fields, .. } => fields,
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
            if matches!(field.as_str(), "stdout" | "success") {
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
    matches!(value, Expr::Select(_, field, _, _) if matches!(field.as_str(), "stdout" | "success"))
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
            if matches!(field.as_str(), "stdout" | "success") {
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
            let mut nested = scopes.to_vec();
            nested.push(items);
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
        other => Err(ostinato_lang::LangError::Workflow(format!(
            "tool task `{other}` is not implemented"
        ))),
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
            if !must_succeed {
                return Ok(ToolTaskOutcome::Done {
                    outputs: Vec::new(),
                });
            }
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
