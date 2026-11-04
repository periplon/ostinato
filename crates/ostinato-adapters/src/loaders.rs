use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use ostinato_application::{LoadRequest, LoadedWorkflow, PortError, PortResult, SpecLoader};
use ostinato_lang::ast::{Expr, Label, Span, StructItem};
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
        if let Some(module) = builtin_module(&import.path) {
            imports.insert(import.alias.clone(), module);
            continue;
        }
        let imported_path = resolve_import(&source_dir, &import.path)?;
        let value = evaluate_path(&imported_path, &BTreeMap::new(), cache, stack, true)?;
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
    let value = eval_file_with_imports_mode(&combined, &imports, &injected, include_definitions)?;
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
        )?;
        imports.insert(import.alias.clone(), value);
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
    collect_schedule(&probe_tasks, Some(&task_items), &mut probe_scheduled)?;
    let task_name_set: std::collections::HashSet<String> = probe_scheduled
        .iter()
        .map(|(name, _, _)| name.clone())
        .collect();
    let value = eval_file_with_imports_mode_allow_outputs(
        &combined,
        &imports,
        &injected,
        false,
        &task_name_set,
    )?;
    let mut tasks = extract_command_tasks(&value, command_name)?;

    let mut scheduled = Vec::new();
    collect_schedule(&tasks, Some(&task_items), &mut scheduled)?;
    if scheduled.is_empty() {
        return Err(ostinato_lang::LangError::Workflow(format!(
            "tool command `{command_name}` has no tasks"
        )));
    }
    let mut task_names = std::collections::BTreeSet::new();
    for (name, _, _) in &scheduled {
        if !task_names.insert(name.clone()) {
            return Err(ostinato_lang::LangError::Workflow(format!(
                "ambiguous tool task name `{name}`"
            )));
        }
    }
    add_output_dependencies(&combined.items, &mut scheduled);
    if let Some(command_value) = command_value(&combined.items, command_name) {
        let all_names: std::collections::BTreeSet<String> =
            scheduled.iter().map(|(name, _, _)| name.clone()).collect();
        let mut operands = Vec::new();
        collect_let_selector_operands(&combined.items, command_value, &mut operands);
        for operand in operands {
            let mut resolved = Vec::new();
            resolve_command_items(
                &combined.items,
                operand,
                &mut std::collections::BTreeSet::new(),
                &mut resolved,
            );
            if scheduled
                .iter()
                .any(|(name, _, _)| task_has_output_refs(&resolved, name, &all_names))
            {
                return Err(ostinato_lang::LangError::Workflow(
                    "tool command selector cannot receive task outputs through a `let`".to_owned(),
                ));
            }
        }
    }
    for (name, dependencies, _) in &scheduled {
        for dependency in dependencies {
            if !task_names.contains(dependency) {
                return Err(ostinato_lang::LangError::Workflow(format!(
                    "task `{name}` has unknown `$after` task `{dependency}`"
                )));
            }
        }
    }

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
            match execute_tool_task(task, &source_dir)? {
                ToolTaskOutcome::NotReady(reason) => last_reason = Some(reason.to_owned()),
                ToolTaskOutcome::Done { outputs } => {
                    done.insert(name.clone());
                    progress = true;
                    if !outputs.is_empty()
                        && inject_command_outputs(&mut combined.items, command_name, name, &outputs)
                    {
                        let value = eval_file_with_imports_mode_allow_outputs(
                            &combined,
                            &imports,
                            &injected,
                            false,
                            &task_name_set,
                        )?;
                        tasks = extract_command_tasks(&value, command_name)?;
                        let mut rescheduled = Vec::new();
                        collect_schedule(&tasks, Some(&task_items), &mut rescheduled)?;
                        add_output_dependencies(&combined.items, &mut rescheduled);
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

fn is_let_name(items: &[StructItem], name: &str) -> bool {
    items
        .iter()
        .any(|item| matches!(item, StructItem::Let { name: let_name, .. } if let_name == name))
}

fn field_definition<'a>(items: &'a [StructItem], name: &str) -> Option<&'a Expr> {
    items.iter().find_map(|item| match item {
        StructItem::Field(field) => match &field.label {
            Label::Name(label, _) if label == name => Some(&field.value),
            _ => None,
        },
        _ => None,
    })
}

/// Whether a selector's root declaration is a file-scoped `let`.
fn selector_root_is_let(
    items: &[StructItem],
    expr: &Expr,
    visited: &mut std::collections::BTreeSet<String>,
) -> bool {
    match expr {
        Expr::Ident(name, _) => {
            if !visited.insert(name.clone()) {
                return false;
            }
            if is_let_name(items, name) {
                return true;
            }
            field_definition(items, name)
                .is_some_and(|value| selector_root_is_let(items, value, visited))
        }
        Expr::Select(base, _, _, _) | Expr::Index(base, _, _) => {
            selector_root_is_let(items, base, visited)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            selector_root_is_let(items, inner, visited)
        }
        _ => false,
    }
}

/// Whether a command value is a selector whose root is a `let`.
fn command_value_is_let_selector(
    items: &[StructItem],
    expr: &Expr,
    visited: &mut std::collections::BTreeSet<String>,
) -> bool {
    match expr {
        Expr::Select(base, _, _, _) | Expr::Index(base, _, _) => {
            selector_root_is_let(items, base, visited)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            command_value_is_let_selector(items, inner, visited)
        }
        Expr::Unify(left, right, _) => {
            command_value_is_let_selector(items, left, visited)
                || command_value_is_let_selector(items, right, visited)
        }
        _ => false,
    }
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
            if selector_root_is_let(items, expr, &mut std::collections::BTreeSet::new()) {
                return;
            }
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

fn named_field<'a>(items: &'a [StructItem], name: &str) -> Option<&'a Expr> {
    items.iter().find_map(|item| match item {
        StructItem::Field(field) => match &field.label {
            Label::Name(label, _) if label == name => Some(&field.value),
            _ => None,
        },
        _ => None,
    })
}

fn after_expr(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::Struct(items, _) => named_field(items, "$after"),
        Expr::Unify(left, right, _) => after_expr(left).or_else(|| after_expr(right)),
        Expr::Paren(inner, _) | Expr::Default(inner, _) => after_expr(inner),
        _ => None,
    }
}

fn after_names(item: &StructItem) -> Result<Vec<String>, ostinato_lang::LangError> {
    let StructItem::Field(field) = item else {
        return Ok(Vec::new());
    };
    let Some(after) = after_expr(&field.value) else {
        return Ok(Vec::new());
    };
    let mut names = Vec::new();
    collect_after_names(after, &mut names)?;
    Ok(names)
}

fn collect_after_names(expr: &Expr, out: &mut Vec<String>) -> Result<(), ostinato_lang::LangError> {
    match expr {
        Expr::Ident(name, _) => {
            out.push(name.clone());
            Ok(())
        }
        Expr::List(items, _) => {
            for item in items {
                collect_after_names(item, out)?;
            }
            Ok(())
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) => collect_after_names(inner, out),
        other => Err(ostinato_lang::LangError::Workflow(format!(
            "unsupported $after expression: {other:?}"
        ))),
    }
}

fn collect_schedule(
    map: &BTreeMap<String, Value>,
    ast: Option<&[StructItem]>,
    out: &mut Vec<(String, Vec<String>, BTreeMap<String, Value>)>,
) -> Result<(), ostinato_lang::LangError> {
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
                Some(items) => after_names_union(items, key)?,
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
        collect_schedule(fields, sub_ast, out)?;
    }
    Ok(())
}

fn after_names_union(
    items: &[StructItem],
    name: &str,
) -> Result<Vec<String>, ostinato_lang::LangError> {
    let mut dependencies = Vec::new();
    for item in items {
        let StructItem::Field(field) = item else {
            continue;
        };
        if !matches!(&field.label, Label::Name(label, _) if label == name) {
            continue;
        }
        for dependency in after_names(item)? {
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

fn local_definition<'a>(
    scope: &'a [StructItem],
    root: &'a [StructItem],
    name: &str,
) -> Option<&'a Expr> {
    fn find<'a>(items: &'a [StructItem], name: &str) -> Option<&'a Expr> {
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
    find(scope, name).or_else(|| find(root, name))
}

fn collect_output_refs(
    expr: &Expr,
    scope: &[StructItem],
    root: &[StructItem],
    names: &std::collections::BTreeSet<String>,
    visited: &mut std::collections::BTreeSet<String>,
    out: &mut std::collections::BTreeSet<String>,
) {
    match expr {
        Expr::Select(base, field, _, _) => {
            if matches!(field.as_str(), "stdout" | "success")
                && let Expr::Ident(name, _) = unwrap_ref_base(base)
                && names.contains(name)
            {
                out.insert(name.clone());
            }
            collect_output_refs(base, scope, root, names, visited, out);
        }
        Expr::Ident(name, _) => {
            if !names.contains(name)
                && visited.insert(name.clone())
                && let Some(value) = local_definition(scope, root, name)
            {
                collect_output_refs(value, scope, root, names, visited, out);
            }
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            collect_output_refs(left, scope, root, names, visited, out);
            collect_output_refs(right, scope, root, names, visited, out);
        }
        Expr::Bound(_, inner, _)
        | Expr::Default(inner, _)
        | Expr::Paren(inner, _)
        | Expr::Unary(_, inner, _) => {
            collect_output_refs(inner, scope, root, names, visited, out);
        }
        Expr::Index(base, index, _) => {
            collect_output_refs(base, scope, root, names, visited, out);
            collect_output_refs(index, scope, root, names, visited, out);
        }
        Expr::Call(callee, arguments, _) => {
            collect_output_refs(callee, scope, root, names, visited, out);
            for argument in arguments {
                collect_output_refs(argument, scope, root, names, visited, out);
            }
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let ostinato_lang::ast::StrPart::Interp(inner) = part {
                    collect_output_refs(inner, scope, root, names, visited, out);
                }
            }
        }
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                collect_output_refs(item, scope, root, names, visited, out);
            }
        }
        Expr::OpenList(Some(inner), _) => {
            collect_output_refs(inner, scope, root, names, visited, out);
        }
        Expr::Struct(items, _) => {
            for item in items {
                match item {
                    StructItem::Field(field) => {
                        collect_output_refs(&field.value, scope, root, names, visited, out);
                    }
                    StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                        collect_output_refs(value, scope, root, names, visited, out);
                    }
                    StructItem::For { source, body, .. } => {
                        collect_output_refs(source, scope, root, names, visited, out);
                        collect_output_refs_items(body, scope, root, names, visited, out);
                    }
                    StructItem::If { cond, body, .. } => {
                        collect_output_refs(cond, scope, root, names, visited, out);
                        collect_output_refs_items(body, scope, root, names, visited, out);
                    }
                    StructItem::Ellipsis(_) => {}
                }
            }
        }
        _ => {}
    }
}

fn collect_output_refs_items(
    items: &[StructItem],
    scope: &[StructItem],
    root: &[StructItem],
    names: &std::collections::BTreeSet<String>,
    visited: &mut std::collections::BTreeSet<String>,
    out: &mut std::collections::BTreeSet<String>,
) {
    for item in items {
        match item {
            StructItem::Field(field) => {
                collect_output_refs(&field.value, scope, root, names, visited, out);
            }
            StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                collect_output_refs(value, scope, root, names, visited, out);
            }
            StructItem::For { source, body, .. } => {
                collect_output_refs(source, scope, root, names, visited, out);
                collect_output_refs_items(body, scope, root, names, visited, out);
            }
            StructItem::If { cond, body, .. } => {
                collect_output_refs(cond, scope, root, names, visited, out);
                collect_output_refs_items(body, scope, root, names, visited, out);
            }
            StructItem::Ellipsis(_) => {}
        }
    }
}

fn collect_refs_for_task(
    items: &[StructItem],
    task_name: &str,
    root: &[StructItem],
    names: &std::collections::BTreeSet<String>,
    out: &mut std::collections::BTreeSet<String>,
) {
    for item in items {
        match item {
            StructItem::Field(field) => {
                if matches!(&field.label, Label::Name(name, _) if name == task_name) {
                    let mut visited = std::collections::BTreeSet::new();
                    collect_output_refs(&field.value, items, root, names, &mut visited, out);
                }
                collect_refs_for_task_expr(&field.value, task_name, root, names, out);
            }
            StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                collect_refs_for_task_expr(value, task_name, root, names, out);
            }
            StructItem::For { source, body, .. } => {
                collect_refs_for_task_expr(source, task_name, root, names, out);
                collect_refs_for_task(body, task_name, root, names, out);
            }
            StructItem::If { cond, body, .. } => {
                collect_refs_for_task_expr(cond, task_name, root, names, out);
                collect_refs_for_task(body, task_name, root, names, out);
            }
            StructItem::Ellipsis(_) => {}
        }
    }
}

fn collect_refs_for_task_expr(
    expr: &Expr,
    task_name: &str,
    root: &[StructItem],
    names: &std::collections::BTreeSet<String>,
    out: &mut std::collections::BTreeSet<String>,
) {
    match expr {
        Expr::Struct(items, _) => collect_refs_for_task(items, task_name, root, names, out),
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            collect_refs_for_task_expr(left, task_name, root, names, out);
            collect_refs_for_task_expr(right, task_name, root, names, out);
        }
        Expr::Paren(inner, _)
        | Expr::Default(inner, _)
        | Expr::Bound(_, inner, _)
        | Expr::Unary(_, inner, _) => {
            collect_refs_for_task_expr(inner, task_name, root, names, out);
        }
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                collect_refs_for_task_expr(item, task_name, root, names, out);
            }
        }
        Expr::OpenList(Some(inner), _) => {
            collect_refs_for_task_expr(inner, task_name, root, names, out);
        }
        _ => {}
    }
}

fn collect_let_selector_operands<'a>(
    items: &[StructItem],
    expr: &'a Expr,
    out: &mut Vec<&'a Expr>,
) {
    match expr {
        Expr::Unify(left, right, _) => {
            collect_let_selector_operands(items, left, out);
            collect_let_selector_operands(items, right, out);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            collect_let_selector_operands(items, inner, out);
        }
        _ => {
            if command_value_is_let_selector(items, expr, &mut std::collections::BTreeSet::new()) {
                out.push(expr);
            }
        }
    }
}

fn command_value<'a>(items: &'a [StructItem], command_name: &str) -> Option<&'a Expr> {
    for item in items {
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
                return Some(&command_field.value);
            }
        }
    }
    None
}

fn task_has_output_refs(
    items: &[StructItem],
    name: &str,
    names: &std::collections::BTreeSet<String>,
) -> bool {
    let mut refs = std::collections::BTreeSet::new();
    collect_refs_for_task(items, name, items, names, &mut refs);
    !refs.is_empty()
}

fn add_output_dependencies(
    items: &[StructItem],
    scheduled: &mut [(String, Vec<String>, BTreeMap<String, Value>)],
) {
    let names: std::collections::BTreeSet<String> =
        scheduled.iter().map(|(name, _, _)| name.clone()).collect();
    let mut refs: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    for (name, _, _) in scheduled.iter() {
        let mut found = std::collections::BTreeSet::new();
        collect_refs_for_task(items, name, items, &names, &mut found);
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

fn inject_path_expr(
    expr: &mut Expr,
    path: &[String],
    task_name: &str,
    outputs: &[(String, Value)],
) -> bool {
    let Some((first, rest)) = path.split_first() else {
        return inject_task_outputs_in_expr(expr, task_name, outputs);
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
                    return inject_path_expr(value, rest, task_name, outputs);
                }
            }
            false
        }
        Expr::Unify(left, right, _) => {
            inject_path_expr(left, path, task_name, outputs)
                || inject_path_expr(right, path, task_name, outputs)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            inject_path_expr(inner, path, task_name, outputs)
        }
        _ => false,
    }
}

fn inject_into_path(
    items: &mut [StructItem],
    path: &[String],
    task_name: &str,
    outputs: &[(String, Value)],
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
                return inject_task_outputs_in_expr(value, task_name, outputs);
            }
            return inject_path_expr(value, rest, task_name, outputs);
        }
    }
    false
}

fn inject_command_outputs(
    items: &mut [StructItem],
    command_name: &str,
    task_name: &str,
    outputs: &[(String, Value)],
) -> bool {
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
            if inject_task_outputs_in_expr(&mut command_field.value, task_name, outputs) {
                injected = true;
            }
        }
    }
    // Inject into referenced declarations (`command: c: base`, selectors, lets).
    for target in targets {
        if inject_into_path(items, &target, task_name, outputs) {
            injected = true;
        }
    }
    injected
}

fn inject_task_outputs(
    items: &mut [StructItem],
    task_name: &str,
    outputs: &[(String, Value)],
) -> bool {
    for item in items {
        match item {
            StructItem::Field(field) => {
                if matches!(&field.label, Label::Name(name, _) if name == task_name) {
                    let mut injected = false;
                    for (name, value) in outputs {
                        if let Some(expr) = output_expr(value)
                            && inject_into_struct(&mut field.value, name, &expr)
                        {
                            injected = true;
                        }
                    }
                    if injected {
                        return true;
                    }
                }
                if inject_task_outputs_in_expr(&mut field.value, task_name, outputs) {
                    return true;
                }
            }
            StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                if inject_task_outputs_in_expr(value, task_name, outputs) {
                    return true;
                }
            }
            StructItem::For { source, body, .. } => {
                if inject_task_outputs_in_expr(source, task_name, outputs)
                    || inject_task_outputs(body, task_name, outputs)
                {
                    return true;
                }
            }
            StructItem::If { cond, body, .. } => {
                if inject_task_outputs_in_expr(cond, task_name, outputs)
                    || inject_task_outputs(body, task_name, outputs)
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
) -> bool {
    match expr {
        Expr::Struct(items, _) => inject_task_outputs(items, task_name, outputs),
        Expr::Unify(left, right, _) => {
            inject_task_outputs_in_expr(left, task_name, outputs)
                || inject_task_outputs_in_expr(right, task_name, outputs)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) => {
            inject_task_outputs_in_expr(inner, task_name, outputs)
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
