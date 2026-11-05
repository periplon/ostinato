use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use bigdecimal::{BigDecimal, RoundingMode};
use num_bigint::BigInt;
use serde_json::{Map, Number, Value as Json};

use crate::ast::{
    BinOp, BoundOp, CompClause, Expr, File, Pattern, Span, StrPart, StructItem, UnOp,
};
use crate::error::LangError;

#[derive(Debug, Clone, PartialEq, Eq)]
struct AliasTarget {
    name: String,
    definition: bool,
    hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admission {
    pub names: BTreeSet<String>,
    pub patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundKind {
    Ge,
    Gt,
    Le,
    Lt,
    Ne,
    Match,
    NotMatch,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bound {
    pub kind: BoundKind,
    pub operand: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoundSet {
    pub items: Vec<Bound>,
    pub ty: Option<TypeKind>,
}

/// A deferred self-reference inside a recursive definition.
pub struct RecursiveSlot {
    value: RefCell<Option<Value>>,
}

impl RecursiveSlot {
    fn new() -> Self {
        Self {
            value: RefCell::new(None),
        }
    }
}

impl std::fmt::Debug for RecursiveSlot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Recursive(..)")
    }
}

impl PartialEq for RecursiveSlot {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for RecursiveSlot {}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    BigInt(BigInt),
    Float(BigDecimal),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Struct(BTreeMap<String, Value>),
    /// A disjunction with optional default alternatives (indices).
    Disjunction(Vec<Value>, Vec<usize>),
    /// A conjunction of bounds such as `>=5 & <10`.
    Bounds(BoundSet),
    /// A CUE definition: fields plus optional-field constraints. Open.
    Definition {
        fields: BTreeMap<String, Value>,
        optional: BTreeMap<String, Value>,
        definitions: BTreeMap<String, Value>,
        patterns: Vec<(Pattern, Value)>,
        closed: bool,
        open: bool,
        admissions: Vec<Admission>,
    },
    /// An open list type `[...T]`; `None` means any element.
    OpenList(Option<Box<Value>>),
    /// A list with a fixed prefix and an open tail.
    OpenListPrefix(Vec<Value>, Option<Box<Value>>),
    /// A type constraint (`string`, `int`, `number`, ...).
    Type(TypeKind),
    /// A builtin function exposed by an imported package.
    Builtin(Builtin),
    /// The top value `_` (any). Also used for incomplete values.
    Bottom,
    /// A deferred constraint conflict (only stored in optional maps).
    Conflict(String),
    /// A self-reference to a definition that is currently being materialized.
    Recursive(Rc<RecursiveSlot>),
    /// A self-reference constrained by a value unified before the definition
    /// finished materializing.
    Deferred(Rc<RecursiveSlot>, Box<Value>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    StringsToUpper,
    StringsToLower,
    StringsTrimSpace,
    StringsJoin,
    StringsSplit,
    StringsHasPrefix,
    StringsHasSuffix,
    ListRange,
    ListConcat,
    Len,
    RegexpMatch,
    RegexpFind,
    RegexpFindAll,
    RegexpReplaceAll,
    RegexpQuoteMeta,
    RegexpValid,
    MathAbs,
    MathFloor,
    MathCeil,
    MathRound,
    MathTrunc,
    MathSqrt,
    MathPow,
    MathExp,
    MathLog,
    TimeUnix,
    TimeParse,
    TimeFormat,
    TimeParseDuration,
    TimeDuration,
    TimeTime,
    TimeFormatString,
    TimeToUnix,
    TimeToUnixNano,
    TimeSplit,
    TimeFormatDuration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    String,
    Int,
    Float,
    Number,
    Bool,
    Bytes,
}

impl Value {
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) | Self::BigInt(_) => "int",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::Bytes(_) => "bytes",
            Self::List(_) => "list",
            Self::Struct(_) | Self::Definition { .. } => "struct",
            Self::Disjunction(_, _) => "disjunction",
            Self::Bounds(_) => "bound",
            Self::OpenList(_) | Self::OpenListPrefix(_, _) => "list",
            Self::Builtin(_) => "builtin",
            Self::Type(kind) => match kind {
                TypeKind::String => "string",
                TypeKind::Int => "int",
                TypeKind::Float => "float",
                TypeKind::Number => "number",
                TypeKind::Bool => "bool",
                TypeKind::Bytes => "bytes",
            },
            Self::Recursive(_) | Self::Deferred(_, _) => "recursive",
            Self::Bottom | Self::Conflict(_) => "_",
        }
    }

    /// Select a single concrete alternative when a disjunction has one
    /// surviving value or exactly one default.
    #[must_use]
    pub fn concretize(&self) -> &Value {
        match self {
            Self::Disjunction(alternatives, defaults) => {
                if alternatives.len() == 1 {
                    return alternatives[0].concretize();
                }
                if defaults.len() == 1
                    && let Some(alternative) = alternatives.get(defaults[0])
                {
                    return alternative.concretize();
                }
                self
            }
            _ => self,
        }
    }

    #[must_use]
    pub fn is_fully_concrete(&self) -> bool {
        self.is_fully_concrete_inner(&mut Vec::new())
    }

    fn is_fully_concrete_inner(&self, stack: &mut Vec<*const RecursiveSlot>) -> bool {
        stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || match self {
            Self::Null
            | Self::Bool(_)
            | Self::Int(_)
            | Self::BigInt(_)
            | Self::Float(_)
            | Self::String(_)
            | Self::Bytes(_) => true,
            Self::List(values) => values
                .iter()
                .all(|value| value.is_fully_concrete_inner(stack)),
            Self::Struct(fields) | Self::Definition { fields, .. } => fields
                .iter()
                .filter(|(key, _)| !is_hidden_key(key))
                .all(|(_, value)| value.is_fully_concrete_inner(stack)),
            Self::Disjunction(_, _) => self.to_json().is_some(),
            Self::Recursive(slot) => {
                let pointer = Rc::as_ptr(slot);
                if stack.contains(&pointer) {
                    return false;
                }
                let Some(value) = slot.value.borrow().clone() else {
                    return false;
                };
                stack.push(pointer);
                let result = value.is_fully_concrete_inner(stack);
                stack.pop();
                result
            }
            Self::Deferred(slot, constraint) => {
                let pointer = Rc::as_ptr(slot);
                if stack.contains(&pointer) {
                    return false;
                }
                let Some(value) = slot.value.borrow().clone() else {
                    return false;
                };
                let Ok(unified) = unify_values(&value, constraint, Span::default()) else {
                    return false;
                };
                stack.push(pointer);
                let result = unified.is_fully_concrete_inner(stack);
                stack.pop();
                result
            }
            // CUE compares open-list types as values (`[...int] == [...string]`),
            // but fixed prefix elements must themselves be concrete.
            Self::OpenList(_) => true,
            Self::OpenListPrefix(prefix, _) => prefix
                .iter()
                .all(|value| value.is_fully_concrete_inner(stack)),
            Self::Bounds(_)
            | Self::Type(_)
            | Self::Builtin(_)
            | Self::Bottom
            | Self::Conflict(_) => false,
        })
    }

    #[must_use]
    pub fn is_concrete(&self) -> bool {
        match self {
            Self::Disjunction(_, _) | Self::Recursive(_) | Self::Deferred(_, _) => {
                self.to_json().is_some()
            }
            Self::Bounds(_) => false,
            _ => !matches!(self, Self::Type(_) | Self::Bottom | Self::Conflict(_)),
        }
    }

    /// Convert to JSON, returning `None` for incomplete values.
    #[must_use]
    pub fn to_json(&self) -> Option<Json> {
        self.to_json_inner(&mut Vec::new())
    }

    fn to_json_inner(&self, stack: &mut Vec<*const RecursiveSlot>) -> Option<Json> {
        stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || {
            self.to_json_inner_impl(stack)
        })
    }

    fn to_json_inner_impl(&self, stack: &mut Vec<*const RecursiveSlot>) -> Option<Json> {
        match self {
            Self::Null => Some(Json::Null),
            Self::Bool(value) => Some(Json::Bool(*value)),
            Self::Int(value) => Some(Json::Number(Number::from(*value))),
            Self::BigInt(value) => value.to_string().parse::<Number>().ok().map(Json::Number),
            Self::Float(value) => {
                let (_, scale) = value.as_bigint_and_exponent();
                float_text(value, scale)
                    .parse::<Number>()
                    .ok()
                    .map(Json::Number)
            }
            Self::String(value) => Some(Json::String(value.clone())),
            Self::Bytes(value) => Some(Json::String(base64_encode(value))),
            Self::List(values) => {
                let mut out = Vec::with_capacity(values.len());
                for value in values {
                    out.push(value.to_json_inner(stack)?);
                }
                Some(Json::Array(out))
            }
            Self::Struct(values) | Self::Definition { fields: values, .. } => {
                let mut out = Map::new();
                for (key, value) in values {
                    if is_hidden_key(key) {
                        continue;
                    }
                    let key = key.strip_prefix(ESCAPED_PREFIX).unwrap_or(key);
                    out.insert(key.to_owned(), value.to_json_inner(stack)?);
                }
                Some(Json::Object(out))
            }
            Self::Disjunction(alternatives, defaults) => {
                disjunction_to_json(alternatives, defaults)
            }
            Self::Bounds(_) => None,
            Self::OpenListPrefix(prefix, _) => {
                let mut out = Vec::with_capacity(prefix.len());
                for value in prefix {
                    out.push(value.to_json_inner(stack)?);
                }
                Some(Json::Array(out))
            }
            Self::OpenList(_) => Some(Json::Array(Vec::new())),
            Self::Recursive(slot) => {
                let pointer = Rc::as_ptr(slot);
                if stack.contains(&pointer) {
                    return None;
                }
                let resolved = slot.value.borrow().clone()?;
                stack.push(pointer);
                let result = resolved.to_json_inner(stack);
                stack.pop();
                result
            }
            Self::Deferred(slot, constraint) => {
                let pointer = Rc::as_ptr(slot);
                if stack.contains(&pointer) {
                    return None;
                }
                let Some(resolved) = slot.value.borrow().clone() else {
                    return constraint.to_json_inner(stack);
                };
                let unified = unify_values(&resolved, constraint, Span::default()).ok()?;
                stack.push(pointer);
                let result = unified.to_json_inner(stack);
                stack.pop();
                result
            }
            Self::Builtin(_) | Self::Conflict(_) | Self::Type(_) | Self::Bottom => None,
        }
    }

    fn open_list_element(&self) -> Option<&Value> {
        match self {
            Self::OpenList(element) => element.as_deref(),
            _ => None,
        }
    }

    #[must_use]
    pub fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(value) => *value,
            Self::Int(value) => *value != 0,
            Self::BigInt(value) => value != &BigInt::from(0),
            Self::Float(value) => value != &BigDecimal::from(0),
            Self::String(value) => !value.is_empty(),
            Self::Bytes(value) => !value.is_empty(),
            Self::List(values) => !values.is_empty(),
            Self::Struct(values) | Self::Definition { fields: values, .. } => !values.is_empty(),
            Self::Disjunction(alternatives, defaults) => {
                disjunction_to_json(alternatives, defaults).is_some_and(|json| match json {
                    Json::Null => false,
                    Json::Bool(value) => value,
                    Json::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
                    Json::String(value) => !value.is_empty(),
                    Json::Array(values) => !values.is_empty(),
                    Json::Object(values) => !values.is_empty(),
                })
            }
            Self::Bounds(_) => false,
            Self::OpenList(_) | Self::OpenListPrefix(_, _) => true,
            Self::Recursive(slot) => slot.value.borrow().as_ref().is_some_and(Value::truthy),
            Self::Deferred(slot, constraint) => slot
                .value
                .borrow()
                .as_ref()
                .map_or_else(|| constraint.truthy(), Value::truthy),
            Self::Builtin(_) | Self::Type(_) | Self::Bottom | Self::Conflict(_) => false,
        }
    }

    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::Null => "null".to_owned(),
            Self::Bool(value) => value.to_string(),
            Self::Int(value) => value.to_string(),
            Self::BigInt(value) => value.to_string(),
            Self::Float(value) => {
                let (_, scale) = value.as_bigint_and_exponent();
                float_text(value, scale)
            }
            Self::String(value) => value.clone(),
            Self::Bytes(value) => format!("'{}'", String::from_utf8_lossy(value)),
            Self::Type(kind) => match kind {
                TypeKind::String => "string",
                TypeKind::Int => "int",
                TypeKind::Float => "float",
                TypeKind::Number => "number",
                TypeKind::Bool => "bool",
                TypeKind::Bytes => "bytes",
            }
            .to_owned(),
            Self::Builtin(_) => "<function>".to_owned(),
            Self::Disjunction(alternatives, defaults) => {
                let mut parts = Vec::with_capacity(alternatives.len());
                for (index, alternative) in alternatives.iter().enumerate() {
                    let prefix = if defaults.contains(&index) { "*" } else { "" };
                    parts.push(format!("{prefix}{}", alternative.display()));
                }
                parts.join(" | ")
            }
            Self::Bounds(bounds) => bounds
                .items
                .iter()
                .map(bound_display)
                .collect::<Vec<_>>()
                .join(" & "),
            Self::Bottom => "_".to_owned(),
            Self::Conflict(message) => format!("<conflict: {message}>"),
            Self::OpenList(_) => format!(
                "[...{}]",
                self.open_list_element()
                    .map_or_else(|| "_".to_owned(), |value| value.display())
            ),
            Self::OpenListPrefix(prefix, tail) => {
                let mut rendered = String::from("[");
                for value in prefix {
                    rendered.push_str(&value.display());
                    rendered.push_str(", ");
                }
                rendered.push_str("...");
                if let Some(tail) = tail {
                    rendered.push_str(&tail.display());
                }
                rendered.push(']');
                rendered
            }
            Self::List(_)
            | Self::Struct(_)
            | Self::Definition { .. }
            | Self::Recursive(_)
            | Self::Deferred(_, _) => self.to_json().map_or_else(
                || format!("<{}>", self.type_name()),
                |json| json.to_string(),
            ),
        }
    }
}

/// Select a CUE-style path expression from an evaluated value.
///
/// Supports dotted fields, hidden fields (`_name`), definitions (`#Name`) and
/// numeric indexes written as `[N]` or as a dotted segment. Optional fields
/// cannot be referenced, matching `cue export -e`.
///
/// # Errors
/// Returns [`LangError::Workflow`] for malformed expressions, missing
/// references and optional fields.
pub fn select_path(value: &Value, expression: &str) -> Result<Value, LangError> {
    let segments = parse_selector(expression)?;
    let mut current = value.clone();
    for segment in segments {
        current = select_segment(&current, &segment)?;
    }
    Ok(current)
}

fn select_segment(value: &Value, segment: &str) -> Result<Value, LangError> {
    match value {
        Value::Struct(fields) => lookup_field(fields, segment),
        Value::Definition {
            fields,
            optional,
            definitions,
            ..
        } => {
            if segment.starts_with('#') {
                return definitions.get(segment).cloned().ok_or_else(|| {
                    LangError::Workflow(format!("reference \"{segment}\" not found"))
                });
            }
            if optional.contains_key(segment) {
                return Err(LangError::Workflow(format!(
                    "cannot reference optional field: {segment}"
                )));
            }
            lookup_field(fields, segment)
        }
        Value::List(items) | Value::OpenListPrefix(items, _) => {
            let index = segment.parse::<usize>().map_err(|_| {
                LangError::Workflow(format!("cannot use `{segment}` as a list index"))
            })?;
            items
                .get(index)
                .cloned()
                .ok_or_else(|| LangError::Workflow(format!("list index {index} out of range")))
        }
        other => Err(LangError::Workflow(format!(
            "cannot select `{segment}` from {}",
            other.type_name()
        ))),
    }
}

fn lookup_field(fields: &BTreeMap<String, Value>, segment: &str) -> Result<Value, LangError> {
    // Hidden fields are stored with an internal prefix (`\u{3}_name`) and
    // escaped labels with another (`\u{4}`). `cue export -e _name` addresses
    // the user-visible name, so try the prefixed forms too.
    fields
        .get(segment)
        .or_else(|| fields.get(&format!("\u{3}{segment}")))
        .or_else(|| fields.get(&format!("\u{4}{segment}")))
        .cloned()
        .ok_or_else(|| LangError::Workflow(format!("reference \"{segment}\" not found")))
}

fn parse_selector(expression: &str) -> Result<Vec<String>, LangError> {
    let expression = expression.trim();
    if expression.is_empty() {
        return Err(LangError::Workflow("empty expression".to_owned()));
    }
    let mut segments = Vec::new();
    let chars: Vec<char> = expression.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '.' => {
                index += 1;
                if index >= chars.len() {
                    return Err(LangError::Workflow("expression ends with `.`".to_owned()));
                }
            }
            '[' => {
                let Some(end) = chars[index + 1..].iter().position(|ch| *ch == ']') else {
                    return Err(LangError::Workflow(
                        "unterminated `[` in expression".to_owned(),
                    ));
                };
                let inner: String = chars[index + 1..index + 1 + end].iter().collect();
                let inner = inner.trim();
                if inner.is_empty() {
                    return Err(LangError::Workflow("empty `[]` index".to_owned()));
                }
                if inner.starts_with('"') {
                    let key: String = serde_json::from_str(inner).map_err(|_| {
                        LangError::Workflow(format!("invalid quoted selector {inner}"))
                    })?;
                    segments.push(key);
                } else if inner.starts_with('\'') && inner.ends_with('\'') && inner.len() >= 2 {
                    segments.push(inner[1..inner.len() - 1].to_owned());
                } else {
                    segments.push(inner.to_owned());
                }
                index += end + 2;
            }
            '"' => {
                let start = index;
                index += 1;
                let mut escaped = false;
                while index < chars.len() {
                    let ch = chars[index];
                    index += 1;
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == '"' {
                        break;
                    }
                }
                let literal: String = chars[start..index].iter().collect();
                let key: String = serde_json::from_str(&literal).map_err(|_| {
                    LangError::Workflow(format!("invalid quoted selector {literal}"))
                })?;
                segments.push(key);
            }
            _ => {
                let start = index;
                while index < chars.len() && !matches!(chars[index], '.' | '[') {
                    index += 1;
                }
                let segment: String = chars[start..index].iter().collect();
                let segment = segment.trim();
                if segment.is_empty() {
                    return Err(LangError::Workflow("empty path segment".to_owned()));
                }
                segments.push(segment.to_owned());
            }
        }
    }
    if segments.is_empty() {
        return Err(LangError::Workflow("empty expression".to_owned()));
    }
    Ok(segments)
}

/// Evaluate a parsed file into a concrete-ish value tree.
///
/// # Errors
/// Returns evaluation errors for undefined references, cyclic references,
/// conflicting values and non-concrete disjunctions.
pub fn eval_file(file: &File) -> Result<Value, LangError> {
    stacker::grow(512 * 1024 * 1024, || {
        crate::attributes::validate_tag_placement(file)?;
        crate::scope::validate_imports(file)?;
        crate::scope::validate_aliases(&file.items, &[])?;
        let frame = Frame::new(&file.items, None, HashMap::new())?;
        frame.ensure_params_context();
        frame.to_struct_value()
    })
}

/// Evaluate a single expression against an already-built JSON context.
///
/// # Errors
/// Returns an evaluation error when references cannot be resolved.
pub fn eval_expr_with_json(expr: &Expr, context: &Json) -> Result<Value, LangError> {
    let frame = json_frame(context);
    eval_expr(expr, &frame)
}

/// Evaluate a file after injecting concrete parameter overrides.
///
/// # Errors
/// Returns an error when `params` is missing or not a struct.
pub fn eval_file_with_overrides(
    file: &File,
    overrides: &BTreeMap<String, Value>,
) -> Result<Value, LangError> {
    stacker::grow(512 * 1024 * 1024, || {
        crate::attributes::validate_tag_placement(file)?;
        crate::scope::validate_imports(file)?;
        crate::scope::validate_aliases(&file.items, &[])?;
        let frame = Frame::from_static(&file.items, None, HashMap::new())?;
        for (key, value) in overrides {
            frame
                .overrides
                .borrow_mut()
                .insert(key.clone(), value.clone());
        }
        frame.ensure_params_context();
        frame.expand(&file.items)?;
        frame.to_struct_value()
    })
}

/// Evaluate a file with imported package values and parameter overrides.
///
/// Imports are inserted into the root scope before `let` bindings and
/// comprehensions are evaluated.
///
/// # Errors
/// Returns an evaluation error for invalid imports or missing `params`.
pub fn eval_file_with_imports(
    file: &File,
    imports: &BTreeMap<String, Value>,
    overrides: &BTreeMap<String, Value>,
) -> Result<Value, LangError> {
    eval_file_with_imports_mode(file, imports, overrides, false)
}

/// Like [`eval_file_with_imports`], optionally retaining definitions.
///
/// Imported packages need `#definitions` to be visible; the root package
/// export must hide them, matching `cue export`.
///
/// # Errors
/// Returns an evaluation error for invalid imports or missing `params`.
pub fn eval_file_with_imports_mode(
    file: &File,
    imports: &BTreeMap<String, Value>,
    overrides: &BTreeMap<String, Value>,
    include_definitions: bool,
) -> Result<Value, LangError> {
    eval_file_with_imports_mode_inner(
        file,
        imports,
        overrides,
        include_definitions,
        IncompleteMode::Forbid,
    )
}

/// Like [`eval_file_with_imports_mode`], but non-concrete interpolations become
/// incomplete placeholders instead of errors.
///
/// Tool commands use this mode so task inputs that reference captured outputs
/// can be scheduled and re-evaluated after the producer injects its values.
///
/// # Errors
/// Returns an evaluation error for invalid imports or tags.
pub fn eval_file_with_imports_mode_allow_incomplete(
    file: &File,
    imports: &BTreeMap<String, Value>,
    overrides: &BTreeMap<String, Value>,
    include_definitions: bool,
) -> Result<Value, LangError> {
    eval_file_with_imports_mode_inner(
        file,
        imports,
        overrides,
        include_definitions,
        IncompleteMode::AllowAll,
    )
}

/// Like [`eval_file_with_imports_mode`], but only interpolations that reference
/// the given task outputs may remain incomplete.
///
/// # Errors
/// Returns an evaluation error for invalid imports, tags or other incomplete
/// interpolations.
pub fn eval_file_with_imports_mode_allow_outputs(
    file: &File,
    imports: &BTreeMap<String, Value>,
    overrides: &BTreeMap<String, Value>,
    include_definitions: bool,
    outputs: &HashSet<String>,
) -> Result<Value, LangError> {
    eval_file_with_imports_mode_inner(
        file,
        imports,
        overrides,
        include_definitions,
        IncompleteMode::AllowOnly(outputs.clone()),
    )
}

fn eval_file_with_imports_mode_inner(
    file: &File,
    imports: &BTreeMap<String, Value>,
    overrides: &BTreeMap<String, Value>,
    include_definitions: bool,
    incomplete_mode: IncompleteMode,
) -> Result<Value, LangError> {
    stacker::grow(512 * 1024 * 1024, || {
        crate::attributes::validate_tag_placement(file)?;
        let import_aliases: Vec<String> = imports.keys().cloned().collect();
        crate::scope::validate_imports(file)?;
        crate::scope::validate_aliases(&file.items, &import_aliases)?;
        let mut vars = HashMap::new();
        for (name, value) in imports {
            vars.insert(name.clone(), value.clone());
        }
        let frame = Frame::from_static(&file.items, None, vars)?;
        *frame.incomplete_mode.borrow_mut() = incomplete_mode;
        for (key, value) in overrides {
            frame
                .overrides
                .borrow_mut()
                .insert(key.clone(), value.clone());
        }
        frame.ensure_params_context();
        frame.expand(&file.items)?;
        frame.to_struct_value_mode(include_definitions)
    })
}

fn json_frame(context: &Json) -> Frame<'static> {
    let frame = Frame {
        parent: None,
        vars: RefCell::new(HashMap::new()),
        lets: RefCell::new(BTreeMap::new()),
        fields: RefCell::new(BTreeMap::new()),
        values: RefCell::new(BTreeMap::new()),
        generated_optionals: RefCell::new(BTreeMap::new()),
        materialized: RefCell::new(BTreeSet::new()),
        injected: RefCell::new(HashSet::new()),
        patterns: RefCell::new(Vec::new()),
        aliases: RefCell::new(BTreeMap::new()),
        closed: Cell::new(false),
        overrides: RefCell::new(BTreeMap::new()),
        evaluating: RefCell::new(HashSet::new()),
        self_rhs: RefCell::new(None),
        params_scope: Cell::new(false),
        params_context: RefCell::new(None),
        params_prefix: RefCell::new(Vec::new()),
        params_segment: RefCell::new(None),
        recursive_handles: RefCell::new(BTreeMap::new()),
        definition_context: RefCell::new(None),
        next_definition: RefCell::new(None),
        next_injections: RefCell::new(None),
        generated_definitions: RefCell::new(BTreeMap::new()),
        skip_cache: Cell::new(false),
        defer_incomplete: Cell::new(false),
        incomplete_mode: RefCell::new(IncompleteMode::Forbid),
    };
    if let Json::Object(map) = context {
        let mut values = BTreeMap::new();
        for (key, value) in map {
            if let Some(value) = json_to_value(value) {
                values.insert(key.clone(), value);
            }
        }
        *frame.values.borrow_mut() = values;
    }
    frame
}

fn json_to_value(value: &Json) -> Option<Value> {
    Some(match value {
        Json::Null => Value::Null,
        Json::Bool(value) => Value::Bool(*value),
        Json::Number(number) => {
            if let Some(int) = number.as_i64() {
                Value::Int(int)
            } else {
                let text = number.to_string();
                if !text.contains(['.', 'e', 'E'])
                    && let Ok(value) = text.parse::<BigInt>()
                {
                    i64::try_from(&value).map_or(Value::BigInt(value), Value::Int)
                } else {
                    Value::Float(text.parse::<BigDecimal>().ok()?)
                }
            }
        }
        Json::String(value) => Value::String(value.clone()),
        Json::Array(values) => Value::List(values.iter().filter_map(json_to_value).collect()),
        Json::Object(map) => Value::Struct(
            map.iter()
                .filter_map(|(key, value)| Some((key.clone(), json_to_value(value)?)))
                .collect(),
        ),
    })
}

#[derive(Clone, Default)]
enum IncompleteMode {
    #[default]
    Forbid,
    AllowAll,
    AllowOnly(HashSet<String>),
}

#[derive(Clone)]
struct FieldDef {
    exprs: Vec<Expr>,
    optional: bool,
    definition: bool,
    bindable: bool,
}

/// Prefix for a regular field whose label collides with a definition name.
const NON_DEFINITION_PREFIX: char = '\u{1}';

fn prefixed_field(name: &str) -> String {
    format!("{NON_DEFINITION_PREFIX}{name}")
}

/// Prefix for an identifier-hidden field (`_name`).
const HIDDEN_PREFIX: char = '\u{3}';
/// Prefix for a regular label that itself begins with an internal prefix.
const ESCAPED_PREFIX: char = '\u{4}';

fn output_field_name(name: &str) -> String {
    if is_hidden_key(name) || name.starts_with(ESCAPED_PREFIX) {
        name.to_owned()
    } else {
        display_field_name(name).to_owned()
    }
}

fn hidden_field(name: &str) -> String {
    format!("{HIDDEN_PREFIX}{name}")
}

fn escaped_field(name: &str) -> String {
    format!("{ESCAPED_PREFIX}{name}")
}

pub(crate) fn is_hidden_key(key: &str) -> bool {
    key.starts_with(HIDDEN_PREFIX)
}

fn display_field_name(key: &str) -> &str {
    if let Some(rest) = key.strip_prefix(NON_DEFINITION_PREFIX) {
        return rest.strip_prefix(ESCAPED_PREFIX).unwrap_or(rest);
    }
    if let Some(rest) = key.strip_prefix(ESCAPED_PREFIX) {
        return rest;
    }
    key.strip_prefix(HIDDEN_PREFIX).unwrap_or(key)
}

#[derive(Debug)]
struct ParamsContext {
    items: Vec<StructItem>,
    overrides: BTreeMap<String, Value>,
    values: RefCell<BTreeMap<Vec<String>, Value>>,
    evaluating: RefCell<HashSet<Vec<String>>>,
    local: RefCell<HashSet<Vec<String>>>,
}

struct Frame<'a> {
    parent: Option<&'a Frame<'a>>,
    vars: RefCell<HashMap<String, Value>>,
    lets: RefCell<BTreeMap<String, Expr>>,
    fields: RefCell<BTreeMap<String, FieldDef>>,
    values: RefCell<BTreeMap<String, Value>>,
    generated_optionals: RefCell<BTreeMap<String, Value>>,
    materialized: RefCell<BTreeSet<String>>,
    /// Fields whose cached value was supplied by an external unification.
    /// Self-selection must ignore these, matching CUE's standalone evaluation
    /// of a definition's own selectors.
    injected: RefCell<HashSet<String>>,
    patterns: RefCell<Vec<(Pattern, Expr)>>,
    aliases: RefCell<BTreeMap<String, AliasTarget>>,
    closed: Cell<bool>,
    overrides: RefCell<BTreeMap<String, Value>>,
    evaluating: RefCell<HashSet<String>>,
    self_rhs: RefCell<Option<String>>,
    params_scope: Cell<bool>,
    params_context: RefCell<Option<Rc<ParamsContext>>>,
    params_prefix: RefCell<Vec<String>>,
    params_segment: RefCell<Option<String>>,
    recursive_handles: RefCell<BTreeMap<String, Rc<RecursiveSlot>>>,
    definition_context: RefCell<Option<String>>,
    next_definition: RefCell<Option<String>>,
    next_injections: RefCell<Option<BTreeMap<String, Value>>>,
    generated_definitions: RefCell<BTreeMap<String, Value>>,
    skip_cache: Cell<bool>,
    /// When set, soft incompleteness errors (`IncompleteValue`,
    /// `IncompleteOptional`) are suppressed so a validation pass can surface
    /// harder errors underneath them.
    defer_incomplete: Cell<bool>,
    incomplete_mode: RefCell<IncompleteMode>,
}

impl<'a> Frame<'a> {
    fn new(
        items: &[StructItem],
        parent: Option<&'a Frame<'a>>,
        vars: HashMap<String, Value>,
    ) -> Result<Self, LangError> {
        let frame = Self::from_static(items, parent, vars)?;
        frame.expand(items)?;
        Ok(frame)
    }

    fn from_static(
        items: &[StructItem],
        parent: Option<&'a Frame<'a>>,
        vars: HashMap<String, Value>,
    ) -> Result<Self, LangError> {
        let frame = Self {
            parent,
            vars: RefCell::new(vars),
            lets: RefCell::new(BTreeMap::new()),
            fields: RefCell::new(BTreeMap::new()),
            values: RefCell::new(BTreeMap::new()),
            generated_optionals: RefCell::new(BTreeMap::new()),
            materialized: RefCell::new(BTreeSet::new()),
            injected: RefCell::new(HashSet::new()),
            patterns: RefCell::new(Vec::new()),
            aliases: RefCell::new(BTreeMap::new()),
            closed: Cell::new(parent.is_some_and(|parent| parent.closed.get())),
            overrides: RefCell::new(BTreeMap::new()),
            evaluating: RefCell::new(HashSet::new()),
            self_rhs: RefCell::new(None),
            params_scope: Cell::new(false),
            params_context: RefCell::new(
                parent.and_then(|parent| parent.params_context.borrow().clone()),
            ),
            params_prefix: RefCell::new(Vec::new()),
            params_segment: RefCell::new(None),
            recursive_handles: RefCell::new(BTreeMap::new()),
            definition_context: RefCell::new(None),
            next_definition: RefCell::new(None),
            next_injections: RefCell::new(None),
            generated_definitions: RefCell::new(BTreeMap::new()),
            skip_cache: Cell::new(false),
            defer_incomplete: Cell::new(parent.is_some_and(|parent| parent.defer_incomplete.get())),
            incomplete_mode: RefCell::new(parent.map_or_else(IncompleteMode::default, |parent| {
                parent.incomplete_mode.borrow().clone()
            })),
        };

        // Pass 1: static fields and lazy let bindings.
        for item in items {
            match item {
                StructItem::Field(field) => {
                    if let crate::ast::Label::Name(name, _) = &field.label {
                        frame.insert_field(
                            name.clone(),
                            field.value.clone(),
                            field.optional,
                            field.definition,
                            !field.quoted,
                            !field.quoted && name.starts_with('_'),
                        );
                        if let Some(alias) = &field.alias {
                            frame.aliases.borrow_mut().insert(
                                alias.clone(),
                                AliasTarget {
                                    name: name.clone(),
                                    definition: field.definition,
                                    hidden: !field.quoted && name.starts_with('_'),
                                },
                            );
                        }
                    }
                }
                StructItem::Let { name, value, .. } => {
                    frame.lets.borrow_mut().insert(name.clone(), value.clone());
                }
                StructItem::Pattern { pattern, value, .. } => {
                    frame
                        .patterns
                        .borrow_mut()
                        .push((pattern.clone(), value.clone()));
                }
                _ => {}
            }
        }
        Ok(frame)
    }

    fn expand(&self, items: &[StructItem]) -> Result<(), LangError> {
        let mut deferred: Vec<(&StructItem, LangError)> = Vec::new();
        for item in items {
            match self.expand_item(item) {
                Ok(()) => {}
                Err(err) if params_cycle_error(&err) => deferred.push((item, err)),
                Err(err) => return Err(err),
            }
        }
        while !deferred.is_empty() {
            let mut next: Vec<(&StructItem, LangError)> = Vec::new();
            let mut progress = false;
            for (item, _) in deferred {
                match self.expand_item(item) {
                    Ok(()) => progress = true,
                    Err(err) if params_cycle_error(&err) => next.push((item, err)),
                    Err(err) => return Err(err),
                }
            }
            if !progress {
                if let Some((_, err)) = next.first() {
                    return Err(err.clone());
                }
                return Ok(());
            }
            deferred = next;
        }
        Ok(())
    }

    fn expand_item(&self, item: &StructItem) -> Result<(), LangError> {
        match item {
            StructItem::Field(field) => {
                if let crate::ast::Label::Dynamic(expr, span) = &field.label {
                    let key = eval_expr(expr, self)?;
                    let Value::String(key) = key else {
                        return Err(LangError::eval(
                            span.line,
                            span.col,
                            "dynamic field label must evaluate to a string",
                        ));
                    };
                    self.insert_field(
                        key.clone(),
                        field.value.clone(),
                        field.optional,
                        false,
                        false,
                        false,
                    );
                    if let Some(alias) = &field.alias {
                        self.aliases.borrow_mut().insert(
                            alias.clone(),
                            AliasTarget {
                                name: key.clone(),
                                definition: false,
                                hidden: false,
                            },
                        );
                    }
                }
            }
            StructItem::For {
                key,
                value,
                source,
                body,
                span,
            } => {
                let source_value = eval_expr(source, self)?.concretize().clone();
                match source_value {
                    Value::Struct(map) | Value::Definition { fields: map, .. } => {
                        for (entry_key, entry_value) in map {
                            if is_hidden_key(&entry_key) {
                                continue;
                            }
                            let mut bindings = HashMap::new();
                            if let Some(key) = key {
                                bindings.insert(
                                    key.clone(),
                                    Value::String(display_field_name(&entry_key).to_owned()),
                                );
                            }
                            bindings.insert(value.clone(), entry_value);
                            self.expand_body(body, bindings)?;
                        }
                    }
                    Value::List(list) => {
                        for (index, entry_value) in list.into_iter().enumerate() {
                            let mut bindings = HashMap::new();
                            if let Some(key) = key {
                                bindings.insert(
                                    key.clone(),
                                    Value::Int(i64::try_from(index).unwrap_or(i64::MAX)),
                                );
                            }
                            bindings.insert(value.clone(), entry_value);
                            self.expand_body(body, bindings)?;
                        }
                    }
                    other => {
                        return Err(LangError::eval(
                            span.line,
                            span.col,
                            format!(
                                "`for ... in` requires a struct or list, found {}",
                                other.type_name()
                            ),
                        ));
                    }
                }
            }
            StructItem::If { cond, body, span } => {
                let condition = require_bool(self, &eval_expr(cond, self)?, *span)?;
                if condition {
                    self.expand_body(body, HashMap::new())?;
                }
            }
            StructItem::Pattern { .. } | StructItem::Let { .. } | StructItem::Ellipsis(_) => {}
        }
        Ok(())
    }

    fn insert_field(
        &self,
        name: String,
        expr: Expr,
        optional: bool,
        definition: bool,
        bindable: bool,
        hidden: bool,
    ) {
        let mut fields = self.fields.borrow_mut();
        let name = if !hidden
            && !definition
            && name.starts_with([HIDDEN_PREFIX, ESCAPED_PREFIX, NON_DEFINITION_PREFIX])
        {
            escaped_field(&name)
        } else {
            name
        };
        let key = if hidden && !definition {
            hidden_field(&name)
        } else if definition {
            // A definition and a quoted regular field of the same name are
            // distinct fields; keep the regular one under a marked key.
            if let Some(existing) = fields.remove(&name) {
                if existing.definition {
                    fields.insert(name.clone(), existing);
                } else {
                    fields.insert(prefixed_field(&name), existing);
                }
            }
            name
        } else if fields.get(&name).is_some_and(|def| def.definition) {
            prefixed_field(&name)
        } else {
            name
        };
        fields
            .entry(key)
            .and_modify(|def| {
                def.exprs.push(expr.clone());
                def.optional = def.optional && optional;
                def.definition = def.definition || definition;
                def.bindable = def.bindable || bindable;
            })
            .or_insert(FieldDef {
                exprs: vec![expr],
                optional,
                definition,
                bindable,
            });
    }

    fn bindable_field_key(&self, name: &str) -> Option<String> {
        let fields = self.fields.borrow();
        if name.starts_with('_') {
            let hidden = hidden_field(name);
            return fields
                .get(&hidden)
                .is_some_and(|def| def.bindable)
                .then_some(hidden);
        }
        if fields.get(name).is_some_and(|def| def.bindable) {
            return Some(name.to_owned());
        }
        [prefixed_field(name), escaped_field(name)]
            .into_iter()
            .find(|prefixed| fields.get(prefixed).is_some_and(|def| def.bindable))
    }

    fn any_field_key(&self, name: &str) -> Option<String> {
        let fields = self.fields.borrow();
        if name.starts_with('_') {
            let hidden = hidden_field(name);
            return fields.contains_key(&hidden).then_some(hidden);
        }
        if name.starts_with('#') {
            if fields
                .get(name)
                .is_some_and(|definition| definition.definition)
            {
                return Some(name.to_owned());
            }
            drop(fields);
            return self
                .generated_definitions
                .borrow()
                .contains_key(name)
                .then(|| name.to_owned());
        }
        if fields.contains_key(name) {
            return Some(name.to_owned());
        }
        [prefixed_field(name), escaped_field(name)]
            .into_iter()
            .find(|prefixed| fields.contains_key(prefixed))
    }

    fn map_identifier_key<'m>(map: &'m BTreeMap<String, Value>, name: &str) -> Option<&'m String> {
        if name.starts_with('_') {
            map.get_key_value(&hidden_field(name)).map(|(key, _)| key)
        } else {
            map.get_key_value(name).map(|(key, _)| key)
        }
    }

    fn field_storage_key(&self, name: &str) -> Option<String> {
        self.regular_field_key(name).or_else(|| {
            let fields = self.fields.borrow();
            if fields.contains_key(name) {
                Some(prefixed_field(name))
            } else if fields.contains_key(&hidden_field(name)) {
                Some(hidden_field(name))
            } else if fields.contains_key(&escaped_field(name)) {
                Some(escaped_field(name))
            } else {
                None
            }
        })
    }

    fn stored_field_key(&self, name: &str) -> Option<String> {
        self.any_field_key(name)
    }

    fn regular_field_key(&self, name: &str) -> Option<String> {
        let fields = self.fields.borrow();
        if fields
            .get(name)
            .is_some_and(|def| !def.definition && !is_hidden_key(name))
        {
            return Some(name.to_owned());
        }
        let prefixed = prefixed_field(name);
        if fields.get(&prefixed).is_some_and(|def| !def.definition) {
            return Some(prefixed);
        }
        None
    }

    fn reconcile_generated_required_fields(&self) -> Result<(), LangError> {
        let names: Vec<String> = self
            .generated_optionals
            .borrow()
            .keys()
            .filter(|name| {
                self.regular_field_key(name)
                    .and_then(|key| self.fields.borrow().get(&key).map(|field| !field.optional))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        for name in names {
            if let Some(constraint) = self.generated_optionals.borrow_mut().remove(&name) {
                let key = self
                    .regular_field_key(&name)
                    .unwrap_or_else(|| name.clone());
                let value = self.eval_field_key(&key, Span::default())?;
                let merged = unify_values(&value, &constraint, Span::default())?;
                self.values.borrow_mut().insert(key, merged);
                self.materialized.borrow_mut().insert(name);
            }
        }
        Ok(())
    }

    fn generated_optional_key(&self, field: &str, quoted: bool) -> Option<String> {
        let optionals = self.generated_optionals.borrow();
        if quoted {
            if field.starts_with([HIDDEN_PREFIX, ESCAPED_PREFIX, NON_DEFINITION_PREFIX]) {
                let key = escaped_field(field);
                return optionals.contains_key(&key).then_some(key);
            }
            if optionals.contains_key(field) {
                return Some(field.to_owned());
            }
            let key = escaped_field(field);
            return optionals.contains_key(&key).then_some(key);
        }
        Frame::map_identifier_key(&optionals, field).cloned()
    }

    fn display_storage_key(&self, key: &str) -> Option<String> {
        if is_hidden_key(key) {
            return self
                .fields
                .borrow()
                .contains_key(key)
                .then(|| key.to_owned());
        }
        self.regular_field_key(key).or_else(|| {
            self.fields
                .borrow()
                .contains_key(key)
                .then(|| prefixed_field(key))
        })
    }

    fn merge_generated_constraint(&self, name: String, value: Value) -> Result<(), LangError> {
        let key = self.display_storage_key(&name);
        let declared = key
            .as_ref()
            .and_then(|key| self.fields.borrow().get(key).map(|field| field.optional));
        let materialized_here = self.materialized.borrow().contains(&name)
            || key
                .as_ref()
                .is_some_and(|key| self.values.borrow().contains_key(key));
        let forced_required = match declared {
            Some(false) => true,
            Some(true) => materialized_here,
            None => materialized_here,
        };
        if forced_required {
            let key = key.unwrap_or_else(|| name.clone());
            let cached = self.values.borrow().get(&key).cloned();
            let current = match cached {
                Some(value) => value,
                None => self.eval_field_key(&key, Span::default())?,
            };
            let merged = unify_values(&current, &value, Span::default())?;
            self.values.borrow_mut().insert(key, merged);
            self.materialized.borrow_mut().insert(name);
        } else {
            merge_generated_optional(&self.generated_optionals, &name, value)?;
        }
        Ok(())
    }

    fn expand_body(
        &self,
        body: &[StructItem],
        bindings: HashMap<String, Value>,
    ) -> Result<(), LangError> {
        let child = Frame::new(body, Some(self), bindings)?;
        let value = child.to_struct_value_mode(true)?;
        let Value::Definition {
            fields: values,
            definitions,
            ..
        } = value
        else {
            return Ok(());
        };
        for (key, value) in values {
            let value = match self.generated_optionals.borrow_mut().remove(&key) {
                Some(constraint) => unify_values(&value, &constraint, Span::default())?,
                None => value,
            };
            let storage_key = self
                .display_storage_key(&key)
                .unwrap_or_else(|| key.clone());
            let existing = self.values.borrow().get(&storage_key).cloned();
            let existing = match existing {
                Some(existing) => Some(existing),
                None if self.fields.borrow().contains_key(&storage_key) => {
                    Some(self.eval_field_key(&storage_key, Span::default())?)
                }
                None => None,
            };
            let merged = match existing {
                Some(existing) => unify_values(&existing, &value, Span::default())?,
                None => value,
            };
            self.materialized.borrow_mut().insert(key.clone());
            self.values.borrow_mut().insert(storage_key, merged);
        }
        for (key, value) in definitions {
            let prior = self.generated_definitions.borrow().get(&key).cloned();
            let declared = if prior.is_none()
                && self
                    .fields
                    .borrow()
                    .get(&key)
                    .is_some_and(|definition| definition.definition)
            {
                Some(self.eval_field_key(&key, Span::default())?)
            } else {
                None
            };
            let merged = match (prior, declared) {
                (Some(prior), _) => merge_declaration(&prior, &value, Span::default())?,
                (None, Some(declared)) => merge_declaration(&declared, &value, Span::default())?,
                (None, None) => value,
            };
            self.generated_definitions
                .borrow_mut()
                .insert(key.clone(), merged.clone());
            if self.values.borrow().contains_key(&key) {
                self.values.borrow_mut().insert(key.clone(), merged);
            }
            self.materialized.borrow_mut().insert(key);
        }
        let optional_names: Vec<String> = child
            .fields
            .borrow()
            .iter()
            .filter(|(_, definition)| definition.optional && !definition.definition)
            .map(|(name, _)| name.clone())
            .collect();
        for name in optional_names {
            let value = child.eval_field_key(&name, Span::default())?;
            self.merge_generated_constraint(name, value)?;
        }
        let generated: Vec<(String, Value)> = child
            .generated_optionals
            .borrow()
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        for (name, value) in generated {
            self.merge_generated_constraint(name, value)?;
        }
        Ok(())
    }

    fn lookup(&self, name: &str, span: Span) -> Result<Value, LangError> {
        if let Some(value) = self.vars.borrow().get(name) {
            return Ok(value.clone());
        }
        if self.lets.borrow().contains_key(name) {
            // A `let` is not in scope in its own value: a same-named
            // identifier in the RHS resolves to an enclosing let or alias.
            if self.evaluating.borrow().contains(name)
                && self.self_rhs.borrow().as_deref() == Some(name)
            {
                if let Some(parent) = self.parent {
                    return parent.lookup(name, span);
                }
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("undefined reference `{name}`"),
                ));
            }
            return self.eval_let(name, span);
        }
        if let Some(target) = self.aliases.borrow().get(name).cloned() {
            let key = if target.hidden {
                let hidden = hidden_field(&target.name);
                self.fields.borrow().contains_key(&hidden).then_some(hidden)
            } else if target.definition {
                self.stored_field_key(&target.name)
            } else {
                self.field_storage_key(&target.name)
            };
            let Some(key) = key else {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("undefined reference `{}`", target.name),
                ));
            };
            let optional = self
                .fields
                .borrow()
                .get(&key)
                .is_some_and(|definition| definition.optional);
            if optional && !self.optional_is_materialized(&key) {
                if self.defer_incomplete.get() {
                    return Ok(Value::Bottom);
                }
                return Err(LangError::incomplete_optional(
                    span.line,
                    span.col,
                    format!("cannot reference optional field `{}`", target.name),
                ));
            }
            return self.eval_field_key(&key, span);
        }
        if let Some(key) = self.bindable_field_key(name) {
            let optional = self
                .fields
                .borrow()
                .get(&key)
                .is_some_and(|definition| definition.optional);
            if optional && !self.optional_is_materialized(&key) {
                if self.defer_incomplete.get() {
                    return Ok(Value::Bottom);
                }
                return Err(LangError::incomplete_optional(
                    span.line,
                    span.col,
                    format!("cannot reference optional field `{name}`"),
                ));
            }
            return self.eval_field_key(&key, span);
        }
        if !self.fields.borrow().contains_key(name)
            && let Some(value) = self.values.borrow().get(name)
        {
            return Ok(value.clone());
        }
        if let Some(parent) = self.parent
            && parent.has_name(name)
        {
            return parent.lookup(name, span);
        }
        match name {
            "string" => Ok(Value::Type(TypeKind::String)),
            "int" => Ok(Value::Type(TypeKind::Int)),
            "float" => Ok(Value::Type(TypeKind::Float)),
            "bytes" => Ok(Value::Type(TypeKind::Bytes)),
            "number" => Ok(Value::Type(TypeKind::Number)),
            "bool" => Ok(Value::Type(TypeKind::Bool)),
            "_" => Ok(Value::Bottom),
            "len" => Ok(Value::Builtin(Builtin::Len)),
            _ => Err(LangError::eval(
                span.line,
                span.col,
                format!("undefined reference `{name}`"),
            )),
        }
    }

    fn has_local_name(&self, name: &str) -> bool {
        self.bindable_field_key(name).is_some()
            || self.vars.borrow().contains_key(name)
            || self.lets.borrow().contains_key(name)
            || self.aliases.borrow().contains_key(name)
            || (!self.fields.borrow().contains_key(name) && self.values.borrow().contains_key(name))
    }

    fn has_name(&self, name: &str) -> bool {
        self.bindable_field_key(name).is_some()
            || self.vars.borrow().contains_key(name)
            || self.lets.borrow().contains_key(name)
            || self.aliases.borrow().contains_key(name)
            || (!self.fields.borrow().contains_key(name) && self.values.borrow().contains_key(name))
            || self.parent.is_some_and(|parent| parent.has_name(name))
    }

    fn eval_field(&self, name: &str, span: Span) -> Result<Value, LangError> {
        let Some(key) = self.bindable_field_key(name) else {
            return Err(LangError::eval(
                span.line,
                span.col,
                format!("undefined reference `{name}`"),
            ));
        };
        self.eval_field_key(&key, span)
    }

    fn root_frame(&self) -> &Frame<'_> {
        let mut frame = self;
        while let Some(parent) = frame.parent {
            frame = parent;
        }
        frame
    }

    fn params_shadowed(&self) -> bool {
        let mut frame = Some(self);
        while let Some(current) = frame {
            if current.parent.is_some()
                && (current.fields.borrow().contains_key("params")
                    || current.lets.borrow().contains_key("params")
                    || current.aliases.borrow().contains_key("params")
                    || current.vars.borrow().contains_key("params"))
            {
                return true;
            }
            if current.params_scope.get() {
                break;
            }
            frame = current.parent;
        }
        false
    }

    fn nearest_params_prefix(&self) -> Vec<String> {
        let mut cursor = Some(self);
        while let Some(current) = cursor {
            let prefix = current.params_prefix.borrow().clone();
            if !prefix.is_empty() {
                return prefix;
            }
            cursor = current.parent;
        }
        Vec::new()
    }

    fn within_params(&self) -> bool {
        let mut cursor = Some(self);
        while let Some(current) = cursor {
            if current.params_scope.get() {
                return true;
            }
            cursor = current.parent;
        }
        false
    }

    fn optional_is_materialized(&self, key: &str) -> bool {
        let materialized = self.materialized.borrow();
        materialized.contains(key) || materialized.contains(display_field_name(key))
    }

    fn is_injected_key(&self, key: &str) -> bool {
        let injected = self.injected.borrow();
        injected.contains(key) || injected.contains(display_field_name(key))
    }

    fn injected_field_reference(&self, name: &str) -> bool {
        let mut cursor = Some(self);
        while let Some(current) = cursor {
            if let Some(key) = current.any_field_key(name) {
                return current.is_injected_key(&key);
            }
            cursor = current.parent;
        }
        false
    }

    /// Evaluate a field's own declaration without honoring a cached value that
    /// came from external injection. Used by self-selection, which CUE always
    /// resolves against the definition in isolation.
    fn eval_field_key_without_injection(&self, key: &str, span: Span) -> Result<Value, LangError> {
        let cached = self.values.borrow_mut().remove(key);
        let previous_cache = self.skip_cache.replace(true);
        let result = self.eval_field_key(key, span);
        self.skip_cache.set(previous_cache);
        if let Some(cached) = cached {
            self.values.borrow_mut().insert(key.to_owned(), cached);
        }
        result
    }

    /// Re-evaluate an incomplete field with soft incompleteness errors deferred.
    /// Used before skipping definitions/hidden fields during export so hard
    /// errors hidden underneath the first incomplete error still surface.
    fn incomplete_field_validates(&self, key: &str, span: Span) -> Result<(), LangError> {
        self.values.borrow_mut().remove(key);
        let previous_defer = self.defer_incomplete.replace(true);
        let previous_cache = self.skip_cache.replace(true);
        let result = self.eval_field_key(key, span);
        self.skip_cache.set(previous_cache);
        self.defer_incomplete.set(previous_defer);
        result.map(|_| ())
    }

    fn generated_value_key(&self, name: &str, quoted: bool) -> Option<String> {
        let values = self.values.borrow();
        if quoted {
            [escaped_field(name), prefixed_field(name), name.to_owned()]
                .into_iter()
                .find(|key| {
                    !is_hidden_key(key)
                        && values.contains_key(key)
                        && !self
                            .fields
                            .borrow()
                            .get(key)
                            .is_some_and(|definition| definition.definition)
                })
        } else if name.starts_with('_') {
            let key = hidden_field(name);
            values.contains_key(&key).then_some(key)
        } else if name.starts_with('#') {
            None
        } else {
            values.contains_key(name).then(|| name.to_owned())
        }
    }

    fn params_path_local(
        &self,
        path: &[(String, bool)],
        span: Span,
    ) -> Option<Result<Value, LangError>> {
        let ((first, quoted), rest) = path.split_first()?;
        let mut frame = Some(self);
        let mut value = None;
        while let Some(current) = frame {
            let current_prefix = current.params_prefix.borrow().clone();
            if !current_prefix.is_empty() {
                let matches_prefix = path.len() > current_prefix.len()
                    && path[..current_prefix.len()]
                        .iter()
                        .map(|(segment, _)| segment.as_str())
                        .eq(current_prefix.iter().map(String::as_str));
                if matches_prefix {
                    let suffix = &path[current_prefix.len()..];
                    if let Some(result) = current.resolve_local_segment(suffix, span) {
                        return Some(result);
                    }
                    if suffix.len() > 1 {
                        return self.resolve_local_segment(&suffix[1..], span);
                    }
                }
                frame = current.parent;
                continue;
            }
            if let Some(key) = current.segment_field_key(first, *quoted) {
                if current.evaluating.borrow().contains(&key) {
                    return self.resolve_local_segment(rest, span);
                }
                match current.eval_field_key(&key, span) {
                    Ok(found) => value = Some(found),
                    Err(err) => return Some(Err(err)),
                }
                break;
            }
            if let Some(key) = current.generated_value_key(first, *quoted)
                && let Some(mut found) = current.values.borrow().get(&key).cloned()
            {
                for (field, quoted) in rest {
                    match select_value(&found, field, *quoted, span) {
                        Ok(next) => found = next,
                        Err(err) => return Some(Err(err)),
                    }
                }
                return Some(Ok(found));
            }
            frame = current.parent;
        }
        let Some(mut value) = value else {
            if !self.params_prefix.borrow().is_empty() {
                return None;
            }
            return self.resolve_local_segment(path, span);
        };
        for (field, quoted) in rest {
            match select_value(&value, field, *quoted, span) {
                Ok(next) => value = next,
                Err(err) => return Some(Err(err)),
            }
        }
        Some(Ok(value))
    }

    fn resolve_local_segment(
        &self,
        rest: &[(String, bool)],
        span: Span,
    ) -> Option<Result<Value, LangError>> {
        let ((first, quoted), tail) = rest.split_first()?;
        if self.segment_field_key(first, *quoted).is_none()
            && self.generated_value_key(first, *quoted).is_none()
            && !tail.is_empty()
        {
            return self.resolve_local_segment(tail, span);
        }
        let mut value = match self.segment_field_key(first, *quoted) {
            Some(key) => {
                if self.evaluating.borrow().contains(&key)
                    && !self.values.borrow().contains_key(&key)
                {
                    return None;
                }
                match self.eval_field_key(&key, span) {
                    Ok(value) => value,
                    Err(err) => return Some(Err(err)),
                }
            }
            None => {
                let key = self.generated_value_key(first, *quoted)?;
                self.values.borrow().get(&key).cloned()?
            }
        };
        for (field, quoted) in tail {
            match select_value(&value, field, *quoted, span) {
                Ok(next) => value = next,
                Err(err) => return Some(Err(err)),
            }
        }
        Some(Ok(value))
    }

    fn segment_field_key(&self, name: &str, quoted: bool) -> Option<String> {
        if !quoted {
            return self.any_field_key(name);
        }
        let fields = self.fields.borrow();
        let escaped = escaped_field(name);
        if fields.contains_key(&escaped) {
            return Some(escaped);
        }
        if fields
            .get(name)
            .is_some_and(|definition| !definition.definition && !is_hidden_key(name))
        {
            return Some(name.to_owned());
        }
        let prefixed = prefixed_field(name);
        if fields
            .get(&prefixed)
            .is_some_and(|definition| !definition.definition)
        {
            return Some(prefixed);
        }
        None
    }

    fn params_path_value(&self, path: &[(String, bool)], span: Span) -> Result<Value, LangError> {
        let Some(context) = self.params_context.borrow().clone() else {
            return Err(LangError::eval(
                span.line,
                span.col,
                "params are not being evaluated",
            ));
        };
        let cache_key = params_cache_key(path);
        if let Some(value) = context.values.borrow().get(&cache_key) {
            return Ok(value.clone());
        }
        if !context.local.borrow().is_empty()
            && !context.evaluating.borrow().contains(&cache_key)
            && !context.local.borrow().contains(&cache_key)
        {
            context.local.borrow_mut().insert(cache_key.clone());
            let local = self.params_path_local(path, span);
            context.local.borrow_mut().remove(&cache_key);
            match local {
                Some(Ok(value)) => return Ok(value),
                Some(Err(err)) => return Err(err),
                None => {}
            }
        }
        if context.evaluating.borrow().contains(&cache_key) {
            if !context.local.borrow().contains(&cache_key) {
                context.local.borrow_mut().insert(cache_key.clone());
                let local = self.params_path_local(path, span);
                context.local.borrow_mut().remove(&cache_key);
                if let Some(local) = local {
                    return local;
                }
            }
            return Err(LangError::eval(
                span.line,
                span.col,
                format!(
                    "cyclic reference while evaluating `params.{}`",
                    path.iter()
                        .map(|(segment, _)| segment.as_str())
                        .collect::<Vec<_>>()
                        .join(".")
                ),
            ));
        }
        context.evaluating.borrow_mut().insert(cache_key.clone());
        let result = (|| {
            let root = self.root_frame();
            let mut items = context.items.clone();
            let mut bindings: HashMap<String, Value> = HashMap::new();
            for (index, (segment, quoted)) in path.iter().enumerate() {
                let child = Frame::from_static(&items, Some(root), HashMap::new())?;
                child.params_scope.set(true);
                *child.params_prefix.borrow_mut() = cache_key[..index].to_vec();
                if index == 0 {
                    for (key, value) in &context.overrides {
                        child.values.borrow_mut().insert(key.clone(), value.clone());
                    }
                }
                for (name, value) in &bindings {
                    let shadowed = child.fields.borrow().contains_key(name)
                        || child.lets.borrow().contains_key(name)
                        || child.aliases.borrow().contains_key(name);
                    if !shadowed {
                        child.vars.borrow_mut().insert(name.clone(), value.clone());
                    }
                }
                child.expand(&items)?;
                child.reconcile_generated_required_fields()?;
                let field_key = child.segment_field_key(segment, *quoted);
                let overridden = index == 0 && context.overrides.contains_key(segment);
                if let Some(key) = &field_key {
                    let optional = child
                        .fields
                        .borrow()
                        .get(key)
                        .is_some_and(|definition| definition.optional);
                    if optional
                        && !overridden
                        && !child.materialized.borrow().contains(key)
                        && !child.values.borrow().contains_key(key)
                    {
                        if child.defer_incomplete.get() {
                            return Ok(Value::Bottom);
                        }
                        return Err(LangError::incomplete_optional(
                            span.line,
                            span.col,
                            format!("cannot reference optional field `{segment}`"),
                        ));
                    }
                }
                let is_final = index + 1 == path.len();
                if overridden {
                    let value = context
                        .overrides
                        .get(segment)
                        .cloned()
                        .unwrap_or(Value::Bottom);
                    if is_final {
                        return Ok(value);
                    }
                    let mut value = value;
                    for (field, quoted) in &path[index + 1..] {
                        value = select_value(&value, field, *quoted, span)?;
                    }
                    return Ok(value);
                }
                if let Some(key) = field_key {
                    if is_final {
                        return child.eval_field_key(&key, span);
                    }
                    let struct_items = {
                        let exprs = child
                            .fields
                            .borrow()
                            .get(&key)
                            .map(|definition| definition.exprs.clone())
                            .unwrap_or_default();
                        let mut out = Vec::new();
                        let mut seen = HashSet::new();
                        let mut complete = true;
                        for expr in &exprs {
                            if child
                                .collect_struct_items(expr, &mut out, &mut seen, 0)
                                .is_none()
                            {
                                complete = false;
                                break;
                            }
                        }
                        if complete && !out.is_empty() {
                            Some(out)
                        } else {
                            None
                        }
                    };
                    if let Some(next_items) = struct_items {
                        for item in &items {
                            match item {
                                StructItem::Field(field) => {
                                    if let crate::ast::Label::Name(name, _) = &field.label
                                        && name != segment
                                        && !field.quoted
                                        && !name.starts_with('_')
                                        && let Some(sibling_key) = child.any_field_key(name)
                                    {
                                        let optional = child
                                            .fields
                                            .borrow()
                                            .get(&sibling_key)
                                            .is_some_and(|definition| definition.optional);
                                        if !optional
                                            && let Ok(value) =
                                                child.eval_field_key(&sibling_key, span)
                                        {
                                            bindings.insert(name.clone(), value);
                                        }
                                    }
                                }
                                StructItem::Let { name, .. } => {
                                    if !bindings.contains_key(name)
                                        && let Ok(value) = child.eval_let(name, span)
                                    {
                                        bindings.insert(name.clone(), value);
                                    }
                                }
                                _ => {}
                            }
                        }
                        items = next_items;
                        continue;
                    }
                    let mut value = child.eval_field_key(&key, span)?;
                    for (field, quoted) in &path[index + 1..] {
                        value = select_value(&value, field, *quoted, span)?;
                    }
                    return Ok(value);
                }
                let generated = child
                    .generated_value_key(segment, *quoted)
                    .and_then(|key| child.values.borrow().get(&key).cloned());
                let Some(mut value) = generated else {
                    return Err(LangError::eval(
                        span.line,
                        span.col,
                        format!("field `{segment}` not found in params"),
                    ));
                };
                if is_final {
                    return Ok(value);
                }
                for (field, quoted) in &path[index + 1..] {
                    value = select_value(&value, field, *quoted, span)?;
                }
                return Ok(value);
            }
            Err(LangError::eval(span.line, span.col, "empty params path"))
        })();
        context.evaluating.borrow_mut().remove(&cache_key);
        let value = result?;
        context.values.borrow_mut().insert(cache_key, value.clone());
        Ok(value)
    }

    fn allows_incomplete(&self, expr: &Expr) -> bool {
        match &*self.incomplete_mode.borrow() {
            IncompleteMode::Forbid => false,
            IncompleteMode::AllowAll => true,
            IncompleteMode::AllowOnly(allowed) => {
                let mut seen = HashSet::new();
                self.expr_reaches_task_output(expr, allowed, &mut seen)
            }
        }
    }

    fn expr_reaches_task_output(
        &self,
        expr: &Expr,
        allowed: &HashSet<String>,
        seen: &mut HashSet<String>,
    ) -> bool {
        match expr {
            Expr::Select(base, _, _, _) => {
                // Any field of a scheduled task can be a task output: besides
                // `stdout`/`stderr`/`success`, packages such as
                // `tool/os.Getenv`/`Environ` export user-named fields.
                match unwrap_interp_base(base) {
                    Expr::Ident(name, _) if allowed.contains(name) => return true,
                    nested => {
                        // Nested task declarations (`a: producer: exec.Run`)
                        // are scheduled under their leaf name.
                        if selector_leaf_name(nested).is_some_and(|leaf| allowed.contains(&leaf)) {
                            return true;
                        }
                        if self
                            .ident_task_label(nested, &mut HashSet::new())
                            .is_some_and(|label| allowed.contains(&label))
                        {
                            return true;
                        }
                    }
                }
                self.expr_reaches_task_output(base, allowed, seen)
            }
            Expr::Ident(name, _) => {
                let local = if let Some(value) = self.lets.borrow().get(name).cloned() {
                    Some(vec![value])
                } else {
                    let fields = self.fields.borrow();
                    let prefixed = prefixed_field(name);
                    fields
                        .get(name)
                        .or_else(|| fields.get(&prefixed))
                        .map(|definition| definition.exprs.clone())
                };
                match local {
                    Some(exprs) => {
                        if !seen.insert(name.clone()) {
                            return false;
                        }
                        let result = exprs
                            .iter()
                            .any(|expr| self.expr_reaches_task_output(expr, allowed, seen));
                        seen.remove(name);
                        result
                    }
                    None => self
                        .parent
                        .is_some_and(|parent| parent.expr_reaches_task_output(expr, allowed, seen)),
                }
            }
            Expr::Unify(left, right, _)
            | Expr::Disjunction(left, right, _)
            | Expr::Binary(_, left, right, _) => {
                self.expr_reaches_task_output(left, allowed, seen)
                    || self.expr_reaches_task_output(right, allowed, seen)
            }
            Expr::Bound(_, inner, _)
            | Expr::Default(inner, _)
            | Expr::Paren(inner, _)
            | Expr::Unary(_, inner, _) => self.expr_reaches_task_output(inner, allowed, seen),
            Expr::Index(base, index, _) => {
                self.expr_reaches_task_output(base, allowed, seen)
                    || self.expr_reaches_task_output(index, allowed, seen)
            }
            Expr::Call(callee, arguments, _) => {
                self.expr_reaches_task_output(callee, allowed, seen)
                    || arguments
                        .iter()
                        .any(|argument| self.expr_reaches_task_output(argument, allowed, seen))
            }
            Expr::Str(parts, _) => parts.iter().any(|part| match part {
                StrPart::Interp(inner) => self.expr_reaches_task_output(inner, allowed, seen),
                StrPart::Text(_) => false,
            }),
            Expr::BytesInterp(parts, _) => parts.iter().any(|part| match part {
                crate::ast::BytePart::Interp(inner) => {
                    self.expr_reaches_task_output(inner, allowed, seen)
                }
                crate::ast::BytePart::Text(_) => false,
            }),
            Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => items
                .iter()
                .any(|item| self.expr_reaches_task_output(item, allowed, seen)),
            Expr::OpenList(Some(inner), _) => self.expr_reaches_task_output(inner, allowed, seen),
            Expr::ListComp { body, .. } => self.expr_reaches_task_output(body, allowed, seen),
            Expr::Struct(items, _) => items
                .iter()
                .any(|item| self.item_reaches_task_output(item, allowed, seen)),
            _ => false,
        }
    }

    fn item_reaches_task_output(
        &self,
        item: &StructItem,
        allowed: &HashSet<String>,
        seen: &mut HashSet<String>,
    ) -> bool {
        match item {
            StructItem::Field(field) => self.expr_reaches_task_output(&field.value, allowed, seen),
            StructItem::Let { value, .. } | StructItem::Pattern { value, .. } => {
                self.expr_reaches_task_output(value, allowed, seen)
            }
            StructItem::For { source, body, .. } => {
                self.expr_reaches_task_output(source, allowed, seen)
                    || body
                        .iter()
                        .any(|item| self.item_reaches_task_output(item, allowed, seen))
            }
            StructItem::If { cond, body, .. } => {
                self.expr_reaches_task_output(cond, allowed, seen)
                    || body
                        .iter()
                        .any(|item| self.item_reaches_task_output(item, allowed, seen))
            }
            StructItem::Ellipsis(_) => false,
        }
    }

    fn eval_field_key(&self, key: &str, span: Span) -> Result<Value, LangError> {
        let name = display_field_name(key);
        if let Some(value) = self.values.borrow().get(key) {
            return Ok(value.clone());
        }
        if let Some(value) = self.generated_definitions.borrow().get(key).cloned() {
            return Ok(value);
        }
        let (exprs, is_definition) = {
            let fields = self.fields.borrow();
            let Some(def) = fields.get(key) else {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("undefined reference `{name}`"),
                ));
            };
            (def.exprs.clone(), def.definition)
        };
        if self.evaluating.borrow().contains(key) {
            if is_definition {
                let slot = self
                    .recursive_handles
                    .borrow_mut()
                    .entry(key.to_owned())
                    .or_insert_with(|| Rc::new(RecursiveSlot::new()))
                    .clone();
                return Ok(Value::Recursive(slot));
            }
            return Err(LangError::eval(
                span.line,
                span.col,
                format!("cyclic reference while evaluating `{name}`"),
            ));
        }
        if name == "params" {
            let items = self.params_items(&exprs);
            if items.is_some() || !self.overrides.borrow().is_empty() {
                if self.evaluating.borrow().contains(key) {
                    return Err(LangError::eval(
                        span.line,
                        span.col,
                        format!("cyclic reference while evaluating `{name}`"),
                    ));
                }
                self.evaluating.borrow_mut().insert(key.to_owned());
                let result = match items {
                    Some(items) => self.eval_params_struct(&items, &exprs, span),
                    None => Err(LangError::eval(
                        span.line,
                        span.col,
                        "cannot apply parameter overrides to this `params` shape",
                    )),
                };
                self.evaluating.borrow_mut().remove(key);
                let result = result?;
                self.values
                    .borrow_mut()
                    .insert(key.to_owned(), result.clone());
                return Ok(result);
            }
        }
        if is_definition {
            let slot = self
                .recursive_handles
                .borrow_mut()
                .entry(key.to_owned())
                .or_insert_with(|| Rc::new(RecursiveSlot::new()))
                .clone();
            self.evaluating.borrow_mut().insert(key.to_owned());
            let previous_next = self
                .next_definition
                .replace(Some(display_field_name(key).to_owned()));
            let mut flattened = false;
            let result = if let Some(items) = self.params_items(&exprs) {
                flattened = true;
                self.eval_definition_items(&items, span)
            } else if exprs.len() == 1 {
                self.eval_definition_expr(&exprs[0], span)
            } else {
                let mut result = Value::Bottom;
                let mut error = None;
                for expr in &exprs {
                    match self.eval_definition_part(expr) {
                        Ok(value) => {
                            result = if matches!(result, Value::Bottom) {
                                value
                            } else {
                                match unify_values(&result, &value, span) {
                                    Ok(value) => value,
                                    Err(err) => {
                                        error = Some(err);
                                        break;
                                    }
                                }
                            };
                        }
                        Err(err) => {
                            error = Some(err);
                            break;
                        }
                    }
                }
                match error {
                    Some(err) => Err(err),
                    None => Ok(close_composed_definition(&result)),
                }
            };
            self.next_definition.replace(previous_next);
            self.evaluating.borrow_mut().remove(key);
            let mut result = result?;
            let mut references = Vec::new();
            if flattened {
                let mut seen = HashSet::new();
                for expr in &exprs {
                    self.collect_definition_references(expr, &mut references, &mut seen);
                }
            }
            for reference in references {
                if reference == name {
                    continue;
                }
                match self.eval_field(&reference, span) {
                    Ok(value) => {
                        result = unify_values(&result, &value, span)?;
                    }
                    Err(err) if is_deferrable_incomplete_error(&err) => {}
                    Err(err) => return Err(err),
                }
            }
            *slot.value.borrow_mut() = Some(result.clone());
            if !self.skip_cache.get() {
                self.values
                    .borrow_mut()
                    .insert(key.to_owned(), result.clone());
            }
            return Ok(result);
        }
        self.evaluating.borrow_mut().insert(key.to_owned());
        let previous_next = self
            .next_definition
            .replace(Some(display_field_name(key).to_owned()));
        let segment = self
            .within_params()
            .then(|| display_field_name(key).to_owned());
        let outcome = (|| -> Result<Value, LangError> {
            let mut evaluated: Vec<Option<Value>> = Vec::with_capacity(exprs.len());
            let mut deferred: Option<LangError> = None;
            for expr in &exprs {
                let previous = segment
                    .as_ref()
                    .map(|segment| self.params_segment.replace(Some(segment.clone())));
                let value = eval_expr(expr, self);
                if let Some(previous) = previous {
                    *self.params_segment.borrow_mut() = previous;
                }
                match value {
                    Ok(value) => evaluated.push(Some(value)),
                    Err(err) if is_deferrable_incomplete_error(&err) => {
                        if deferred.is_none() {
                            deferred = Some(err);
                        }
                        evaluated.push(None);
                    }
                    Err(err) => return Err(err),
                }
            }
            let container = |value: &Value| -> Option<Value> {
                if !value_is_plain_concrete(value) {
                    return None;
                }
                match value.concretize() {
                    Value::Struct(_)
                    | Value::Definition { .. }
                    | Value::List(_)
                    | Value::OpenListPrefix(..) => Some(value.clone()),
                    _ => None,
                }
            };
            let mut candidates: Vec<Value> = Vec::new();
            for (index, expr) in exprs.iter().enumerate() {
                if !expr_is_narrowing_source(expr) {
                    continue;
                }
                if let Some(value) = evaluated[index].as_ref().and_then(container)
                    && !candidates.contains(&value)
                {
                    candidates.push(value);
                }
            }
            for value in evaluated.iter().flatten() {
                if let Some(value) = container(value)
                    && !candidates.contains(&value)
                {
                    candidates.push(value);
                }
            }
            if exprs.len() > 1 {
                // Re-evaluate every declaration against a concrete sibling.
                // This narrows nested defaults while still letting the normal
                // unification below reject genuine conflicts.
                for injection in &candidates {
                    let mut trial = Vec::with_capacity(exprs.len());
                    let mut complete = true;
                    for expr in &exprs {
                        let previous = segment
                            .as_ref()
                            .map(|segment| self.params_segment.replace(Some(segment.clone())));
                        let value = eval_declaration_part(self, expr, injection, span);
                        if let Some(previous) = previous {
                            *self.params_segment.borrow_mut() = previous;
                        }
                        match value {
                            Ok(value) => trial.push(value),
                            Err(_) => {
                                complete = false;
                                break;
                            }
                        }
                    }
                    if !complete {
                        continue;
                    }
                    let mut result = Value::Bottom;
                    let mut unified = true;
                    for (index, value) in trial.into_iter().enumerate() {
                        if index == 0 {
                            result = value;
                        } else {
                            match unify_values(&result, &value, span) {
                                Ok(value) => result = value,
                                Err(_) => {
                                    unified = false;
                                    break;
                                }
                            }
                        }
                    }
                    if unified {
                        return Ok(result);
                    }
                }
            }
            if evaluated.iter().any(Option::is_none) {
                // A sibling declaration in the same field supplied a concrete
                // struct/list. Re-evaluate the incomplete declaration against
                // it so nested optionals see the concrete values.
                let Some(injection) = candidates.first() else {
                    return Err(deferred.unwrap_or_else(|| {
                        LangError::eval(span.line, span.col, "incomplete value")
                    }));
                };
                for (index, expr) in exprs.iter().enumerate() {
                    if evaluated[index].is_some() {
                        continue;
                    }
                    let previous = segment
                        .as_ref()
                        .map(|segment| self.params_segment.replace(Some(segment.clone())));
                    let value = eval_declaration_part(self, expr, injection, span);
                    if let Some(previous) = previous {
                        *self.params_segment.borrow_mut() = previous;
                    }
                    evaluated[index] = Some(value?);
                }
            }
            let mut result = Value::Bottom;
            for (index, value) in evaluated.into_iter().enumerate() {
                let Some(value) = value else {
                    continue;
                };
                result = if index == 0 {
                    value
                } else {
                    unify_values(&result, &value, span)?
                };
            }
            Ok(result)
        })();
        self.next_definition.replace(previous_next);
        let result = outcome;
        self.evaluating.borrow_mut().remove(key);
        let result = result?;
        if !self.skip_cache.get() {
            self.values
                .borrow_mut()
                .insert(key.to_owned(), result.clone());
        }
        Ok(result)
    }

    fn collect_definition_references(
        &self,
        expr: &Expr,
        out: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) {
        match expr {
            Expr::Ident(name, _) if name.starts_with('#') => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Expr::Ident(name, _) => {
                if !seen.insert(name.clone()) {
                    return;
                }
                if let Some(value) = self.lets.borrow().get(name).cloned() {
                    self.collect_definition_references(&value, out, seen);
                } else {
                    let exprs = {
                        let fields = self.fields.borrow();
                        let prefixed = prefixed_field(name);
                        fields
                            .get(name)
                            .or_else(|| fields.get(&prefixed))
                            .map(|definition| definition.exprs.clone())
                    };
                    if let Some(exprs) = exprs {
                        for expr in &exprs {
                            self.collect_definition_references(expr, out, seen);
                        }
                    }
                }
                seen.remove(name);
            }
            Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
                self.collect_definition_references(left, out, seen);
                self.collect_definition_references(right, out, seen);
            }
            Expr::Paren(inner, _) | Expr::Default(inner, _) => {
                self.collect_definition_references(inner, out, seen);
            }
            _ => {}
        }
    }

    fn eval_definition_items(&self, items: &[StructItem], span: Span) -> Result<Value, LangError> {
        self.eval_struct_items(items, span, true)
    }

    fn eval_struct_items(
        &self,
        items: &[StructItem],
        span: Span,
        closed_by_default: bool,
    ) -> Result<Value, LangError> {
        let child = Frame::from_static(items, Some(self), HashMap::new())?;
        *child.definition_context.borrow_mut() = self.next_definition.borrow().clone();
        let pending_injections = self.next_injections.borrow_mut().take();
        if let Some(injections) = pending_injections {
            // Seed the concrete injected values first so that dependent fields
            // resolve regardless of declaration order. Declarations are still
            // evaluated and unified below, so type conflicts remain errors.
            for (name, value) in &injections {
                if let Some(key) = child.any_field_key(name) {
                    let is_definition = child
                        .fields
                        .borrow()
                        .get(&key)
                        .is_some_and(|definition| definition.definition);
                    if !is_definition {
                        child.values.borrow_mut().insert(key.clone(), value.clone());
                        child.materialized.borrow_mut().insert(key.clone());
                        child.injected.borrow_mut().insert(key.clone());
                        child
                            .materialized
                            .borrow_mut()
                            .insert(display_field_name(&key).to_owned());
                    }
                }
            }
            for (name, value) in injections {
                let Some(key) = child.any_field_key(&name) else {
                    continue;
                };
                let exprs = {
                    let fields = child.fields.borrow();
                    let Some(field) = fields.get(&key) else {
                        continue;
                    };
                    if field.definition {
                        continue;
                    }
                    field.exprs.clone()
                };
                let mut declared = Value::Bottom;
                for (index, expr) in exprs.iter().enumerate() {
                    let part = eval_declaration_part(&child, expr, &value, span)?;
                    declared = if index == 0 {
                        part
                    } else {
                        unify_values(&declared, &part, span)?
                    };
                }
                let declared = if closed_by_default {
                    close_nested(&declared)
                } else {
                    declared
                };
                let unified = unify_values(&declared, &value, span)?;
                child.values.borrow_mut().insert(key.clone(), unified);
                child.materialized.borrow_mut().insert(key.clone());
                child
                    .materialized
                    .borrow_mut()
                    .insert(display_field_name(&key).to_owned());
            }
        }
        if let Some(segment) = self.params_segment.borrow().clone() {
            let mut prefix = self.nearest_params_prefix();
            prefix.push(segment);
            *child.params_prefix.borrow_mut() = prefix;
        }
        child.closed.set(closed_by_default);
        child.expand(items)?;
        child.reconcile_generated_required_fields()?;
        let mut names: Vec<String> = child.fields.borrow().keys().cloned().collect();
        names.extend(child.values.borrow().keys().cloned());
        names.extend(child.generated_optionals.borrow().keys().cloned());
        names.extend(child.generated_definitions.borrow().keys().cloned());
        names.sort();
        names.dedup();
        let mut fields = BTreeMap::new();
        let mut optional = BTreeMap::new();
        let mut definitions = BTreeMap::new();
        for name in names {
            if is_hidden_key(&name) {
                let declared_optional = child
                    .fields
                    .borrow()
                    .get(&name)
                    .is_some_and(|field| field.optional)
                    || child.generated_optionals.borrow().contains_key(&name);
                let materialized = child.materialized.borrow().contains(&name);
                if !declared_optional || materialized {
                    let value = child.eval_field_key(&name, span)?;
                    fields.insert(name, value);
                }
                continue;
            }
            let display = output_field_name(&name);
            if let Some(value) = child.generated_definitions.borrow().get(&name).cloned() {
                definitions.insert(display, value);
                continue;
            }
            let defined = child.fields.borrow().get(&name).cloned();
            if defined.as_ref().is_some_and(|field| field.definition) {
                let is_optional = defined.as_ref().is_some_and(|field| field.optional);
                if !is_optional {
                    definitions.insert(display.clone(), child.eval_field_key(&name, span)?);
                }
                if let Some(constraint) = child.generated_optionals.borrow_mut().remove(&display) {
                    optional.insert(display, constraint);
                }
                continue;
            }
            let declared_optional = defined.as_ref().is_some_and(|field| field.optional);
            let generated = child.generated_optionals.borrow().get(&display).cloned();
            let materialized = child.materialized.borrow().contains(&display);
            let optional_field = !materialized && (declared_optional || generated.is_some());
            if optional_field {
                let mut value = if child.fields.borrow().contains_key(&name) {
                    match child.eval_field_key(&name, span) {
                        Ok(value) => value,
                        Err(err) => Value::Conflict(err.to_string()),
                    }
                } else {
                    generated
                        .clone()
                        .unwrap_or_else(|| Value::Conflict("missing optional value".to_owned()))
                };
                if let Some(constraint) = generated {
                    value = match unify_values(&value, &constraint, span) {
                        Ok(value) => value,
                        Err(err) => Value::Conflict(err.to_string()),
                    };
                }
                child.values.borrow_mut().remove(&name);
                optional.insert(display, value);
            } else {
                let mut value = child.eval_field_key(&name, span)?;
                if let Some(constraint) = generated {
                    value = unify_values(&value, &constraint, span)?;
                }
                fields.insert(display, value);
            }
        }
        let mut patterns = Vec::new();
        for (pattern, expr) in child.patterns.borrow().iter() {
            patterns.push((pattern.clone(), eval_expr(expr, &child)?));
        }
        apply_patterns(&patterns, &mut fields, span)?;
        apply_optional_patterns(&patterns, &mut optional, span)?;
        let open = items
            .iter()
            .any(|item| matches!(item, StructItem::Ellipsis(_)));
        if closed_by_default {
            for value in fields.values_mut() {
                *value = close_nested(value);
            }
            for value in optional.values_mut() {
                *value = close_nested(value);
            }
        }
        let admissions = if closed_by_default && !open {
            let mut names: BTreeSet<String> = fields.keys().cloned().collect();
            names.extend(optional.keys().cloned());
            vec![Admission {
                names,
                patterns: patterns
                    .iter()
                    .map(|(pattern, _)| pattern.clone())
                    .collect(),
            }]
        } else {
            Vec::new()
        };
        if closed_by_default
            || open
            || !patterns.is_empty()
            || !optional.is_empty()
            || !definitions.is_empty()
        {
            Ok(Value::Definition {
                fields,
                optional,
                definitions,
                patterns,
                closed: closed_by_default && !open,
                open,
                admissions,
            })
        } else {
            Ok(Value::Struct(fields))
        }
    }

    fn eval_definition_part(&self, expr: &Expr) -> Result<Value, LangError> {
        match expr {
            Expr::Paren(inner, _) => self.eval_definition_part(inner),
            Expr::Struct(items, struct_span) => self.eval_struct_items(items, *struct_span, false),
            _ => eval_expr(expr, self),
        }
    }

    fn eval_definition_expr(&self, expr: &Expr, span: Span) -> Result<Value, LangError> {
        if let Expr::Paren(inner, _) = expr {
            return self.eval_definition_expr(inner, span);
        }
        if let Expr::Struct(items, struct_span) = expr {
            return self.eval_definition_items(items, *struct_span);
        }
        let value = eval_expr(expr, self)?;
        let _ = span;
        Ok(close_nested(&value))
    }

    fn params_items(&self, exprs: &[Expr]) -> Option<Vec<StructItem>> {
        let mut items = Vec::new();
        let mut seen = HashSet::new();
        for expr in exprs {
            self.collect_struct_items(expr, &mut items, &mut seen, 0)?;
        }
        Some(items)
    }

    #[allow(clippy::question_mark)]
    fn collect_struct_items(
        &self,
        expr: &Expr,
        out: &mut Vec<StructItem>,
        seen: &mut HashSet<String>,
        depth: usize,
    ) -> Option<()> {
        if depth > 32 {
            return None;
        }
        match expr {
            Expr::Struct(items, _) => out.extend(items.iter().cloned()),
            Expr::Default(inner, _) | Expr::Paren(inner, _) => {
                self.collect_struct_items(inner, out, seen, depth + 1)?;
            }
            Expr::Unify(left, right, _) => {
                self.collect_struct_items(left, out, seen, depth + 1)?;
                self.collect_struct_items(right, out, seen, depth + 1)?;
            }
            Expr::Ident(name, _) => {
                if !seen.insert(name.clone()) {
                    return None;
                }
                let exprs = if let Some(definition) = self.fields.borrow().get(name) {
                    let optional = definition.optional;
                    let exprs = definition.exprs.clone();
                    if optional && !self.optional_is_materialized(name) {
                        return None;
                    }
                    exprs
                } else if let Some(value) = self.lets.borrow().get(name).cloned() {
                    vec![value]
                } else if let Some(target) = self.aliases.borrow().get(name).cloned() {
                    let key = if target.hidden {
                        let hidden = hidden_field(&target.name);
                        self.fields.borrow().contains_key(&hidden).then_some(hidden)
                    } else if target.definition {
                        self.stored_field_key(&target.name)
                    } else {
                        self.field_storage_key(&target.name)
                    }?;
                    self.fields
                        .borrow()
                        .get(&key)
                        .map(|definition| definition.exprs.clone())?
                } else {
                    return None;
                };
                for expr in &exprs {
                    self.collect_struct_items(expr, out, seen, depth + 1)?;
                }
                seen.remove(name);
            }
            _ => return None,
        }
        Some(())
    }

    fn eval_params_struct(
        &self,
        items: &[StructItem],
        exprs: &[Expr],
        span: Span,
    ) -> Result<Value, LangError> {
        let context = Rc::new(ParamsContext {
            items: items.to_vec(),
            overrides: self.overrides.borrow().clone(),
            values: RefCell::new(BTreeMap::new()),
            evaluating: RefCell::new(HashSet::new()),
            local: RefCell::new(HashSet::new()),
        });
        *self.params_context.borrow_mut() = Some(context);
        let child = Frame::from_static(items, Some(self), HashMap::new())?;
        child.params_scope.set(true);
        let overrides = self.overrides.borrow().clone();
        for (key, value) in &overrides {
            child.values.borrow_mut().insert(key.clone(), value.clone());
        }
        child.expand(items)?;
        let mut materialized_values = BTreeMap::new();
        for name in child.materialized.borrow().iter() {
            if let Some(key) = child.display_storage_key(name)
                && let Some(value) = child.values.borrow().get(&key)
            {
                materialized_values.insert(display_field_name(name).to_owned(), value.clone());
            }
        }
        for key in overrides.keys() {
            let known = child.fields.borrow().contains_key(key)
                || child.materialized.borrow().contains(key)
                || child.generated_optionals.borrow().contains_key(key);
            if !known {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("unknown parameter `{key}`"),
                ));
            }
        }
        for (key, value) in &overrides {
            child.values.borrow_mut().remove(key);
            let declared = {
                let fields = child.fields.borrow();
                fields.get(key).map(|definition| definition.exprs.clone())
            };
            let resolved = if let Some(exprs) = declared {
                let mut result: Option<Value> = None;
                for expr in &exprs {
                    let candidate = eval_with_override(expr, &child, value, span)?;
                    result = Some(match result {
                        None => candidate,
                        Some(current) => unify_values(&current, &candidate, span)?,
                    });
                }
                result.ok_or_else(|| {
                    LangError::eval(span.line, span.col, format!("empty parameter `{key}`"))
                })?
            } else {
                value.clone()
            };
            let resolved = match materialized_values.get(key) {
                Some(prior) => unify_values(prior, &resolved, span)?,
                None => resolved,
            };
            child.values.borrow_mut().insert(key.clone(), resolved);
        }
        let mut result = child.to_struct_value()?;
        let patterns: Vec<(Pattern, Value)> = child
            .patterns
            .borrow()
            .iter()
            .map(|(pattern, expr)| Ok((pattern.clone(), eval_expr(expr, &child)?)))
            .collect::<Result<_, LangError>>()?;
        if let Value::Struct(map) = &mut result {
            apply_patterns(&patterns, map, span)?;
        }
        for expr in exprs {
            self.apply_closed_params(expr, &child, &mut result, span)?;
        }
        Ok(result)
    }

    fn apply_closed_params(
        &self,
        expr: &Expr,
        params_frame: &Frame<'_>,
        result: &mut Value,
        span: Span,
    ) -> Result<(), LangError> {
        match expr {
            Expr::Paren(inner, _) | Expr::Default(inner, _) => {
                self.apply_closed_params(inner, params_frame, result, span)?;
            }
            Expr::Unify(left, right, _) => {
                self.apply_closed_params(left, params_frame, result, span)?;
                self.apply_closed_params(right, params_frame, result, span)?;
            }
            Expr::Ident(name, _) if name != "params" => {
                let injections = concrete_fields(result).unwrap_or_default();
                let value = if self.overrides.borrow().is_empty() {
                    if !injections.is_empty()
                        && let Some(narrowed) =
                            eval_definition_with_injections(self, name, injections.clone(), span)?
                    {
                        narrowed
                    } else {
                        self.lookup(name, span)?
                    }
                } else if let Some(exprs) = self
                    .fields
                    .borrow()
                    .get(name)
                    .map(|definition| definition.exprs.clone())
                {
                    if let Some(items) = self.params_items(&exprs) {
                        let def = Frame::from_static(&items, Some(params_frame), HashMap::new())?;
                        def.closed.set(true);
                        for key in self.overrides.borrow().keys() {
                            if let Some(value) = params_frame.values.borrow().get(key) {
                                def.values.borrow_mut().insert(key.clone(), value.clone());
                            }
                        }
                        def.expand(&items)?;
                        def.reconcile_generated_required_fields()?;
                        def.to_struct_value_mode(true)?
                    } else {
                        self.lookup(name, span)?
                    }
                } else {
                    let injections = concrete_fields(result).unwrap_or_default();
                    lookup_with_injections(self, name, injections, span)?
                };
                if matches!(value, Value::Definition { closed: true, .. }) {
                    *result = unify_values(result, &value, span)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn eval_let_with_injections(
        &self,
        name: &str,
        injections: &Value,
        span: Span,
    ) -> Result<Value, LangError> {
        if self.evaluating.borrow().contains(name) {
            return Err(LangError::eval(
                span.line,
                span.col,
                format!("cyclic let binding while evaluating `{name}`"),
            ));
        }
        let expr = self.lets.borrow().get(name).cloned();
        let Some(expr) = expr else {
            return Err(LangError::eval(
                span.line,
                span.col,
                format!("undefined let binding `{name}`"),
            ));
        };
        self.evaluating.borrow_mut().insert(name.to_owned());
        let previous = self.self_rhs.replace(Some(name.to_owned()));
        let previous_cache = self.skip_cache.replace(true);
        let effective = self.merge_let_injections(&expr, injections, span);
        let result = match effective {
            Ok(effective) => eval_declaration_part(self, &expr, &effective, span),
            Err(err) => Err(err),
        };
        self.skip_cache.set(previous_cache);
        *self.self_rhs.borrow_mut() = previous;
        self.evaluating.borrow_mut().remove(name);
        result
    }

    fn merge_let_injections(
        &self,
        expr: &Expr,
        injections: &Value,
        span: Span,
    ) -> Result<Value, LangError> {
        let Some(mut fields) = concrete_fields(injections) else {
            return Ok(injections.clone());
        };
        let expr = strip_expr_parens(expr);
        if !matches!(expr, Expr::Unify(_, _, _)) {
            return Ok(injections.clone());
        }
        let mut operands = Vec::new();
        collect_unify_operands(expr, &mut operands);
        let Some(base_index) = split_base_index(&operands) else {
            return Ok(injections.clone());
        };
        for (index, operand) in operands.iter().enumerate() {
            if index == base_index || !expr_is_struct_literal(operand) {
                continue;
            }
            let value = eval_expr(operand, self)?;
            if let Some(extra) = concrete_fields(&value) {
                merge_injection_fields(&mut fields, extra, span)?;
            }
        }
        Ok(Value::Struct(fields))
    }

    fn eval_let(&self, name: &str, span: Span) -> Result<Value, LangError> {
        if !self.skip_cache.get()
            && let Some(value) = self.vars.borrow().get(name)
        {
            return Ok(value.clone());
        }
        if self.evaluating.borrow().contains(name) {
            return Err(LangError::eval(
                span.line,
                span.col,
                format!("cyclic let binding while evaluating `{name}`"),
            ));
        }
        let expr = self.lets.borrow().get(name).cloned();
        let Some(expr) = expr else {
            return Err(LangError::eval(
                span.line,
                span.col,
                format!("undefined let binding `{name}`"),
            ));
        };
        self.evaluating.borrow_mut().insert(name.to_owned());
        let previous = self.self_rhs.replace(Some(name.to_owned()));
        let result = eval_expr(&expr, self);
        *self.self_rhs.borrow_mut() = previous;
        self.evaluating.borrow_mut().remove(name);
        let value = result?;
        if !self.skip_cache.get() {
            self.vars
                .borrow_mut()
                .insert(name.to_owned(), value.clone());
        }
        Ok(value)
    }

    fn ensure_params_context(&self) -> Option<Rc<ParamsContext>> {
        if let Some(context) = self.params_context.borrow().clone() {
            return Some(context);
        }
        let exprs = self.fields.borrow().get("params")?.exprs.clone();
        let items = self.params_items(&exprs)?;
        let context = Rc::new(ParamsContext {
            items,
            overrides: self.overrides.borrow().clone(),
            values: RefCell::new(BTreeMap::new()),
            evaluating: RefCell::new(HashSet::new()),
            local: RefCell::new(HashSet::new()),
        });
        *self.params_context.borrow_mut() = Some(context.clone());
        Some(context)
    }

    fn to_struct_value(&self) -> Result<Value, LangError> {
        self.to_struct_value_mode(false)
    }

    fn to_struct_value_mode(&self, include_definitions: bool) -> Result<Value, LangError> {
        self.reconcile_generated_required_fields()?;
        let generated = self.generated_optionals.borrow().clone();
        let names: Vec<String> = {
            let fields = self.fields.borrow();
            fields
                .iter()
                .filter(|(name, definition)| {
                    (include_definitions || !definition.definition)
                        && (definition.definition
                            || (!generated.contains_key(*name)
                                && (name.starts_with([
                                    HIDDEN_PREFIX,
                                    ESCAPED_PREFIX,
                                    NON_DEFINITION_PREFIX,
                                ]) || !generated.contains_key(display_field_name(name)))))
                })
                .map(|(name, _)| name.clone())
                .collect()
        };
        let mut names = names;
        if include_definitions {
            names.extend(self.generated_definitions.borrow().keys().cloned());
        }
        names.sort();
        names.dedup();
        let mut fields_out = BTreeMap::new();
        let mut definitions_out = BTreeMap::new();
        for name in names {
            let optional = self
                .fields
                .borrow()
                .get(&name)
                .is_some_and(|def| def.optional);
            if optional {
                // Match `cue export`: optional fields are not part of the
                // concrete export, even when their value is known.
                continue;
            }
            let is_definition = self
                .fields
                .borrow()
                .get(&name)
                .is_some_and(|def| def.definition);
            let value = match self.eval_field_key(&name, Span::default()) {
                Ok(value) => value,
                Err(err)
                    if (is_hidden_key(&name) || (include_definitions && is_definition))
                        && is_deferrable_incomplete_error(&err) =>
                {
                    // Re-evaluate with soft incompleteness deferred; only skip
                    // if no hard error lies underneath the first incomplete
                    // error. Definitions are never emitted by CUE, and hidden
                    // fields may stay incomplete.
                    self.incomplete_field_validates(&name, Span::default())?;
                    continue;
                }
                Err(err) => return Err(err),
            };
            let display = output_field_name(&name);
            if is_definition {
                definitions_out.insert(display, value);
            } else {
                fields_out.insert(display, value);
            }
        }
        if include_definitions {
            for (name, value) in self.generated_definitions.borrow().iter() {
                definitions_out.insert(output_field_name(name), value.clone());
            }
        }
        for (name, value) in self.values.borrow().iter() {
            let definition = self
                .fields
                .borrow()
                .get(name)
                .is_some_and(|definition| definition.definition);
            if (!include_definitions && definition)
                || self.generated_optionals.borrow().contains_key(name)
                || (!name.starts_with([HIDDEN_PREFIX, ESCAPED_PREFIX, NON_DEFINITION_PREFIX])
                    && self
                        .generated_optionals
                        .borrow()
                        .contains_key(display_field_name(name)))
            {
                continue;
            }
            let display = output_field_name(name);
            if definition {
                definitions_out.insert(display, value.clone());
            } else {
                fields_out.insert(display, value.clone());
            }
        }
        if include_definitions {
            return Ok(Value::Definition {
                fields: fields_out,
                optional: BTreeMap::new(),
                definitions: definitions_out,
                patterns: Vec::new(),
                closed: false,
                open: true,
                admissions: Vec::new(),
            });
        }
        Ok(Value::Struct(fields_out))
    }
}

fn quoted_lookup<'m, V>(map: &'m BTreeMap<String, V>, field: &str) -> Option<&'m V> {
    if field.starts_with([HIDDEN_PREFIX, ESCAPED_PREFIX, NON_DEFINITION_PREFIX]) {
        map.get(&escaped_field(field))
    } else {
        map.get(field).or_else(|| map.get(&escaped_field(field)))
    }
}

fn self_definition_select(
    frame: &Frame<'_>,
    base: &Expr,
    field: &str,
    quoted: bool,
    span: Span,
) -> Result<Option<Value>, LangError> {
    let mut base = base;
    while let Expr::Paren(inner, _) | Expr::Default(inner, _) = base {
        base = inner;
    }
    let Expr::Ident(name, _) = base else {
        return Ok(None);
    };
    let mut candidate = Some(frame);
    while let Some(current) = candidate {
        if current.has_local_name(name) {
            // A closer lexical binding shadows the definition; let the normal
            // identifier/selector path resolve it.
            return Ok(None);
        }
        if current.definition_context.borrow().as_deref() == Some(name.as_str()) {
            if let Some(key) = current.segment_field_key(field, quoted) {
                let optional = current
                    .fields
                    .borrow()
                    .get(&key)
                    .is_some_and(|definition| definition.optional);
                let injected = current.is_injected_key(&key);
                if optional && (injected || !current.optional_is_materialized(&key)) {
                    if current.defer_incomplete.get() {
                        return Ok(Some(Value::Bottom));
                    }
                    return Err(LangError::incomplete_optional(
                        span.line,
                        span.col,
                        format!("cannot reference optional field `{field}`"),
                    ));
                }
                if injected {
                    return current
                        .eval_field_key_without_injection(&key, span)
                        .map(Some);
                }
                return current.eval_field_key(&key, span).map(Some);
            }
            if let Some(key) = current.generated_optional_key(field, quoted)
                && (current.is_injected_key(&key) || !current.optional_is_materialized(&key))
            {
                if current.defer_incomplete.get() {
                    return Ok(Some(Value::Bottom));
                }
                return Err(LangError::incomplete_optional(
                    span.line,
                    span.col,
                    format!("cannot reference optional field `{field}`"),
                ));
            }
            if let Some(key) = current.generated_value_key(field, quoted)
                && let Some(value) = current.values.borrow().get(&key).cloned()
            {
                return Ok(Some(value));
            }
        }
        candidate = current.parent;
    }
    Ok(None)
}

fn strip_expr_parens(expr: &Expr) -> &Expr {
    let mut expr = expr;
    while let Expr::Paren(inner, _) | Expr::Default(inner, _) = expr {
        expr = inner;
    }
    expr
}

fn concrete_fields(value: &Value) -> Option<BTreeMap<String, Value>> {
    match value.concretize() {
        Value::Struct(fields) => Some(fields.clone()),
        Value::Definition { fields, .. } => Some(fields.clone()),
        _ => None,
    }
}

fn collect_unify_operands<'e>(expr: &'e Expr, out: &mut Vec<&'e Expr>) {
    let expr = strip_expr_parens(expr);
    if let Expr::Unify(left, right, _) = expr {
        collect_unify_operands(left, out);
        collect_unify_operands(right, out);
    } else {
        out.push(expr);
    }
}

fn expr_is_struct_literal(expr: &Expr) -> bool {
    matches!(strip_expr_parens(expr), Expr::Struct(_, _))
}

fn split_base_index(operands: &[&Expr]) -> Option<usize> {
    operands
        .iter()
        .position(|expr| matches!(strip_expr_parens(expr), Expr::Ident(_, _)))
        .or_else(|| {
            operands
                .iter()
                .position(|expr| expr_is_struct_literal(expr))
        })
}

fn merge_injection_fields(
    target: &mut BTreeMap<String, Value>,
    fields: BTreeMap<String, Value>,
    span: Span,
) -> Result<(), LangError> {
    for (key, value) in fields {
        if let Some(existing) = target.remove(&key) {
            target.insert(key, unify_values(&existing, &value, span)?);
        } else {
            target.insert(key, value);
        }
    }
    Ok(())
}

fn value_is_plain_concrete(value: &Value) -> bool {
    match value {
        Value::Null
        | Value::Bool(_)
        | Value::Int(_)
        | Value::BigInt(_)
        | Value::Float(_)
        | Value::String(_)
        | Value::Bytes(_) => true,
        Value::List(values) | Value::OpenListPrefix(values, _) => {
            values.iter().all(value_is_plain_concrete)
        }
        Value::Struct(fields) | Value::Definition { fields, .. } => {
            fields.values().all(value_is_plain_concrete)
        }
        Value::OpenList(_)
        | Value::Disjunction(_, _)
        | Value::Bounds(_)
        | Value::Type(_)
        | Value::Builtin(_)
        | Value::Bottom
        | Value::Conflict(_)
        | Value::Recursive(_)
        | Value::Deferred(_, _) => false,
    }
}

fn is_optional_incomplete_error(err: &LangError) -> bool {
    err.is_incomplete_optional() || err.to_string().contains("cannot reference optional field")
}

fn is_soft_alternative_error(err: &LangError) -> bool {
    is_deferrable_incomplete_error(err)
}

fn hard_error_beneath_soft(expr: &Expr, frame: &Frame<'_>) -> Result<(), LangError> {
    let previous_defer = frame.defer_incomplete.replace(true);
    let previous_cache = frame.skip_cache.replace(true);
    let result = eval_expr(expr, frame);
    frame.skip_cache.set(previous_cache);
    frame.defer_incomplete.set(previous_defer);
    match result {
        Ok(_) => Ok(()),
        Err(err) if err.is_conflict() || is_deferrable_incomplete_error(&err) => Ok(()),
        Err(err) => Err(err),
    }
}

fn is_deferrable_incomplete_error(err: &LangError) -> bool {
    is_optional_incomplete_error(err)
        || err.is_incomplete_value()
        || err.to_string().contains("non-concrete value")
}

fn lookup_with_injections(
    frame: &Frame<'_>,
    name: &str,
    injections: BTreeMap<String, Value>,
    span: Span,
) -> Result<Value, LangError> {
    match frame.lookup(name, span) {
        Ok(value) => Ok(value),
        Err(err) if is_deferrable_incomplete_error(&err) => {
            match eval_definition_with_injections(frame, name, injections, span)? {
                Some(value) => Ok(value),
                None => Err(err),
            }
        }
        Err(err) => Err(err),
    }
}

fn eval_definition_named(
    frame: &Frame<'_>,
    name: &str,
    span: Span,
) -> Result<Option<Value>, LangError> {
    let mut cursor = Some(frame);
    while let Some(current) = cursor {
        if current.vars.borrow().contains_key(name)
            || current.lets.borrow().contains_key(name)
            || current.aliases.borrow().contains_key(name)
        {
            return Ok(None);
        }
        if let Some(key) = current.any_field_key(name) {
            let optional = current
                .fields
                .borrow()
                .get(&key)
                .is_some_and(|definition| definition.optional);
            if optional && !current.optional_is_materialized(&key) {
                return Ok(None);
            }
            return current.eval_field_key(&key, span).map(Some);
        }
        cursor = current.parent;
    }
    Ok(None)
}

fn eval_definition_with_injections(
    frame: &Frame<'_>,
    name: &str,
    injections: BTreeMap<String, Value>,
    span: Span,
) -> Result<Option<Value>, LangError> {
    let mut cursor = Some(frame);
    while let Some(current) = cursor {
        if current.lets.borrow().contains_key(name) {
            let value = Value::Struct(injections);
            return current
                .eval_let_with_injections(name, &value, span)
                .map(Some);
        }
        if current.vars.borrow().contains_key(name) || current.aliases.borrow().contains_key(name) {
            return Ok(None);
        }
        if let Some(key) = current.any_field_key(name) {
            let exprs = current
                .fields
                .borrow()
                .get(&key)
                .map(|definition| definition.exprs.clone())
                .unwrap_or_default();
            let optional = current
                .fields
                .borrow()
                .get(&key)
                .is_some_and(|definition| definition.optional);
            if optional && !current.optional_is_materialized(&key) {
                return Ok(None);
            }
            if exprs.len() == 1
                && let Expr::Ident(target, _) = strip_expr_parens(&exprs[0])
                && target != name
            {
                return eval_definition_with_injections(current, target, injections, span);
            }
            current.values.borrow_mut().remove(&key);
            let previous = current.next_injections.replace(Some(injections));
            let previous_cache = current.skip_cache.replace(true);
            let result = current.eval_field_key(&key, span);
            current.skip_cache.set(previous_cache);
            current.next_injections.replace(previous);
            return result.map(Some);
        }
        cursor = current.parent;
    }
    Ok(None)
}

fn struct_narrows_key(
    frame: &Frame<'_>,
    items: &[StructItem],
    injections: &BTreeMap<String, Value>,
) -> bool {
    items.iter().any(|item| match item {
        StructItem::Field(field) => {
            let crate::ast::Label::Name(name, _) = &field.label else {
                return false;
            };
            injections.contains_key(name) && field_value_needs_injection(frame, &field.value)
        }
        StructItem::If { body, .. } | StructItem::For { body, .. } => {
            struct_narrows_key(frame, body, injections)
        }
        _ => false,
    })
}

fn expr_narrows_key(frame: &Frame<'_>, expr: &Expr, injections: &BTreeMap<String, Value>) -> bool {
    match expr {
        Expr::Struct(items, _) => struct_narrows_key(frame, items, injections),
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            expr_narrows_key(frame, left, injections) || expr_narrows_key(frame, right, injections)
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) => {
            expr_narrows_key(frame, inner, injections)
        }
        _ => false,
    }
}

fn field_value_needs_injection(frame: &Frame<'_>, expr: &Expr) -> bool {
    if contains_default(expr) {
        return true;
    }
    let mut identifiers = Vec::new();
    collect_identifiers(expr, &mut identifiers);
    if identifiers
        .iter()
        .any(|name| !name.starts_with('#') && !is_type_keyword(name))
    {
        return true;
    }
    identifiers
        .iter()
        .filter(|name| name.starts_with('#'))
        .any(|name| {
            definition_exprs(frame, name).iter().any(|expr| {
                contains_default(expr) || {
                    let mut identifiers = Vec::new();
                    collect_identifiers(expr, &mut identifiers);
                    identifiers
                        .iter()
                        .any(|name| !name.starts_with('#') && !is_type_keyword(name))
                }
            })
        })
}

fn is_type_keyword(name: &str) -> bool {
    matches!(
        name,
        "int" | "string" | "number" | "float" | "bytes" | "bool"
    )
}

fn definition_needs_narrowing(
    frame: &Frame<'_>,
    name: &str,
    injections: &BTreeMap<String, Value>,
) -> bool {
    let mut visited = BTreeSet::new();
    definition_needs_narrowing_inner(frame, name, injections, &mut visited)
}

fn definition_needs_narrowing_inner(
    frame: &Frame<'_>,
    name: &str,
    injections: &BTreeMap<String, Value>,
    visited: &mut BTreeSet<String>,
) -> bool {
    if !visited.insert(name.to_owned()) {
        return false;
    }
    let mut cursor = Some(frame);
    while let Some(current) = cursor {
        if let Some(key) = current.any_field_key(name) {
            let exprs = current
                .fields
                .borrow()
                .get(&key)
                .map(|definition| definition.exprs.clone())
                .unwrap_or_default();
            let self_keys: BTreeSet<String> = std::iter::once(name.to_owned()).collect();
            if exprs
                .iter()
                .any(|expr| expr_references_keys(expr, &self_keys))
            {
                return false;
            }
            if exprs
                .iter()
                .any(|expr| expr_narrows_key(current, expr, injections))
            {
                return true;
            }
            for expr in &exprs {
                if let Expr::Ident(target, _) = strip_expr_parens(expr)
                    && target != name
                    && definition_needs_narrowing_inner(current, target, injections, visited)
                {
                    return true;
                }
            }
            return false;
        }
        cursor = current.parent;
    }
    false
}

fn try_definition_unify(
    definition: &Expr,
    other: &Expr,
    frame: &Frame<'_>,
    span: Span,
) -> Result<Option<Value>, LangError> {
    let definition = strip_expr_parens(definition);
    let Expr::Ident(name, _) = definition else {
        let Expr::Struct(items, _) = definition else {
            return Ok(None);
        };
        let value = eval_expr(other, frame)?;
        let Some(injections) = concrete_fields(&value) else {
            return Ok(None);
        };
        if !struct_narrows_key(frame, items, &injections) {
            return Ok(None);
        }
        let previous = frame.next_injections.replace(Some(injections));
        let result = eval_expr(definition, frame);
        frame.next_injections.replace(previous);
        return unify_values(&result?, &value, span).map(Some);
    };
    let value = eval_expr(other, frame)?;
    let Some(injections) = concrete_fields(&value) else {
        return Ok(None);
    };
    if definition_needs_narrowing(frame, name, &injections) {
        let Some(defined) = eval_definition_with_injections(frame, name, injections, span)? else {
            return Ok(None);
        };
        return unify_values(&defined, &value, span).map(Some);
    }
    let may_help = |err: &LangError| injection_may_help(frame, name, &injections, err);
    match eval_definition_named(frame, name, span) {
        Ok(Some(defined)) => match unify_values(&defined, &value, span) {
            Ok(value) => return Ok(Some(value)),
            Err(err) if may_help(&err) => {}
            Err(err) => return Err(err),
        },
        Ok(None) => {
            if let Some(defined) = eval_definition_with_injections(frame, name, injections, span)? {
                return unify_values(&defined, &value, span).map(Some);
            }
            return Ok(None);
        }
        Err(err) if may_help(&err) => {}
        Err(err) => return Err(err),
    }
    let Some(defined) = eval_definition_with_injections(frame, name, injections, span)? else {
        return Ok(None);
    };
    unify_values(&defined, &value, span).map(Some)
}

fn injection_may_help(
    frame: &Frame<'_>,
    name: &str,
    injections: &BTreeMap<String, Value>,
    err: &LangError,
) -> bool {
    if is_optional_incomplete_error(err) {
        return true;
    }
    if !err.to_string().contains("non-concrete value") {
        return false;
    }
    let keys: BTreeSet<String> = injections.keys().cloned().collect();
    if definition_has_optional_injected_field(frame, name, &keys) {
        return true;
    }
    definition_exprs(frame, name)
        .iter()
        .any(|expr| expr_references_keys(expr, &keys))
}

fn definition_has_optional_injected_field(
    frame: &Frame<'_>,
    name: &str,
    keys: &BTreeSet<String>,
) -> bool {
    definition_exprs(frame, name)
        .iter()
        .any(|expr| expr_has_optional_injected_field(expr, keys))
}

fn expr_has_optional_injected_field(expr: &Expr, keys: &BTreeSet<String>) -> bool {
    match expr {
        Expr::Struct(items, _) => struct_items_have_optional_injected_field(items, keys),
        Expr::Paren(inner, _) | Expr::Default(inner, _) => {
            expr_has_optional_injected_field(inner, keys)
        }
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            expr_has_optional_injected_field(left, keys)
                || expr_has_optional_injected_field(right, keys)
        }
        _ => false,
    }
}

fn struct_items_have_optional_injected_field(
    items: &[StructItem],
    keys: &BTreeSet<String>,
) -> bool {
    items.iter().any(|item| match item {
        StructItem::Field(field) => match &field.label {
            crate::ast::Label::Name(name, _) if field.optional => keys.contains(name),
            _ => false,
        },
        StructItem::If { body, .. } | StructItem::For { body, .. } => {
            struct_items_have_optional_injected_field(body, keys)
        }
        StructItem::Pattern { .. } | StructItem::Let { .. } | StructItem::Ellipsis(_) => false,
    })
}

fn definition_exprs(frame: &Frame<'_>, name: &str) -> Vec<Expr> {
    let mut cursor = Some(frame);
    while let Some(current) = cursor {
        if let Some(key) = current.any_field_key(name) {
            return current
                .fields
                .borrow()
                .get(&key)
                .map(|definition| definition.exprs.clone())
                .unwrap_or_default();
        }
        cursor = current.parent;
    }
    Vec::new()
}

fn expr_references_keys(expr: &Expr, keys: &BTreeSet<String>) -> bool {
    match expr {
        Expr::Ident(name, _) => keys.contains(name),
        Expr::Select(base, _, _, _) => expr_references_keys(base, keys),
        Expr::Index(base, index, _) => {
            expr_references_keys(base, keys) || expr_references_keys(index, keys)
        }
        Expr::Call(callee, arguments, _) => {
            expr_references_keys(callee, keys)
                || arguments.iter().any(|arg| expr_references_keys(arg, keys))
        }
        Expr::Bound(_, operand, _)
        | Expr::Paren(operand, _)
        | Expr::Default(operand, _)
        | Expr::Unary(_, operand, _) => expr_references_keys(operand, keys),
        Expr::Binary(_, left, right, _)
        | Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _) => {
            expr_references_keys(left, keys) || expr_references_keys(right, keys)
        }
        Expr::Struct(items, _) => struct_items_reference_keys(items, keys),
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            items.iter().any(|item| expr_references_keys(item, keys))
        }
        Expr::OpenList(Some(item), _) => expr_references_keys(item, keys),
        Expr::OpenList(None, _) => false,
        Expr::Str(parts, _) => parts.iter().any(|part| match part {
            crate::ast::StrPart::Text(_) => false,
            crate::ast::StrPart::Interp(expr) => expr_references_keys(expr, keys),
        }),
        Expr::BytesInterp(parts, _) => parts.iter().any(|part| match part {
            crate::ast::BytePart::Text(_) => false,
            crate::ast::BytePart::Interp(expr) => expr_references_keys(expr, keys),
        }),
        Expr::ListComp { clauses, body, .. } => {
            expr_references_keys(body, keys)
                || clauses.iter().any(|clause| match clause {
                    crate::ast::CompClause::For { source, .. } => {
                        expr_references_keys(source, keys)
                    }
                    crate::ast::CompClause::If { cond, .. } => expr_references_keys(cond, keys),
                })
        }
        Expr::Bytes(_, _)
        | Expr::Null(_)
        | Expr::Bool(_, _)
        | Expr::Int(_, _)
        | Expr::BigInt(_, _)
        | Expr::Float(_, _) => false,
    }
}

fn struct_items_reference_keys(items: &[StructItem], keys: &BTreeSet<String>) -> bool {
    items.iter().any(|item| match item {
        StructItem::Field(field) => expr_references_keys(&field.value, keys),
        StructItem::Pattern { value, .. } => expr_references_keys(value, keys),
        StructItem::Let { value, .. } => expr_references_keys(value, keys),
        StructItem::For { source, body, .. } => {
            expr_references_keys(source, keys) || struct_items_reference_keys(body, keys)
        }
        StructItem::If { cond, body, .. } => {
            expr_references_keys(cond, keys) || struct_items_reference_keys(body, keys)
        }
        StructItem::Ellipsis(_) => false,
    })
}

fn eval_declaration_part(
    frame: &Frame<'_>,
    expr: &Expr,
    value: &Value,
    span: Span,
) -> Result<Value, LangError> {
    let expr = strip_expr_parens(expr);
    match (expr, value) {
        (Expr::Struct(_, _), value) => {
            if let Some(nested) = concrete_fields(value) {
                *frame.next_injections.borrow_mut() = Some(nested);
            }
            let declared = eval_expr(expr, frame)?;
            frame.next_injections.borrow_mut().take();
            Ok(declared)
        }
        (Expr::Unify(left, right, _), value) => {
            let left = eval_declaration_part(frame, left, value, span)?;
            let right = eval_declaration_part(frame, right, value, span)?;
            unify_values(&left, &right, span)
        }
        (Expr::Ident(name, _), value) => {
            if frame.injected_field_reference(name) {
                return eval_expr(expr, frame);
            }
            if let Some(injections) = concrete_fields(value)
                && let Some(narrowed) =
                    eval_definition_with_injections(frame, name, injections, span)?
            {
                return Ok(narrowed);
            }
            eval_expr(expr, frame)
        }
        (Expr::List(items, _), Value::List(values)) => {
            if items.len() != values.len() {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "cannot unify lists of different lengths",
                ));
            }
            let mut out = Vec::with_capacity(values.len());
            for (item, value) in items.iter().zip(values) {
                out.push(eval_declaration_part(frame, item, value, span)?);
            }
            Ok(Value::List(out))
        }
        (Expr::OpenList(element, _), Value::List(values)) => {
            let mut out = Vec::with_capacity(values.len());
            for value in values {
                out.push(match element {
                    Some(element) => eval_declaration_part(frame, element, value, span)?,
                    None => value.clone(),
                });
            }
            Ok(Value::List(out))
        }
        (Expr::OpenListPrefix(prefix, tail, _), Value::List(values)) => {
            if prefix.len() > values.len() {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "list prefix is longer than the concrete list",
                ));
            }
            let mut out = Vec::with_capacity(values.len());
            for (index, value) in values.iter().enumerate() {
                let expected = prefix.get(index).or(tail.as_deref());
                out.push(match expected {
                    Some(expected) => eval_declaration_part(frame, expected, value, span)?,
                    None => value.clone(),
                });
            }
            Ok(Value::List(out))
        }
        _ => eval_expr(expr, frame),
    }
}

fn expr_is_narrowing_source(expr: &Expr) -> bool {
    !contains_default(expr)
        && !expr_contains_ident(expr)
        && matches!(
            strip_expr_parens(expr),
            Expr::Struct(_, _) | Expr::List(_, _) | Expr::OpenListPrefix(_, _, _)
        )
}

fn expr_contains_ident(expr: &Expr) -> bool {
    let mut identifiers = Vec::new();
    collect_identifiers(expr, &mut identifiers);
    !identifiers.is_empty()
}

fn collect_identifiers(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Ident(name, _) => out.push(name.clone()),
        Expr::Select(base, _, _, _) => collect_identifiers(base, out),
        Expr::Index(base, index, _) => {
            collect_identifiers(base, out);
            collect_identifiers(index, out);
        }
        Expr::Call(callee, arguments, _) => {
            collect_identifiers(callee, out);
            for argument in arguments {
                collect_identifiers(argument, out);
            }
        }
        Expr::Bound(_, operand, _)
        | Expr::Paren(operand, _)
        | Expr::Default(operand, _)
        | Expr::Unary(_, operand, _) => collect_identifiers(operand, out),
        Expr::Binary(_, left, right, _)
        | Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _) => {
            collect_identifiers(left, out);
            collect_identifiers(right, out);
        }
        Expr::Struct(items, _) => collect_struct_identifiers(items, out),
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                collect_identifiers(item, out);
            }
        }
        Expr::OpenList(Some(item), _) => collect_identifiers(item, out),
        Expr::OpenList(None, _) => {}
        Expr::Str(parts, _) => {
            for part in parts {
                if let crate::ast::StrPart::Interp(expr) = part {
                    collect_identifiers(expr, out);
                }
            }
        }
        Expr::BytesInterp(parts, _) => {
            for part in parts {
                if let crate::ast::BytePart::Interp(expr) = part {
                    collect_identifiers(expr, out);
                }
            }
        }
        Expr::ListComp { clauses, body, .. } => {
            collect_identifiers(body, out);
            for clause in clauses {
                match clause {
                    crate::ast::CompClause::For { source, .. } => {
                        collect_identifiers(source, out);
                    }
                    crate::ast::CompClause::If { cond, .. } => {
                        collect_identifiers(cond, out);
                    }
                }
            }
        }
        Expr::Bytes(_, _)
        | Expr::Null(_)
        | Expr::Bool(_, _)
        | Expr::Int(_, _)
        | Expr::BigInt(_, _)
        | Expr::Float(_, _) => {}
    }
}

fn collect_struct_identifiers(items: &[StructItem], out: &mut Vec<String>) {
    for item in items {
        match item {
            StructItem::Field(field) => collect_identifiers(&field.value, out),
            StructItem::Pattern { value, .. } => collect_identifiers(value, out),
            StructItem::Let { value, .. } => collect_identifiers(value, out),
            StructItem::For { source, body, .. } => {
                collect_identifiers(source, out);
                collect_struct_identifiers(body, out);
            }
            StructItem::If { cond, body, .. } => {
                collect_identifiers(cond, out);
                collect_struct_identifiers(body, out);
            }
            StructItem::Ellipsis(_) => {}
        }
    }
}

fn contains_default(expr: &Expr) -> bool {
    match expr {
        Expr::Default(_, _) => true,
        Expr::Disjunction(left, right, _) | Expr::Unify(left, right, _) => {
            contains_default(left) || contains_default(right)
        }
        Expr::Paren(inner, _) => contains_default(inner),
        Expr::Struct(items, _) => items.iter().any(|item| match item {
            StructItem::Field(field) => contains_default(&field.value),
            StructItem::If { body, .. } | StructItem::For { body, .. } => {
                body.iter().any(|item| match item {
                    StructItem::Field(field) => contains_default(&field.value),
                    _ => false,
                })
            }
            _ => false,
        }),
        Expr::List(items, _) => items.iter().any(contains_default),
        Expr::OpenList(Some(inner), _) => contains_default(inner),
        Expr::OpenListPrefix(prefix, tail, _) => {
            prefix.iter().any(contains_default) || tail.as_deref().is_some_and(contains_default)
        }
        _ => false,
    }
}
fn merge_declaration(left: &Value, right: &Value, span: Span) -> Result<Value, LangError> {
    fn is_struct_like(value: &Value) -> bool {
        matches!(value, Value::Definition { .. } | Value::Struct(_))
    }
    fn merge_map(
        left: &BTreeMap<String, Value>,
        right: &BTreeMap<String, Value>,
        span: Span,
    ) -> Result<BTreeMap<String, Value>, LangError> {
        let mut out = left.clone();
        for (key, value) in right {
            if let Some(existing) = out.get(key).cloned() {
                out.insert(
                    key.clone(),
                    if is_struct_like(&existing) && is_struct_like(value) {
                        merge_declaration(&existing, value, span)?
                    } else {
                        unify_values(&existing, value, span)?
                    },
                );
            } else {
                out.insert(key.clone(), value.clone());
            }
        }
        Ok(out)
    }

    match (left, right) {
        (
            Value::Definition {
                fields: left_fields,
                optional: left_optional,
                definitions: left_definitions,
                patterns: left_patterns,
                open: left_open,
                ..
            },
            Value::Definition {
                fields: right_fields,
                optional: right_optional,
                definitions: right_definitions,
                patterns: right_patterns,
                open: right_open,
                ..
            },
        ) => {
            let fields = merge_map(left_fields, right_fields, span)?;
            let optional = merge_map(left_optional, right_optional, span)?;
            let definitions = merge_map(left_definitions, right_definitions, span)?;
            let mut patterns = left_patterns.clone();
            for pattern in right_patterns {
                if !patterns.contains(pattern) {
                    patterns.push(pattern.clone());
                }
            }
            let open = *left_open || *right_open;
            let closed = !open;
            let admissions = if closed {
                vec![Admission {
                    names: fields.keys().chain(optional.keys()).cloned().collect(),
                    patterns: patterns
                        .iter()
                        .map(|(pattern, _)| pattern.clone())
                        .collect(),
                }]
            } else {
                Vec::new()
            };
            Ok(Value::Definition {
                fields,
                optional,
                definitions,
                patterns,
                closed,
                open,
                admissions,
            })
        }
        (
            Value::Definition {
                fields: left_fields,
                optional,
                definitions,
                patterns,
                open,
                ..
            },
            Value::Struct(right_fields),
        ) => {
            let fields = merge_map(left_fields, right_fields, span)?;
            let closed = !*open;
            let admissions = if closed {
                vec![Admission {
                    names: fields.keys().chain(optional.keys()).cloned().collect(),
                    patterns: patterns
                        .iter()
                        .map(|(pattern, _)| pattern.clone())
                        .collect(),
                }]
            } else {
                Vec::new()
            };
            Ok(Value::Definition {
                fields,
                optional: optional.clone(),
                definitions: definitions.clone(),
                patterns: patterns.clone(),
                closed,
                open: *open,
                admissions,
            })
        }
        (Value::List(left), Value::List(right)) => {
            if left.len() != right.len() {
                return unify_values(
                    &Value::List(left.clone()),
                    &Value::List(right.clone()),
                    span,
                );
            }
            let mut out = Vec::with_capacity(left.len());
            for (left, right) in left.iter().zip(right) {
                out.push(merge_declaration(left, right, span)?);
            }
            Ok(Value::List(out))
        }
        (Value::OpenList(Some(left)), Value::OpenList(Some(right))) => Ok(Value::OpenList(Some(
            Box::new(merge_declaration(left, right, span)?),
        ))),
        (Value::OpenList(Some(element)), Value::List(values))
        | (Value::List(values), Value::OpenList(Some(element))) => {
            let mut out = Vec::with_capacity(values.len());
            for value in values {
                out.push(merge_declaration(element, value, span)?);
            }
            Ok(Value::List(out))
        }
        (Value::OpenListPrefix(prefix, tail), Value::List(values))
        | (Value::List(values), Value::OpenListPrefix(prefix, tail)) => {
            let mut out = Vec::with_capacity(values.len());
            for (index, value) in values.iter().enumerate() {
                let expected = prefix.get(index).or(tail.as_deref());
                out.push(match expected {
                    Some(expected) => merge_declaration(expected, value, span)?,
                    None => value.clone(),
                });
            }
            Ok(Value::List(out))
        }
        (Value::Struct(left_fields), Value::Struct(right_fields)) => {
            Ok(Value::Struct(merge_map(left_fields, right_fields, span)?))
        }
        _ => unify_values(left, right, span),
    }
}

fn selector_leaf_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Select(_, field, _, _) => Some(field.clone()),
        Expr::Index(_, index, _) => {
            let Expr::Str(parts, _) = &**index else {
                return None;
            };
            let mut text = String::new();
            for part in parts {
                match part {
                    StrPart::Text(value) => text.push_str(value),
                    StrPart::Interp(_) => return None,
                }
            }
            Some(text)
        }
        _ => None,
    }
}

impl Frame<'_> {
    /// Follow identifier chains through lets and fields to a field label.
    fn ident_task_label(&self, expr: &Expr, seen: &mut HashSet<String>) -> Option<String> {
        match expr {
            Expr::Ident(name, _) => {
                if !seen.insert(name.clone()) {
                    return None;
                }
                if let Some(value) = self.lets.borrow().get(name).cloned() {
                    let found = self.ident_task_label(&value, seen);
                    seen.remove(name);
                    return found;
                }
                let fields = self.fields.borrow();
                if fields.contains_key(name) || fields.contains_key(&prefixed_field(name)) {
                    return Some(name.clone());
                }
                drop(fields);
                seen.remove(name);
                self.parent
                    .and_then(|parent| parent.ident_task_label(expr, seen))
            }
            Expr::Select(_, field, _, _) => Some(field.clone()),
            Expr::Index(_, index, _) => selector_leaf_name(index),
            Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
                self.ident_task_label(inner, seen)
            }
            _ => None,
        }
    }
}

fn unwrap_interp_base(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            unwrap_interp_base(inner)
        }
        other => other,
    }
}

fn select_value(value: &Value, field: &str, quoted: bool, span: Span) -> Result<Value, LangError> {
    let missing = || {
        LangError::eval(
            span.line,
            span.col,
            format!("field `{field}` not found in struct"),
        )
    };
    match value.concretize() {
        Value::Struct(map) => {
            if !quoted && field.starts_with('#') {
                return Err(missing());
            }
            let selected = if quoted {
                quoted_lookup(map, field)
            } else {
                Frame::map_identifier_key(map, field).and_then(|key| map.get(key))
            };
            selected.cloned().ok_or_else(missing)
        }
        Value::Definition {
            fields,
            definitions,
            ..
        } => {
            let selected = if quoted {
                quoted_lookup(fields, field).cloned()
            } else if field.starts_with('#') {
                definitions.get(field).cloned()
            } else {
                Frame::map_identifier_key(fields, field)
                    .and_then(|key| fields.get(key))
                    .cloned()
            };
            selected.ok_or_else(missing)
        }
        other => Err(LangError::eval(
            span.line,
            span.col,
            format!("cannot select `{field}` from {}", other.type_name()),
        )),
    }
}

fn params_cache_key(path: &[(String, bool)]) -> Vec<String> {
    path.iter()
        .map(|(segment, quoted)| {
            if *quoted {
                format!("\u{5}{segment}")
            } else {
                segment.clone()
            }
        })
        .collect()
}

fn params_select_path(expr: &Expr, frame: &Frame<'_>) -> Option<Vec<(String, bool)>> {
    match expr {
        Expr::Ident(name, _) if name == "params" => Some(Vec::new()),
        Expr::Select(base, field, quoted, _) => {
            let mut path = params_select_path(base, frame)?;
            path.push((field.clone(), *quoted));
            Some(path)
        }
        Expr::Index(base, index, _) => {
            let mut path = params_select_path(base, frame)?;
            let key = match &**index {
                Expr::Str(parts, _)
                    if parts.iter().all(|part| matches!(part, StrPart::Text(_))) =>
                {
                    let mut text = String::new();
                    for part in parts {
                        if let StrPart::Text(value) = part {
                            text.push_str(value);
                        }
                    }
                    text
                }
                other => match eval_expr(other, frame).ok()?.concretize() {
                    Value::String(key) => key.clone(),
                    _ => return None,
                },
            };
            path.push((key, true));
            Some(path)
        }
        _ => None,
    }
}

fn eval_expr(expr: &Expr, frame: &Frame<'_>) -> Result<Value, LangError> {
    stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || {
        eval_expr_impl(expr, frame)
    })
}

fn eval_expr_impl(expr: &Expr, frame: &Frame<'_>) -> Result<Value, LangError> {
    match expr {
        Expr::Null(_) => Ok(Value::Null),
        Expr::Bool(value, _) => Ok(Value::Bool(*value)),
        Expr::Int(value, _) => Ok(Value::Int(*value)),
        Expr::BigInt(value, _) => Ok(Value::BigInt(value.clone())),
        Expr::Float(value, _) => Ok(Value::Float(value.clone())),
        Expr::Bytes(value, _) => Ok(Value::Bytes(value.clone())),
        Expr::BytesInterp(parts, span) => {
            let mut out = Vec::new();
            for part in parts {
                match part {
                    crate::ast::BytePart::Text(bytes) => out.extend_from_slice(bytes),
                    crate::ast::BytePart::Interp(expr) => {
                        let value = eval_expr(expr, frame)?;
                        let concrete = value.concretize();
                        if concrete.to_json().is_none() {
                            if frame.allows_incomplete(expr) {
                                return Ok(Value::Type(TypeKind::Bytes));
                            }
                            if frame.defer_incomplete.get() {
                                return Ok(Value::Type(TypeKind::Bytes));
                            }
                            return Err(LangError::incomplete_value(
                                span.line,
                                span.col,
                                "non-concrete value in bytes interpolation",
                            ));
                        }
                        match &concrete {
                            Value::Bytes(bytes) => out.extend_from_slice(bytes),
                            Value::String(text) => out.extend_from_slice(text.as_bytes()),
                            Value::Bool(_) | Value::Int(_) | Value::BigInt(_) | Value::Float(_) => {
                                out.extend_from_slice(concrete.display().as_bytes());
                            }
                            other => {
                                return Err(LangError::conflict(
                                    span.line,
                                    span.col,
                                    format!("cannot interpolate {}", other.type_name()),
                                ));
                            }
                        }
                    }
                }
            }
            Ok(Value::Bytes(out))
        }
        Expr::Str(parts, _) => {
            let mut out = String::new();
            for part in parts {
                match part {
                    StrPart::Text(text) => out.push_str(text),
                    StrPart::Interp(expr) => {
                        let value = eval_expr(expr, frame)?;
                        let concrete = value.concretize();
                        if concrete.to_json().is_none() {
                            if frame.allows_incomplete(expr) {
                                return Ok(Value::Type(TypeKind::String));
                            }
                            if frame.defer_incomplete.get() {
                                return Ok(Value::Type(TypeKind::String));
                            }
                            return Err(LangError::incomplete_value(
                                1,
                                1,
                                "non-concrete value in string interpolation",
                            ));
                        }
                        match &concrete {
                            Value::Bytes(bytes) => {
                                out.push_str(&String::from_utf8_lossy(bytes));
                            }
                            Value::String(_)
                            | Value::Bool(_)
                            | Value::Int(_)
                            | Value::BigInt(_)
                            | Value::Float(_) => {
                                out.push_str(&concrete.display());
                            }
                            other => {
                                return Err(LangError::conflict(
                                    1,
                                    1,
                                    format!("cannot interpolate {}", other.type_name()),
                                ));
                            }
                        }
                    }
                }
            }
            Ok(Value::String(out))
        }
        Expr::Ident(name, span) => frame.lookup(name, *span),
        Expr::Select(base, field, quoted, span) => {
            if let Some(value) = self_definition_select(frame, base, field, *quoted, *span)? {
                return Ok(value);
            }
            if let Some(path) = params_select_path(expr, frame)
                && !path.is_empty()
                && frame.params_context.borrow().is_some()
                && !frame.params_shadowed()
            {
                return frame.params_path_value(&path, *span);
            }
            let base = eval_expr(base, frame)?.concretize().clone();
            if frame.allows_incomplete(expr)
                && value_is_pending(&base)
                && !matches!(base, Value::Struct(_) | Value::Definition { .. })
            {
                // Structs still support selection: the selected field may
                // already be concrete (for example an injected task response).
                return Ok(Value::Bottom);
            }
            if frame.defer_incomplete.get() && matches!(base, Value::Bottom) {
                return Ok(Value::Bottom);
            }
            match base {
                Value::Struct(map) => {
                    if !*quoted && field.starts_with('#') {
                        return Err(LangError::eval(
                            span.line,
                            span.col,
                            format!("field `{field}` not found in struct"),
                        ));
                    }
                    let selected = if *quoted {
                        quoted_lookup(&map, field)
                    } else {
                        Frame::map_identifier_key(&map, field).and_then(|key| map.get(key))
                    };
                    selected.cloned().ok_or_else(|| {
                        LangError::eval(
                            span.line,
                            span.col,
                            format!("field `{field}` not found in struct"),
                        )
                    })
                }
                Value::Definition {
                    fields,
                    definitions,
                    optional,
                    ..
                } => {
                    let selected = if *quoted {
                        quoted_lookup(&fields, field).cloned()
                    } else if field.starts_with('#') {
                        definitions.get(field).cloned()
                    } else {
                        Frame::map_identifier_key(&fields, field)
                            .and_then(|key| fields.get(key))
                            .cloned()
                    };
                    if let Some(value) = selected {
                        Ok(value)
                    } else if optional.contains_key(field) {
                        if frame.defer_incomplete.get() {
                            Ok(Value::Bottom)
                        } else {
                            Err(LangError::incomplete_optional(
                                span.line,
                                span.col,
                                format!("cannot reference optional field `{field}`"),
                            ))
                        }
                    } else {
                        Err(LangError::eval(
                            span.line,
                            span.col,
                            format!("field `{field}` not found in struct"),
                        ))
                    }
                }
                other => Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("cannot select `{field}` from {}", other.type_name()),
                )),
            }
        }
        Expr::Bound(op, operand, span) => {
            let operand = eval_expr(operand, frame)?;
            if operand.to_json().is_none() {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "bound has non-concrete value",
                ));
            }
            let kind = match op {
                BoundOp::Ge => BoundKind::Ge,
                BoundOp::Gt => BoundKind::Gt,
                BoundOp::Le => BoundKind::Le,
                BoundOp::Lt => BoundKind::Lt,
                BoundOp::Ne => BoundKind::Ne,
                BoundOp::Match => BoundKind::Match,
                BoundOp::NotMatch => BoundKind::NotMatch,
            };
            validate_bound_operand(kind, &operand, *span)?;
            Ok(Value::Bounds(BoundSet {
                items: vec![Bound { kind, operand }],
                ty: None,
            }))
        }
        Expr::Paren(inner, _) => eval_expr(inner, frame),
        Expr::Call(callee, arguments, span) => {
            let callee = eval_expr(callee, frame)?;
            let mut values = Vec::with_capacity(arguments.len());
            for argument in arguments {
                values.push(eval_expr(argument, frame)?.concretize().clone());
            }
            let pending = values.iter().any(value_is_pending);
            if pending && frame.allows_incomplete(expr) {
                if let Value::Builtin(builtin) = &callee
                    && let Some(value) = deferred_builtin_value(*builtin)
                {
                    return Ok(value);
                }
                if frame.defer_incomplete.get() {
                    return Ok(Value::Bottom);
                }
            }
            if frame.defer_incomplete.get()
                && values.iter().any(|value| matches!(value, Value::Bottom))
            {
                return Ok(Value::Bottom);
            }
            apply_builtin(&callee, &values, *span)
        }
        Expr::Index(base, index, span) => {
            if let Expr::Str(parts, _) = &**index {
                let mut key = String::new();
                let mut literal = true;
                for part in parts {
                    match part {
                        StrPart::Text(text) => key.push_str(text),
                        StrPart::Interp(_) => {
                            literal = false;
                            break;
                        }
                    }
                }
                if literal
                    && let Some(value) = self_definition_select(frame, base, &key, true, *span)?
                {
                    return Ok(value);
                }
            }
            if frame.params_context.borrow().is_some()
                && !frame.params_shadowed()
                && let Some(mut path) = params_select_path(base, frame)
                && let Value::String(key) = eval_expr(index, frame)?.concretize()
            {
                path.push((key.clone(), true));
                return frame.params_path_value(&path, *span);
            }
            let base = eval_expr(base, frame)?.concretize().clone();
            let index = eval_expr(index, frame)?.concretize().clone();
            if frame.allows_incomplete(expr) && value_is_pending(&base) {
                match &base {
                    Value::OpenList(Some(element))
                        if matches!(index, Value::Int(_) | Value::BigInt(_)) =>
                    {
                        return Ok((**element).clone());
                    }
                    Value::List(_)
                    | Value::OpenListPrefix(..)
                    | Value::Struct(_)
                    | Value::Definition { .. } => {}
                    _ => return Ok(Value::Bottom),
                }
            }
            if frame.defer_incomplete.get()
                && (matches!(base, Value::Bottom) || matches!(index, Value::Bottom))
            {
                return Ok(Value::Bottom);
            }
            match (base, index) {
                (Value::List(list) | Value::OpenListPrefix(list, _), Value::Int(index)) => {
                    let index = usize::try_from(index).map_err(|_| {
                        LangError::eval(span.line, span.col, "list index must be non-negative")
                    })?;
                    list.get(index).cloned().ok_or_else(|| {
                        LangError::eval(span.line, span.col, "list index out of range")
                    })
                }
                (Value::List(list) | Value::OpenListPrefix(list, _), Value::BigInt(index)) => {
                    let index = usize::try_from(&index).map_err(|_| {
                        LangError::eval(span.line, span.col, "list index must be non-negative")
                    })?;
                    list.get(index).cloned().ok_or_else(|| {
                        LangError::eval(span.line, span.col, "list index out of range")
                    })
                }
                (Value::Struct(map), Value::String(key)) => {
                    quoted_lookup(&map, &key).cloned().ok_or_else(|| {
                        LangError::eval(
                            span.line,
                            span.col,
                            format!("field `{key}` not found in struct"),
                        )
                    })
                }
                (Value::Definition { fields, .. }, Value::String(key)) => {
                    { quoted_lookup(&fields, &key).cloned() }.ok_or_else(|| {
                        LangError::eval(
                            span.line,
                            span.col,
                            format!("field `{key}` not found in struct"),
                        )
                    })
                }
                (other, _) => Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("cannot index into {}", other.type_name()),
                )),
            }
        }
        Expr::Struct(items, span) => frame.eval_struct_items(items, *span, false),
        Expr::List(items, _) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(eval_expr(item, frame)?);
            }
            Ok(Value::List(out))
        }
        Expr::OpenList(element, _) => Ok(Value::OpenList(match element {
            Some(element) => Some(Box::new(eval_expr(element, frame)?)),
            None => None,
        })),
        Expr::ListComp { clauses, body, .. } => {
            let mut out = Vec::new();
            eval_list_comp(clauses, 0, body, frame, &mut out)?;
            Ok(Value::List(out))
        }
        Expr::OpenListPrefix(prefix, tail, _) => {
            let mut values = Vec::with_capacity(prefix.len());
            for item in prefix {
                values.push(eval_expr(item, frame)?);
            }
            let tail = match tail {
                Some(tail) => Some(Box::new(eval_expr(tail, frame)?)),
                None => None,
            };
            Ok(Value::OpenListPrefix(values, tail))
        }
        Expr::Unify(left, right, span) => {
            let mut operands = Vec::new();
            collect_unify_operands(expr, &mut operands);
            let pending_injections = frame.next_injections.borrow().clone();
            if (operands.len() > 2 || pending_injections.is_some())
                && let Some(base_index) = split_base_index(&operands)
                && operands
                    .iter()
                    .enumerate()
                    .all(|(index, operand)| index == base_index || expr_is_struct_literal(operand))
            {
                let mut injections = pending_injections.unwrap_or_default();
                frame.next_injections.borrow_mut().take();
                let mut override_values = Vec::new();
                let mut merge_failed = false;
                for (index, operand) in operands.iter().enumerate() {
                    if index == base_index {
                        continue;
                    }
                    let value = eval_expr(operand, frame)?;
                    if let Some(fields) = concrete_fields(&value) {
                        merge_injection_fields(&mut injections, fields, *span)?;
                    } else {
                        merge_failed = true;
                    }
                    override_values.push(value);
                }
                if !merge_failed {
                    let base = operands[base_index];
                    let base_value = match strip_expr_parens(base) {
                        Expr::Ident(name, _) => {
                            match eval_definition_with_injections(
                                frame,
                                name,
                                injections.clone(),
                                *span,
                            )? {
                                Some(value) => value,
                                None => eval_expr(base, frame)?,
                            }
                        }
                        Expr::Struct(_, _) => {
                            let previous = frame.next_injections.replace(Some(injections.clone()));
                            let value = eval_expr(base, frame);
                            frame.next_injections.replace(previous);
                            value?
                        }
                        _ => eval_expr(base, frame)?,
                    };
                    let mut result = base_value;
                    for value in override_values {
                        result = unify_values(&result, &value, *span)?;
                    }
                    return Ok(result);
                }
            }
            if let Some(value) = try_definition_unify(left, right, frame, *span)? {
                return Ok(value);
            }
            if let Some(value) = try_definition_unify(right, left, frame, *span)? {
                return Ok(value);
            }
            let left = eval_expr(left, frame)?;
            let right = eval_expr(right, frame)?;
            unify_values(&left, &right, *span)
        }
        Expr::Disjunction(_, _, span) => {
            let mut ast_alternatives = Vec::new();
            collect_disjunction(expr, &mut ast_alternatives);
            let mut alternatives = Vec::new();
            let mut direct = Vec::new();
            let mut inherited = Vec::new();
            let mut optional_error = None;
            for alternative in ast_alternatives {
                match alternative {
                    Expr::Default(inner, _) => {
                        let value = match eval_expr(inner, frame) {
                            Ok(value) => value,
                            Err(err) if err.is_conflict() => continue,
                            Err(err) if is_soft_alternative_error(&err) => {
                                hard_error_beneath_soft(inner, frame)?;
                                if is_optional_incomplete_error(&err) && optional_error.is_none() {
                                    optional_error = Some(err);
                                }
                                continue;
                            }
                            Err(err) => return Err(err),
                        };
                        collect_disjunction_value(
                            &mut alternatives,
                            &mut direct,
                            &mut inherited,
                            value,
                            DefaultMode::Direct,
                        );
                    }
                    other => {
                        let value = match eval_expr(other, frame) {
                            Ok(value) => value,
                            Err(err) if err.is_conflict() => continue,
                            Err(err) if is_soft_alternative_error(&err) => {
                                hard_error_beneath_soft(other, frame)?;
                                if is_optional_incomplete_error(&err) && optional_error.is_none() {
                                    optional_error = Some(err);
                                }
                                continue;
                            }
                            Err(err) => return Err(err),
                        };
                        let mode = if matches!(value, Value::Disjunction(_, _)) {
                            DefaultMode::Inherited
                        } else {
                            DefaultMode::None
                        };
                        collect_disjunction_value(
                            &mut alternatives,
                            &mut direct,
                            &mut inherited,
                            value,
                            mode,
                        );
                    }
                }
            }
            if alternatives.is_empty() {
                if let Some(err) = optional_error {
                    return Err(err);
                }
                return Err(LangError::eval(span.line, span.col, "empty disjunction"));
            }
            let defaults = if direct.is_empty() { inherited } else { direct };
            Ok(Value::Disjunction(alternatives, defaults))
        }
        Expr::Default(inner, _) => eval_expr(inner, frame),
        Expr::Binary(op, left, right, span) => {
            let left_value = eval_expr(left, frame)?.concretize().clone();
            let right_value = eval_expr(right, frame)?.concretize().clone();
            if frame.allows_incomplete(expr)
                && (value_is_pending(&left_value) || value_is_pending(&right_value))
            {
                return Ok(deferred_binary_value(op, &left_value, &right_value));
            }
            if matches!(op, BinOp::And | BinOp::Or) {
                let left = require_bool(frame, &left_value, *span)?;
                let right = require_bool(frame, &right_value, *span)?;
                return Ok(Value::Bool(if matches!(op, BinOp::And) {
                    left && right
                } else {
                    left || right
                }));
            }
            if !matches!(op, BinOp::Eq | BinOp::Ne)
                || matches!(
                    (&left_value, &right_value),
                    (Value::Disjunction(_, _), _)
                        | (_, Value::Disjunction(_, _))
                        | (Value::Bounds(_), _)
                        | (_, Value::Bounds(_))
                )
            {
                let left_concrete = require_operand(frame, &left_value, left, *span)?;
                let right_concrete = require_operand(frame, &right_value, right, *span)?;
                if left_concrete.is_none() || right_concrete.is_none() {
                    return Ok(Value::Type(TypeKind::Number));
                }
            }
            binary(op, &left_value, &right_value, *span)
        }
        Expr::Unary(op, inner, span) => {
            let value = eval_expr(inner, frame)?.concretize().clone();
            if require_operand(frame, &value, inner, *span)?.is_none() {
                return Ok(Value::Type(match op {
                    UnOp::Not => TypeKind::Bool,
                    UnOp::Neg => TypeKind::Number,
                }));
            }
            match (op, value) {
                (UnOp::Not, Value::Bool(value)) => Ok(Value::Bool(!value)),
                (UnOp::Not, other) => Err(LangError::conflict(
                    span.line,
                    span.col,
                    format!("cannot negate {}", other.type_name()),
                )),
                (UnOp::Neg, Value::Int(value)) => value
                    .checked_neg()
                    .map(Value::Int)
                    .ok_or_else(|| LangError::eval(span.line, span.col, "integer overflow")),
                (UnOp::Neg, Value::BigInt(value)) => Ok(Value::BigInt(-value)),
                (UnOp::Neg, Value::Float(value)) => Ok(Value::Float(-value)),
                (UnOp::Neg, other) => Err(LangError::conflict(
                    span.line,
                    span.col,
                    format!("cannot negate {}", other.type_name()),
                )),
            }
        }
    }
}

fn eval_list_comp(
    clauses: &[CompClause],
    index: usize,
    body: &Expr,
    frame: &Frame<'_>,
    out: &mut Vec<Value>,
) -> Result<(), LangError> {
    let Some(clause) = clauses.get(index) else {
        out.push(eval_expr(body, frame)?);
        return Ok(());
    };
    match clause {
        CompClause::For {
            key,
            value,
            source,
            span,
        } => {
            let source = eval_expr(source, frame)?.concretize().clone();
            let entries: Vec<(Option<Value>, Value)> = match source {
                Value::Struct(map) | Value::Definition { fields: map, .. } => map
                    .into_iter()
                    .filter(|(key, _)| !is_hidden_key(key))
                    .map(|(key, value)| {
                        (
                            Some(Value::String(display_field_name(&key).to_owned())),
                            value,
                        )
                    })
                    .collect(),
                Value::List(values) | Value::OpenListPrefix(values, _) => values
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| {
                        (
                            Some(Value::Int(i64::try_from(index).unwrap_or(i64::MAX))),
                            value,
                        )
                    })
                    .collect(),
                Value::OpenList(_) => Vec::new(),
                other => {
                    return Err(LangError::eval(
                        span.line,
                        span.col,
                        format!(
                            "`for ... in` requires a struct or list, found {}",
                            other.type_name()
                        ),
                    ));
                }
            };
            for (entry_key, entry_value) in entries {
                let mut bindings = HashMap::new();
                if let Some(key) = key
                    && let Some(entry_key) = entry_key
                {
                    bindings.insert(key.clone(), entry_key);
                }
                bindings.insert(value.clone(), entry_value);
                let child = Frame::new(&[], Some(frame), bindings)?;
                eval_list_comp(clauses, index + 1, body, &child, out)?;
            }
        }
        CompClause::If { cond, span } => {
            if require_bool(frame, &eval_expr(cond, frame)?, *span)? {
                eval_list_comp(clauses, index + 1, body, frame, out)?;
            }
        }
    }
    Ok(())
}

fn merge_generated_optional(
    optionals: &RefCell<BTreeMap<String, Value>>,
    name: &str,
    value: Value,
) -> Result<(), LangError> {
    let existing = optionals.borrow().get(name).cloned();
    let merged = match existing {
        Some(existing) => match unify_values(&existing, &value, Span::default()) {
            Ok(value) => value,
            Err(err) => Value::Conflict(err.to_string()),
        },
        None => value,
    };
    optionals.borrow_mut().insert(name.to_owned(), merged);
    Ok(())
}

fn close_composed_definition(value: &Value) -> Value {
    stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || {
        close_composed_definition_impl(value)
    })
}

fn close_composed_definition_impl(value: &Value) -> Value {
    match value {
        Value::Disjunction(alternatives, defaults) => Value::Disjunction(
            alternatives.iter().map(close_composed_definition).collect(),
            defaults.clone(),
        ),
        Value::Definition {
            fields,
            optional,
            definitions,
            patterns,
            open,
            admissions,
            ..
        } => {
            let fields: BTreeMap<String, Value> = fields
                .iter()
                .map(|(key, value)| (key.clone(), close_nested(value)))
                .collect();
            let optional: BTreeMap<String, Value> = optional
                .iter()
                .map(|(key, value)| (key.clone(), close_nested(value)))
                .collect();
            let closed = !*open;
            let mut admissions = admissions.clone();
            if !*open {
                let mut names: BTreeSet<String> = fields.keys().cloned().collect();
                names.extend(optional.keys().cloned());
                admissions.push(Admission {
                    names,
                    patterns: patterns
                        .iter()
                        .map(|(pattern, _)| pattern.clone())
                        .collect(),
                });
            }
            Value::Definition {
                fields,
                optional,
                definitions: definitions.clone(),
                patterns: patterns.clone(),
                closed,
                open: *open,
                admissions,
            }
        }
        other => close_nested(other),
    }
}

fn close_nested(value: &Value) -> Value {
    stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || {
        close_nested_impl(value)
    })
}

fn close_nested_impl(value: &Value) -> Value {
    match value {
        Value::Struct(fields) => {
            let fields: BTreeMap<String, Value> = fields
                .iter()
                .map(|(key, value)| (key.clone(), close_nested(value)))
                .collect();
            let names = fields.keys().cloned().collect();
            Value::Definition {
                fields,
                optional: BTreeMap::new(),
                definitions: BTreeMap::new(),
                patterns: Vec::new(),
                closed: true,
                open: false,
                admissions: vec![Admission {
                    names,
                    patterns: Vec::new(),
                }],
            }
        }
        Value::Definition {
            fields,
            optional,
            definitions,
            patterns,
            closed,
            open,
            admissions,
        } => {
            let fields: BTreeMap<String, Value> = fields
                .iter()
                .map(|(key, value)| (key.clone(), close_nested(value)))
                .collect();
            let optional: BTreeMap<String, Value> = optional
                .iter()
                .map(|(key, value)| (key.clone(), close_nested(value)))
                .collect();
            let admissions = if *open {
                Vec::new()
            } else if admissions.is_empty() {
                let mut names: BTreeSet<String> = fields.keys().cloned().collect();
                names.extend(optional.keys().cloned());
                vec![Admission {
                    names,
                    patterns: patterns
                        .iter()
                        .map(|(pattern, _)| pattern.clone())
                        .collect(),
                }]
            } else {
                admissions.clone()
            };
            Value::Definition {
                fields,
                optional,
                definitions: definitions.clone(),
                patterns: patterns.clone(),
                closed: *closed || !*open,
                open: *open,
                admissions,
            }
        }
        Value::List(values) => Value::List(values.iter().map(close_nested).collect()),
        Value::OpenList(Some(element)) => Value::OpenList(Some(Box::new(close_nested(element)))),
        Value::OpenList(None) => Value::OpenList(None),
        Value::OpenListPrefix(prefix, tail) => Value::OpenListPrefix(
            prefix.iter().map(close_nested).collect(),
            tail.as_ref().map(|tail| Box::new(close_nested(tail))),
        ),
        Value::Disjunction(alternatives, defaults) => Value::Disjunction(
            alternatives.iter().map(close_nested).collect(),
            defaults.clone(),
        ),
        other => other.clone(),
    }
}

/// Remove package-hidden `_name` fields from an imported package value.
#[must_use]
pub fn strip_hidden(value: Value) -> Value {
    match value {
        Value::Struct(fields) => Value::Struct(
            fields
                .into_iter()
                .filter(|(key, _)| !is_hidden_key(key))
                .map(|(key, value)| (key, strip_hidden(value)))
                .collect(),
        ),
        Value::Definition {
            fields,
            optional,
            definitions,
            patterns,
            closed,
            open,
            admissions,
        } => Value::Definition {
            fields: fields
                .into_iter()
                .filter(|(key, _)| !is_hidden_key(key))
                .map(|(key, value)| (key, strip_hidden(value)))
                .collect(),
            optional: optional
                .into_iter()
                .filter(|(key, _)| !is_hidden_key(key))
                .map(|(key, value)| (key, strip_hidden(value)))
                .collect(),
            definitions: definitions
                .into_iter()
                .map(|(key, value)| (key, strip_hidden(value)))
                .collect(),
            patterns: patterns
                .into_iter()
                .map(|(pattern, value)| (pattern, strip_hidden(value)))
                .collect(),
            closed,
            open,
            admissions,
        },
        Value::List(values) => Value::List(values.into_iter().map(strip_hidden).collect()),
        Value::Disjunction(alternatives, defaults) => Value::Disjunction(
            alternatives.into_iter().map(strip_hidden).collect(),
            defaults,
        ),
        other => other,
    }
}

fn admitted(admissions: &[Admission], key: &str, span: Span) -> Result<bool, LangError> {
    if admissions.is_empty() || is_hidden_key(key) {
        return Ok(true);
    }
    let label = display_field_name(key);
    for admission in admissions {
        if admission.names.contains(key) || admission.names.contains(label) {
            continue;
        }
        let mut matched = false;
        for pattern in &admission.patterns {
            if pattern_matches(pattern, label, span)? {
                matched = true;
                break;
            }
        }
        if !matched {
            return Ok(false);
        }
    }
    Ok(true)
}

fn closed_error(span: Span, key: &str) -> LangError {
    LangError::eval(
        span.line,
        span.col,
        format!("field `{key}` is not allowed by the closed definition"),
    )
}

fn pattern_matches(pattern: &Pattern, key: &str, span: Span) -> Result<bool, LangError> {
    match pattern {
        Pattern::Any => Ok(true),
        Pattern::Regex(regex) => cue_regex(regex)
            .map(|regex| regex.is_match(key))
            .map_err(|err| {
                LangError::eval(span.line, span.col, format!("invalid pattern regex: {err}"))
            }),
    }
}

fn pattern_constraint(
    patterns: &[(Pattern, Value)],
    key: &str,
    value: &Value,
    span: Span,
) -> Result<Option<Value>, LangError> {
    let mut matched = false;
    let mut result = value.clone();
    for (pattern, constraint) in patterns {
        if pattern_matches(pattern, key, span)? {
            matched = true;
            result = unify_values(&result, constraint, span)?;
        }
    }
    Ok(matched.then_some(result))
}

fn pattern_constraint_closed(
    patterns: &[(Pattern, Value)],
    key: &str,
    value: &Value,
    span: Span,
) -> Result<Option<Value>, LangError> {
    let mut matching = Vec::new();
    for (pattern, constraint) in patterns {
        if pattern_matches(pattern, key, span)? {
            matching.push(constraint.clone());
        }
    }
    let Some(first) = matching.first().cloned() else {
        return Ok(None);
    };
    let mut merged = first;
    for constraint in matching.iter().skip(1) {
        merged = unify_values(&merged, constraint, span)?;
    }
    Ok(Some(unify_values(value, &close_nested(&merged), span)?))
}

fn apply_patterns(
    patterns: &[(Pattern, Value)],
    fields: &mut BTreeMap<String, Value>,
    span: Span,
) -> Result<(), LangError> {
    for (key, value) in fields.iter_mut() {
        if is_hidden_key(key) {
            continue;
        }
        if let Some(constrained) =
            pattern_constraint(patterns, display_field_name(key), value, span)?
        {
            *value = constrained;
        }
    }
    Ok(())
}

fn apply_optional_patterns(
    patterns: &[(Pattern, Value)],
    optional: &mut BTreeMap<String, Value>,
    span: Span,
) -> Result<(), LangError> {
    for (key, value) in optional.iter_mut() {
        if is_hidden_key(key) {
            continue;
        }
        for (pattern, _) in patterns {
            pattern_matches(pattern, display_field_name(key), span)?;
        }
        if matches!(value, Value::Conflict(_)) {
            continue;
        }
        match pattern_constraint(patterns, display_field_name(key), value, span) {
            Ok(Some(constrained)) => *value = constrained,
            Ok(None) => {}
            Err(err) if err.to_string().contains("invalid pattern regex") => return Err(err),
            Err(err) => *value = Value::Conflict(err.to_string()),
        }
    }
    Ok(())
}

fn bound_display(bound: &Bound) -> String {
    let op = match bound.kind {
        BoundKind::Ge => ">=",
        BoundKind::Gt => ">",
        BoundKind::Le => "<=",
        BoundKind::Lt => "<",
        BoundKind::Ne => "!=",
        BoundKind::Match => "=~",
        BoundKind::NotMatch => "!~",
    };
    format!("{op}{}", bound.operand.display())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundClass {
    Numeric,
    String,
    Null,
    List,
    Struct,
}

fn bound_class(bound: &Bound) -> Option<BoundClass> {
    match &bound.operand {
        Value::Int(_) | Value::BigInt(_) | Value::Float(_) => Some(BoundClass::Numeric),
        Value::String(_) => Some(BoundClass::String),
        Value::Null => Some(BoundClass::Null),
        Value::List(_) => Some(BoundClass::List),
        Value::Struct(_) | Value::Definition { .. } => Some(BoundClass::Struct),
        _ => None,
    }
}

fn validate_bound_operand(kind: BoundKind, operand: &Value, span: Span) -> Result<(), LangError> {
    let allowed = match kind {
        BoundKind::Ge | BoundKind::Gt | BoundKind::Le | BoundKind::Lt => {
            matches!(
                operand,
                Value::Int(_) | Value::BigInt(_) | Value::Float(_) | Value::String(_)
            )
        }
        BoundKind::Match | BoundKind::NotMatch => {
            matches!(
                operand,
                Value::Int(_) | Value::BigInt(_) | Value::Float(_) | Value::String(_)
            )
        }
        BoundKind::Ne => !matches!(operand, Value::Bool(_)),
    };
    if allowed {
        Ok(())
    } else {
        Err(LangError::conflict(
            span.line,
            span.col,
            format!(
                "invalid value {} for bound {}",
                operand.display(),
                match kind {
                    BoundKind::Ge => ">=",
                    BoundKind::Gt => ">",
                    BoundKind::Le => "<=",
                    BoundKind::Lt => "<",
                    BoundKind::Ne => "!=",
                    BoundKind::Match => "=~",
                    BoundKind::NotMatch => "!~",
                }
            ),
        ))
    }
}

fn bound_type_compatible(kind: BoundKind, ty: TypeKind, operand: &Value) -> bool {
    match kind {
        BoundKind::Ge | BoundKind::Gt | BoundKind::Le | BoundKind::Lt => match ty {
            TypeKind::String => matches!(operand, Value::String(_)),
            TypeKind::Int | TypeKind::Float | TypeKind::Number => {
                matches!(operand, Value::Int(_) | Value::BigInt(_) | Value::Float(_))
            }
            TypeKind::Bool | TypeKind::Bytes => false,
        },
        BoundKind::Match | BoundKind::NotMatch => match ty {
            TypeKind::String => matches!(operand, Value::String(_)),
            TypeKind::Int | TypeKind::Float | TypeKind::Number => {
                matches!(operand, Value::Int(_) | Value::BigInt(_) | Value::Float(_))
            }
            TypeKind::Bool | TypeKind::Bytes => false,
        },
        BoundKind::Ne => match ty {
            TypeKind::String => matches!(operand, Value::String(_) | Value::Null),
            TypeKind::Int | TypeKind::Float | TypeKind::Number => {
                matches!(
                    operand,
                    Value::Int(_) | Value::BigInt(_) | Value::Float(_) | Value::Null
                )
            }
            TypeKind::Bool | TypeKind::Bytes => matches!(operand, Value::Null),
        },
    }
}

fn bound_matches(bound: &Bound, value: &Value, span: Span) -> Result<bool, LangError> {
    match bound.kind {
        BoundKind::Ne => {
            let compatible = match bound_class(bound) {
                Some(BoundClass::Null) => !matches!(value, Value::Null),
                Some(BoundClass::Numeric) => {
                    matches!(value, Value::Int(_) | Value::BigInt(_) | Value::Float(_))
                }
                Some(BoundClass::String) => matches!(value, Value::String(_)),
                Some(BoundClass::List) => matches!(value, Value::List(_)),
                Some(BoundClass::Struct) => {
                    matches!(value, Value::Struct(_) | Value::Definition { .. })
                }
                None => false,
            };
            if !compatible {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    format!(
                        "invalid value {} (out of bound {})",
                        value.display(),
                        bound_display(bound)
                    ),
                ));
            }
            return Ok(!values_equal(value, &bound.operand));
        }
        BoundKind::Match | BoundKind::NotMatch => {
            let Value::String(text) = value else {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    format!("bound `{}` requires a string", bound_display(bound)),
                ));
            };
            let Value::String(regex) = &bound.operand else {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    format!("bound `{}` requires a string operand", bound_display(bound)),
                ));
            };
            let matched = cue_regex(regex)
                .map(|regex| regex.is_match(text))
                .map_err(|err| {
                    LangError::eval(span.line, span.col, format!("invalid regex: {err}"))
                })?;
            return Ok(matched == (bound.kind == BoundKind::Match));
        }
        _ => {}
    }
    let Some(ordering) = ordered_compare(value, &bound.operand) else {
        return Err(LangError::conflict(
            span.line,
            span.col,
            format!("bound `{}` is not comparable", bound_display(bound)),
        ));
    };
    Ok(match bound.kind {
        BoundKind::Ge => ordering.is_ge(),
        BoundKind::Gt => ordering.is_gt(),
        BoundKind::Le => ordering.is_le(),
        BoundKind::Lt => ordering.is_lt(),
        _ => unreachable!("non-ordering bound"),
    })
}

fn ordered_compare(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
        (Value::BigInt(a), Value::BigInt(b)) => a.partial_cmp(b),
        (Value::Int(a), Value::BigInt(b)) => BigInt::from(*a).partial_cmp(b),
        (Value::BigInt(a), Value::Int(b)) => a.partial_cmp(&BigInt::from(*b)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Int(a), Value::Float(b)) => BigDecimal::from(*a).partial_cmp(b),
        (Value::Float(a), Value::Int(b)) => a.partial_cmp(&BigDecimal::from(*b)),
        (Value::BigInt(a), Value::Float(b)) => BigDecimal::from(a.clone()).partial_cmp(b),
        (Value::Float(a), Value::BigInt(b)) => a.partial_cmp(&BigDecimal::from(b.clone())),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

fn merge_bounds(left: &BoundSet, right: &BoundSet, span: Span) -> Result<Value, LangError> {
    let mut items = left.items.clone();
    for bound in &right.items {
        if !items.contains(bound) {
            items.push(bound.clone());
        }
    }
    check_bound_classes(&items, span)?;
    let ty = match (&left.ty, &right.ty) {
        (None, None) => None,
        (Some(ty), None) | (None, Some(ty)) => Some(*ty),
        (Some(a), Some(b)) => match unify_types(*a, *b, span)? {
            Value::Type(ty) => Some(ty),
            _ => None,
        },
    };
    check_bound_interval(&items, ty, span)?;
    Ok(Value::Bounds(BoundSet { items, ty }))
}

fn check_bound_classes(items: &[Bound], span: Span) -> Result<(), LangError> {
    let mut class: Option<BoundClass> = None;
    for bound in items {
        let Some(bound_class) = bound_class(bound) else {
            return Err(LangError::conflict(
                span.line,
                span.col,
                "bound operand has an unsupported type",
            ));
        };
        if bound_class == BoundClass::Null {
            continue;
        }
        match class {
            None => class = Some(bound_class),
            Some(existing) if existing == bound_class => {}
            Some(_) => {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "incompatible bound operand types",
                ));
            }
        }
    }
    Ok(())
}

fn check_bound_interval(
    items: &[Bound],
    ty: Option<TypeKind>,
    span: Span,
) -> Result<(), LangError> {
    let mut lower: Option<(Value, bool)> = None;
    let mut upper: Option<(Value, bool)> = None;
    for bound in items {
        let (value, inclusive, is_lower) = match bound.kind {
            BoundKind::Ge => (&bound.operand, true, true),
            BoundKind::Gt => (&bound.operand, false, true),
            BoundKind::Le => (&bound.operand, true, false),
            BoundKind::Lt => (&bound.operand, false, false),
            _ => continue,
        };
        if is_lower {
            lower = Some(match lower {
                None => (value.clone(), inclusive),
                Some((current, current_inclusive)) => match ordered_compare(value, &current) {
                    Some(std::cmp::Ordering::Greater) => (value.clone(), inclusive),
                    Some(std::cmp::Ordering::Equal) => (current, current_inclusive && inclusive),
                    _ => (current, current_inclusive),
                },
            });
        } else {
            upper = Some(match upper {
                None => (value.clone(), inclusive),
                Some((current, current_inclusive)) => match ordered_compare(value, &current) {
                    Some(std::cmp::Ordering::Less) => (value.clone(), inclusive),
                    Some(std::cmp::Ordering::Equal) => (current, current_inclusive && inclusive),
                    _ => (current, current_inclusive),
                },
            });
        }
    }
    if let (Some((low, low_inclusive)), Some((high, high_inclusive))) = (lower, upper) {
        match ordered_compare(&low, &high) {
            Some(std::cmp::Ordering::Greater) => {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "empty bound interval",
                ));
            }
            Some(std::cmp::Ordering::Equal) if !(low_inclusive && high_inclusive) => {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "empty bound interval",
                ));
            }
            _ => {}
        }
        if ty == Some(TypeKind::Int) {
            let smallest = bound_smallest_integer(&low, low_inclusive);
            let largest = bound_largest_integer(&high, high_inclusive);
            if let (Some(smallest), Some(largest)) = (smallest, largest)
                && smallest > largest
            {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "empty integer bound interval",
                ));
            }
        }
    }
    Ok(())
}

fn bound_decimal(value: &Value) -> Option<BigDecimal> {
    match value {
        Value::Int(value) => Some(BigDecimal::from(*value)),
        Value::BigInt(value) => Some(BigDecimal::from(value.clone())),
        Value::Float(value) => Some(value.clone()),
        _ => None,
    }
}

fn bound_smallest_integer(value: &Value, inclusive: bool) -> Option<BigDecimal> {
    let decimal = bound_decimal(value)?;
    Some(if inclusive {
        decimal.with_scale_round(0, RoundingMode::Ceiling)
    } else {
        decimal.with_scale_round(0, RoundingMode::Floor) + BigDecimal::from(1)
    })
}

fn bound_largest_integer(value: &Value, inclusive: bool) -> Option<BigDecimal> {
    let decimal = bound_decimal(value)?;
    Some(if inclusive {
        decimal.with_scale_round(0, RoundingMode::Floor)
    } else {
        decimal.with_scale_round(0, RoundingMode::Ceiling) - BigDecimal::from(1)
    })
}

fn apply_bounds(bounds: &BoundSet, other: &Value, span: Span) -> Result<Value, LangError> {
    if let Value::Type(kind) = other {
        for bound in &bounds.items {
            if !bound_type_compatible(bound.kind, *kind, &bound.operand) {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    format!(
                        "bound `{}` is incompatible with this type",
                        bound_display(bound)
                    ),
                ));
            }
        }
        let ty = match bounds.ty {
            Some(existing) => match unify_types(existing, *kind, span)? {
                Value::Type(ty) => Some(ty),
                _ => None,
            },
            None => Some(*kind),
        };
        check_bound_interval(&bounds.items, ty, span)?;
        return Ok(Value::Bounds(BoundSet {
            items: bounds.items.clone(),
            ty,
        }));
    }
    if let Some(ty) = bounds.ty
        && !type_matches(ty, other)
    {
        return Err(LangError::conflict(
            span.line,
            span.col,
            format!("{} does not satisfy {}", other.type_name(), kind_name(ty)),
        ));
    }
    for bound in &bounds.items {
        if !bound_matches(bound, other, span)? {
            return Err(LangError::conflict(
                span.line,
                span.col,
                format!(
                    "invalid value {} (out of bound {})",
                    other.display(),
                    bound_display(bound)
                ),
            ));
        }
    }
    Ok(other.clone())
}

fn numeric_kind_mismatch(left: &Value, right: &Value) -> bool {
    matches!(
        (left, right),
        (Value::Int(_), Value::Float(_))
            | (Value::Float(_), Value::Int(_))
            | (Value::BigInt(_), Value::Float(_))
            | (Value::Float(_), Value::BigInt(_))
    )
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).map_or(0, |value| u32::from(*value));
        let b2 = chunk.get(2).map_or(0, |value| u32::from(*value));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((triple >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn list_prefix(value: &Value) -> Option<&[Value]> {
    match value {
        Value::List(values) | Value::OpenListPrefix(values, _) => Some(values),
        Value::OpenList(_) => Some(&[]),
        _ => None,
    }
}

fn values_identical(left: &Value, right: &Value) -> bool {
    stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || {
        values_identical_impl(left, right)
    })
}

fn values_identical_impl(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Recursive(_) | Value::Deferred(_, _), _)
        | (_, Value::Recursive(_) | Value::Deferred(_, _)) => {
            match (left.to_json(), right.to_json()) {
                (Some(left), Some(right)) => left == right,
                _ => false,
            }
        }
        (
            Value::Struct(a) | Value::Definition { fields: a, .. },
            Value::Struct(b) | Value::Definition { fields: b, .. },
        ) => {
            let visible_a: Vec<_> = a.iter().filter(|(key, _)| !is_hidden_key(key)).collect();
            let visible_b: Vec<_> = b.iter().filter(|(key, _)| !is_hidden_key(key)).collect();
            visible_a.len() == visible_b.len()
                && visible_a.iter().all(|(key, value)| {
                    b.get(*key)
                        .is_some_and(|other| values_identical(value, other))
                })
        }
        (left, right) if list_prefix(left).is_some() && list_prefix(right).is_some() => {
            let a = list_prefix(left).unwrap_or_default();
            let b = list_prefix(right).unwrap_or_default();
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| values_identical(a, b))
        }
        (Value::Disjunction(_, _), _) | (_, Value::Disjunction(_, _)) => {
            match (left.to_json(), right.to_json()) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            }
        }
        (Value::Int(a), Value::BigInt(b)) => BigInt::from(*a) == *b,
        (Value::BigInt(a), Value::Int(b)) => *a == BigInt::from(*b),
        (Value::Conflict(_), _) | (_, Value::Conflict(_)) => false,
        _ => left == right,
    }
}

#[allow(clippy::cmp_owned)]
fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => {
            BigDecimal::from(*a) == *b
        }
        (Value::BigInt(a), Value::Float(b)) | (Value::Float(b), Value::BigInt(a)) => {
            BigDecimal::from(a.clone()) == *b
        }
        _ => values_identical(left, right),
    }
}

fn go_case(text: &str, upper: bool) -> String {
    text.chars()
        .map(|character| {
            let simple = match (character, upper) {
                ('\u{0130}', false) => Some('i'),
                ('\u{1F80}'..='\u{1F87}', true) => {
                    Some(char::from_u32(character as u32 + 8).unwrap_or(character))
                }
                ('\u{1F90}'..='\u{1F97}', true) => {
                    Some(char::from_u32(character as u32 + 8).unwrap_or(character))
                }
                ('\u{1FA0}'..='\u{1FA7}', true) => {
                    Some(char::from_u32(character as u32 + 8).unwrap_or(character))
                }
                ('\u{1FB3}', true) => Some('\u{1FBC}'),
                ('\u{1FC3}', true) => Some('\u{1FCC}'),
                ('\u{1FF3}', true) => Some('\u{1FFC}'),
                _ => None,
            };
            if let Some(value) = simple {
                return value;
            }
            let mapped: Vec<char> = if upper {
                character.to_uppercase().collect()
            } else {
                character.to_lowercase().collect()
            };
            if mapped.len() == 1 {
                mapped[0]
            } else {
                character
            }
        })
        .collect()
}

/// Return the builtin package value for a standard import path.
#[must_use]
pub fn builtin_module(path: &str) -> Option<Value> {
    let entries: &[(&str, Builtin)] = match path {
        "strings" => &[
            ("ToUpper", Builtin::StringsToUpper),
            ("ToLower", Builtin::StringsToLower),
            ("TrimSpace", Builtin::StringsTrimSpace),
            ("Join", Builtin::StringsJoin),
            ("Split", Builtin::StringsSplit),
            ("HasPrefix", Builtin::StringsHasPrefix),
            ("HasSuffix", Builtin::StringsHasSuffix),
        ],
        "list" => &[
            ("Range", Builtin::ListRange),
            ("Concat", Builtin::ListConcat),
        ],
        "regexp" => &[
            ("Match", Builtin::RegexpMatch),
            ("Find", Builtin::RegexpFind),
            ("FindAll", Builtin::RegexpFindAll),
            ("ReplaceAll", Builtin::RegexpReplaceAll),
            ("QuoteMeta", Builtin::RegexpQuoteMeta),
            ("Valid", Builtin::RegexpValid),
        ],
        "math" => &[
            ("Abs", Builtin::MathAbs),
            ("Floor", Builtin::MathFloor),
            ("Ceil", Builtin::MathCeil),
            ("Round", Builtin::MathRound),
            ("Trunc", Builtin::MathTrunc),
            ("Sqrt", Builtin::MathSqrt),
            ("Pow", Builtin::MathPow),
            ("Exp", Builtin::MathExp),
            ("Log", Builtin::MathLog),
        ],
        "time" => &[
            ("Unix", Builtin::TimeUnix),
            ("Parse", Builtin::TimeParse),
            ("Format", Builtin::TimeFormat),
            ("ParseDuration", Builtin::TimeParseDuration),
            ("Duration", Builtin::TimeDuration),
            ("Time", Builtin::TimeTime),
            ("FormatString", Builtin::TimeFormatString),
            ("ToUnix", Builtin::TimeToUnix),
            ("ToUnixNano", Builtin::TimeToUnixNano),
            ("Split", Builtin::TimeSplit),
            ("FormatDuration", Builtin::TimeFormatDuration),
        ],
        _ => return None,
    };
    let mut values: BTreeMap<String, Value> = entries
        .iter()
        .map(|(name, builtin)| ((*name).to_owned(), Value::Builtin(*builtin)))
        .collect();
    if path == "time" {
        for (name, literal) in [
            ("ANSIC", "Mon Jan _2 15:04:05 2006"),
            ("UnixDate", "Mon Jan _2 15:04:05 MST 2006"),
            ("RubyDate", "Mon Jan 02 15:04:05 -0700 2006"),
            ("RFC822", "02 Jan 06 15:04 MST"),
            ("RFC822Z", "02 Jan 06 15:04 -0700"),
            ("RFC850", "Monday, 02-Jan-06 15:04:05 MST"),
            ("RFC1123", "Mon, 02 Jan 2006 15:04:05 MST"),
            ("RFC1123Z", "Mon, 02 Jan 2006 15:04:05 -0700"),
            ("RFC3339", "2006-01-02T15:04:05Z07:00"),
            ("RFC3339Nano", "2006-01-02T15:04:05.999999999Z07:00"),
            ("RFC3339Date", "2006-01-02"),
            ("Kitchen", "3:04PM"),
            ("Kitchen24", "15:04"),
        ] {
            values.insert(name.to_owned(), Value::String(literal.to_owned()));
        }
        for (name, literal) in [
            ("Nanosecond", 1i64),
            ("Microsecond", 1_000),
            ("Millisecond", 1_000_000),
            ("Second", 1_000_000_000),
            ("Minute", 60_000_000_000),
            ("Hour", 3_600_000_000_000),
            ("January", 1),
            ("February", 2),
            ("March", 3),
            ("April", 4),
            ("May", 5),
            ("June", 6),
            ("July", 7),
            ("August", 8),
            ("September", 9),
            ("October", 10),
            ("November", 11),
            ("December", 12),
            ("Sunday", 0),
            ("Monday", 1),
            ("Tuesday", 2),
            ("Wednesday", 3),
            ("Thursday", 4),
            ("Friday", 5),
            ("Saturday", 6),
        ] {
            values.insert(name.to_owned(), Value::Int(literal));
        }
    }
    if path == "math" {
        for (name, literal) in [
            (
                "Pi",
                "3.14159265358979323846264338327950288419716939937510582097494459",
            ),
            (
                "E",
                "2.71828182845904523536028747135266249775724709369995957496696763",
            ),
        ] {
            if let Ok(value) = literal.parse::<BigDecimal>() {
                values.insert(name.to_owned(), Value::Float(value));
            }
        }
    }
    Some(Value::Struct(values))
}

fn regex_arg(arguments: &[Value], index: usize, span: Span) -> Result<&str, LangError> {
    match arguments.get(index) {
        Some(Value::String(value)) => Ok(value),
        _ => Err(LangError::conflict(
            span.line,
            span.col,
            "regexp function requires string arguments",
        )),
    }
}

fn quantifier_end(chars: &[char], start: usize) -> Option<usize> {
    if chars.get(start) != Some(&'{') {
        return None;
    }
    let mut index = start + 1;
    let digits_start = index;
    while index < chars.len() && chars[index].is_ascii_digit() {
        index += 1;
    }
    if index == digits_start {
        return None;
    }
    if chars.get(index) == Some(&',') {
        index += 1;
        while index < chars.len() && chars[index].is_ascii_digit() {
            index += 1;
        }
    }
    if chars.get(index) == Some(&'}') {
        Some(index)
    } else {
        None
    }
}

fn escape_invalid_braces(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len());
    let mut index = 0usize;
    let mut in_class = false;
    while index < chars.len() {
        let character = chars[index];
        match character {
            '\\' => {
                out.push(character);
                index += 1;
                if let Some(next) = chars.get(index) {
                    out.push(*next);
                    index += 1;
                }
            }
            '[' if !in_class => {
                in_class = true;
                out.push(character);
                index += 1;
            }
            ']' if in_class => {
                in_class = false;
                out.push(character);
                index += 1;
            }
            '{' if !in_class => {
                if let Some(end) = quantifier_end(&chars, index) {
                    for value in &chars[index..=end] {
                        out.push(*value);
                    }
                    index = end + 1;
                } else {
                    out.push('\\');
                    out.push('{');
                    index += 1;
                }
            }
            '}' if !in_class => {
                out.push('\\');
                out.push('}');
                index += 1;
            }
            _ => {
                out.push(character);
                index += 1;
            }
        }
    }
    out
}

fn validate_quantifiers(pattern: &str) -> Result<(), String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut index = 0usize;
    let mut in_class = false;
    let mut last_quantifier = false;
    let mut lazy_used = false;
    while index < chars.len() {
        let character = chars[index];
        match character {
            '\\' => {
                if chars.get(index + 1) == Some(&'Q') {
                    index += 2;
                    while index < chars.len() {
                        if chars[index] == '\\' && chars.get(index + 1) == Some(&'E') {
                            index += 2;
                            break;
                        }
                        index += 1;
                    }
                } else {
                    index += 2;
                }
                last_quantifier = false;
                lazy_used = false;
                continue;
            }
            '[' if !in_class => {
                in_class = true;
                index += 1;
                last_quantifier = false;
                lazy_used = false;
                continue;
            }
            ']' if in_class => {
                in_class = false;
                index += 1;
                last_quantifier = false;
                lazy_used = false;
                continue;
            }
            '*' | '+' | '?' if !in_class => {
                if last_quantifier {
                    if character == '?' && !lazy_used {
                        lazy_used = true;
                    } else {
                        return Err("invalid nested repetition operator".to_owned());
                    }
                } else {
                    last_quantifier = true;
                    lazy_used = false;
                }
                index += 1;
                continue;
            }
            '{' if !in_class => {
                if let Some(end) = quantifier_end(&chars, index) {
                    if last_quantifier {
                        return Err("invalid nested repetition operator".to_owned());
                    }
                    last_quantifier = true;
                    lazy_used = false;
                    index = end + 1;
                    continue;
                }
            }
            _ => {}
        }
        last_quantifier = false;
        lazy_used = false;
        index += 1;
    }
    Ok(())
}

fn translate_re2_pattern(pattern: &str) -> Result<String, String> {
    let escaped = escape_invalid_braces(pattern);
    let pattern = escaped.as_str();
    validate_quantifiers(pattern)?;
    let mut out = String::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();
    let mut in_class = false;
    while let Some(character) = chars.next() {
        match character {
            '\\' => {
                let Some(next) = chars.next() else {
                    out.push('\\');
                    break;
                };
                match next {
                    'Q' if !in_class => {
                        let mut literal = String::new();
                        while let Some(value) = chars.next() {
                            if value == '\\' && chars.peek() == Some(&'E') {
                                chars.next();
                                break;
                            }
                            literal.push(value);
                        }
                        out.push_str(&regex::escape(&literal));
                    }
                    'd' => out.push_str(if in_class { "0-9" } else { "[0-9]" }),
                    'D' => out.push_str(if in_class {
                        "\\x00-\\x2F\\x3A-\\x{10FFFF}"
                    } else {
                        "[^0-9]"
                    }),
                    'w' => out.push_str(if in_class {
                        "0-9A-Za-z_"
                    } else {
                        "[0-9A-Za-z_]"
                    }),
                    'W' => out.push_str(if in_class {
                        "\\x00-\\x2F\\x3A-\\x40\\x5B-\\x5E\\x60\\x7B-\\x{10FFFF}"
                    } else {
                        "[^0-9A-Za-z_]"
                    }),
                    's' => out.push_str(if in_class {
                        "\\t\\n\\f\\r "
                    } else {
                        "[\\t\\n\\f\\r ]"
                    }),
                    'S' => out.push_str(if in_class {
                        "\\x00-\\x08\\x0B\\x0E-\\x1F\\x21-\\x{10FFFF}"
                    } else {
                        "[^\\t\\n\\f\\r ]"
                    }),
                    'b' if in_class => return Err("invalid escape sequence".to_owned()),
                    'B' if in_class => return Err("invalid escape sequence".to_owned()),
                    'Q' if in_class => return Err("invalid escape sequence".to_owned()),
                    'b' => out.push_str("(?-u:\\b)"),
                    'B' => out.push_str("(?-u:\\B)"),
                    other => {
                        out.push('\\');
                        out.push(other);
                    }
                }
            }
            '[' => {
                in_class = true;
                out.push(character);
            }
            ']' => {
                in_class = false;
                out.push(character);
            }
            _ => out.push(character),
        }
    }
    check_repeat_limits(pattern)?;
    Ok(out)
}

fn check_repeat_limits(pattern: &str) -> Result<(), String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut index = 0usize;
    let mut in_class = false;
    while index < chars.len() {
        match chars[index] {
            '\\' => {
                if chars.get(index + 1) == Some(&'Q') {
                    index += 2;
                    while index < chars.len() {
                        if chars[index] == '\\' && chars.get(index + 1) == Some(&'E') {
                            index += 2;
                            break;
                        }
                        index += 1;
                    }
                    continue;
                }
                index += 2;
                continue;
            }
            '[' => {
                in_class = true;
                index += 1;
                continue;
            }
            ']' => {
                in_class = false;
                index += 1;
                continue;
            }
            '{' if !in_class => {
                let mut end = index + 1;
                while end < chars.len() && chars[end].is_ascii_digit() {
                    end += 1;
                }
                let mut valid = end > index + 1;
                let mut max = chars[index + 1..end]
                    .iter()
                    .collect::<String>()
                    .parse::<u32>()
                    .unwrap_or(0);
                if valid && chars.get(end) == Some(&',') {
                    let after = end + 1;
                    let mut close = after;
                    while close < chars.len() && chars[close].is_ascii_digit() {
                        close += 1;
                    }
                    if chars.get(close) == Some(&'}') {
                        if close > after {
                            let value = chars[after..close]
                                .iter()
                                .collect::<String>()
                                .parse::<u32>()
                                .unwrap_or(0);
                            max = max.max(value);
                        }
                        end = close;
                    } else {
                        valid = false;
                    }
                } else if valid && chars.get(end) == Some(&'}') {
                } else {
                    valid = false;
                }
                if valid && chars.get(end) == Some(&'}') {
                    if max > 1000 {
                        return Err("invalid repeat count".to_owned());
                    }
                    index = end + 1;
                    continue;
                }
                index += 1;
                continue;
            }
            _ => index += 1,
        }
    }
    Ok(())
}

fn cue_regex(pattern: &str) -> Result<regex::Regex, String> {
    let translated = translate_re2_pattern(pattern)?;
    regex::Regex::new(&translated).map_err(|err| err.to_string())
}

fn regex_compile(pattern: &str, span: Span) -> Result<regex::Regex, LangError> {
    cue_regex(pattern)
        .map_err(|err| LangError::conflict(span.line, span.col, format!("invalid regex: {err}")))
}

fn quote_meta(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(
            character,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        ) {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

fn apply_regexp_builtin(
    builtin: Builtin,
    arguments: &[Value],
    span: Span,
) -> Result<Value, LangError> {
    match builtin {
        Builtin::RegexpMatch => {
            let regex = regex_compile(regex_arg(arguments, 0, span)?, span)?;
            Ok(Value::Bool(regex.is_match(regex_arg(arguments, 1, span)?)))
        }
        Builtin::RegexpFind => {
            let regex = regex_compile(regex_arg(arguments, 0, span)?, span)?;
            let Some(found) = regex.find(regex_arg(arguments, 1, span)?) else {
                return Err(LangError::conflict(span.line, span.col, "no match"));
            };
            Ok(Value::String(found.as_str().to_owned()))
        }
        Builtin::RegexpFindAll => {
            let regex = regex_compile(regex_arg(arguments, 0, span)?, span)?;
            let text = regex_arg(arguments, 1, span)?;
            let limit = match arguments.get(2) {
                Some(Value::Int(limit)) => *limit,
                Some(Value::BigInt(limit)) => {
                    let bound = BigInt::from(u64::MAX);
                    if limit > &bound || limit < &-bound {
                        return Err(LangError::conflict(
                            span.line,
                            span.col,
                            "regexp.FindAll: integer limit overflows int64",
                        ));
                    }
                    if limit < &BigInt::from(0) {
                        -1
                    } else {
                        i64::MAX
                    }
                }
                _ => {
                    return Err(LangError::conflict(
                        span.line,
                        span.col,
                        "FindAll requires an integer limit",
                    ));
                }
            };
            let values: Vec<Value> = if limit < 0 {
                regex
                    .find_iter(text)
                    .map(|found| Value::String(found.as_str().to_owned()))
                    .collect()
            } else {
                regex
                    .find_iter(text)
                    .take(usize::try_from(limit).unwrap_or(0))
                    .map(|found| Value::String(found.as_str().to_owned()))
                    .collect()
            };
            if values.is_empty() {
                return Err(LangError::conflict(span.line, span.col, "no match"));
            }
            Ok(Value::List(values))
        }
        Builtin::RegexpReplaceAll => {
            let regex = regex_compile(regex_arg(arguments, 0, span)?, span)?;
            let text = regex_arg(arguments, 1, span)?;
            let replacement = regex_arg(arguments, 2, span)?;
            Ok(Value::String(
                regex.replace_all(text, replacement).into_owned(),
            ))
        }
        Builtin::RegexpQuoteMeta => Ok(Value::String(quote_meta(regex_arg(arguments, 0, span)?))),
        Builtin::RegexpValid => {
            regex_compile(regex_arg(arguments, 0, span)?, span)?;
            Ok(Value::Bool(true))
        }
        _ => unreachable!("regexp builtin"),
    }
}

fn numeric_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Int(value) => Some(*value as f64),
        Value::BigInt(value) => value.to_string().parse::<f64>().ok(),
        Value::Float(value) => value.to_string().parse::<f64>().ok(),
        _ => None,
    }
}

fn integer_big(value: &Value) -> Option<BigInt> {
    match value {
        Value::Int(value) => Some(BigInt::from(*value)),
        Value::BigInt(value) => Some(value.clone()),
        _ => None,
    }
}

fn value_from_bigint(value: BigInt) -> Value {
    i64::try_from(&value).map_or(Value::BigInt(value), Value::Int)
}

fn numeric_value_decimal(value: &Value) -> Option<BigDecimal> {
    match value {
        Value::Int(value) => Some(BigDecimal::from(*value)),
        Value::BigInt(value) => Some(BigDecimal::from(value.clone())),
        Value::Float(value) => Some(value.clone()),
        _ => None,
    }
}

fn math_decimal(arguments: &[Value], index: usize, span: Span) -> Result<BigDecimal, LangError> {
    match arguments.get(index) {
        Some(Value::Int(value)) => Ok(BigDecimal::from(*value)),
        Some(Value::BigInt(value)) => Ok(BigDecimal::from(value.clone())),
        Some(Value::Float(value)) => Ok(value.clone()),
        _ => Err(LangError::conflict(
            span.line,
            span.col,
            "math function requires a number",
        )),
    }
}

fn math_decimal_number(value: BigDecimal) -> Value {
    let text = value.to_string();
    if let Ok(int) = text.parse::<i64>() {
        Value::Int(int)
    } else {
        Value::Float(value)
    }
}

fn math_float_number(value: f64) -> Value {
    format!("{value}")
        .parse::<BigDecimal>()
        .map(|value| Value::Float(value.normalized()))
        .unwrap_or(Value::Bottom)
}

fn math_number(value: f64) -> Value {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 9.0e15 {
        #[allow(clippy::cast_possible_truncation)]
        return Value::Int(value as i64);
    }
    format!("{value}")
        .parse::<BigDecimal>()
        .map_or(Value::Bottom, Value::Float)
}

fn math_arg(arguments: &[Value], index: usize, span: Span) -> Result<f64, LangError> {
    arguments
        .get(index)
        .and_then(numeric_f64)
        .ok_or_else(|| LangError::conflict(span.line, span.col, "math function requires a number"))
}

fn apply_math_builtin(
    builtin: Builtin,
    arguments: &[Value],
    span: Span,
) -> Result<Value, LangError> {
    match builtin {
        Builtin::MathAbs => match arguments.first() {
            Some(Value::Int(value)) => value
                .checked_abs()
                .map(Value::Int)
                .ok_or_else(|| overflow(span)),
            Some(Value::BigInt(value)) => Ok(Value::BigInt(if value < &BigInt::from(0) {
                -value.clone()
            } else {
                value.clone()
            })),
            Some(Value::Float(value)) => {
                let magnitude = value.abs();
                match magnitude.normalized().to_string().parse::<i64>() {
                    Ok(int) => Ok(Value::Int(int)),
                    Err(_) => Ok(Value::Float(magnitude)),
                }
            }
            _ => Err(LangError::conflict(
                span.line,
                span.col,
                "math.Abs requires a number",
            )),
        },
        Builtin::MathFloor | Builtin::MathCeil | Builtin::MathRound | Builtin::MathTrunc => {
            let decimal = math_decimal(arguments, 0, span)?;
            let rounding = match builtin {
                Builtin::MathFloor => RoundingMode::Floor,
                Builtin::MathCeil => RoundingMode::Ceiling,
                Builtin::MathRound => RoundingMode::HalfUp,
                _ => RoundingMode::Down,
            };
            Ok(math_decimal_number(decimal.with_scale_round(0, rounding)))
        }
        Builtin::MathSqrt => {
            let value = math_arg(arguments, 0, span)?.sqrt();
            if !value.is_finite() {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "math function result is not a number",
                ));
            }
            Ok(math_float_number(value))
        }
        Builtin::MathPow => {
            if let (Some(base), Some(exponent)) = (arguments.first(), arguments.get(1))
                && let Some(base) = integer_big(base)
                && let Some(exponent) = integer_big(exponent)
            {
                if base == BigInt::from(0) && exponent == BigInt::from(0) {
                    return Err(LangError::conflict(
                        span.line,
                        span.col,
                        "math.Pow: invalid operation",
                    ));
                }
                if let Ok(exponent) = u32::try_from(&exponent) {
                    let estimated_digits = base
                        .bits()
                        .saturating_sub(1)
                        .saturating_mul(u64::from(exponent))
                        .saturating_mul(30_103)
                        / 100_000;
                    if estimated_digits > 100_000 {
                        return Err(LangError::conflict(
                            span.line,
                            span.col,
                            "math.Pow: exponent out of range",
                        ));
                    }
                    return Ok(value_from_bigint(base.pow(exponent)));
                }
            }
            Ok(math_number(
                math_arg(arguments, 0, span)?.powf(math_arg(arguments, 1, span)?),
            ))
        }
        Builtin::MathExp => Ok(math_number(math_arg(arguments, 0, span)?.exp())),
        Builtin::MathLog => Ok(math_number(math_arg(arguments, 0, span)?.ln())),
        _ => unreachable!("math builtin"),
    }
}

fn time_arg(arguments: &[Value], index: usize, span: Span) -> Result<&str, LangError> {
    match arguments.get(index) {
        Some(Value::String(value)) => Ok(value),
        _ => Err(LangError::conflict(
            span.line,
            span.col,
            "time function requires string arguments",
        )),
    }
}

fn leading_fraction(input: &str) -> (u64, f64) {
    let mut value = 0u64;
    let mut scale = 1f64;
    let mut overflow = false;
    for byte in input.bytes() {
        if !byte.is_ascii_digit() {
            break;
        }
        if overflow {
            continue;
        }
        if value > ((1u64 << 63) - 1) / 10 {
            overflow = true;
            continue;
        }
        let next = value * 10 + u64::from(byte - b'0');
        if next > 1u64 << 63 {
            overflow = true;
            continue;
        }
        value = next;
        scale *= 10.0;
    }
    (value, scale)
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn parse_duration_nanos(text: &str) -> Option<i64> {
    if text == "0" || text == "+0" || text == "-0" {
        return Some(0);
    }
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => (-1i128, rest),
        None => (1i128, text.strip_prefix('+').unwrap_or(text)),
    };
    if rest.is_empty() {
        return None;
    }
    let mut total: i128 = 0;
    let mut rest = rest;
    while !rest.is_empty() {
        let integer_len = rest.chars().take_while(char::is_ascii_digit).count();
        let integer: i128 = if integer_len == 0 {
            0
        } else {
            rest[..integer_len].parse().ok()?
        };
        rest = &rest[integer_len..];
        let mut fraction = 0u64;
        let mut fraction_scale = 1f64;
        if let Some(after) = rest.strip_prefix('.') {
            let digit_len = after.chars().take_while(char::is_ascii_digit).count();
            if integer_len == 0 && digit_len == 0 {
                return None;
            }
            (fraction, fraction_scale) = leading_fraction(after);
            rest = &after[digit_len..];
        } else if integer_len == 0 {
            return None;
        }
        let (unit, scale) = [
            ("ns", 1i128),
            ("us", 1_000),
            ("µs", 1_000),
            ("μs", 1_000),
            ("ms", 1_000_000),
            ("s", 1_000_000_000),
            ("m", 60_000_000_000),
            ("h", 3_600_000_000_000),
        ]
        .iter()
        .find(|(unit, _)| rest.starts_with(unit))
        .copied()?;
        rest = &rest[unit.len()..];
        let mut value = integer.checked_mul(scale)?;
        if fraction > 0 {
            value =
                value.checked_add((fraction as f64 * (scale as f64 / fraction_scale)) as i128)?;
        }
        total = total.checked_add(value)?;
    }
    total = total.checked_mul(sign)?;
    i64::try_from(total).ok()
}

fn rfc3339_nano(datetime: &chrono::DateTime<chrono::Utc>) -> String {
    crate::gotime::to_rfc3339_nano(datetime)
}

fn parse_layout(
    layout: &str,
    text: &str,
    span: Span,
) -> Result<chrono::DateTime<chrono::Utc>, LangError> {
    crate::gotime::parse(layout, text)
        .map(|value| value.utc())
        .ok_or_else(|| LangError::conflict(span.line, span.col, format!("invalid time `{text}`")))
}

fn time_value(text: &str, span: Span) -> Result<crate::gotime::ParsedTime, LangError> {
    crate::gotime::parse_rfc3339_nano(text)
        .ok_or_else(|| LangError::conflict(span.line, span.col, format!("invalid time `{text}`")))
}

fn apply_time_builtin(
    builtin: Builtin,
    arguments: &[Value],
    span: Span,
) -> Result<Value, LangError> {
    use chrono::DateTime;

    match builtin {
        Builtin::TimeUnix => {
            let (Some(Value::Int(seconds)), Some(Value::Int(nanos))) =
                (arguments.first(), arguments.get(1))
            else {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "time.Unix requires seconds and nanoseconds",
                ));
            };
            let total = i128::from(*seconds) * 1_000_000_000 + i128::from(*nanos);
            let seconds = i64::try_from(total.div_euclid(1_000_000_000))
                .map_err(|_| LangError::conflict(span.line, span.col, "invalid timestamp"))?;
            let nanos = u32::try_from(total.rem_euclid(1_000_000_000))
                .map_err(|_| LangError::conflict(span.line, span.col, "invalid timestamp"))?;
            let datetime = DateTime::from_timestamp(seconds, nanos)
                .ok_or_else(|| LangError::conflict(span.line, span.col, "invalid timestamp"))?;
            Ok(Value::String(rfc3339_nano(&datetime)))
        }
        Builtin::TimeParse => {
            let layout = time_arg(arguments, 0, span)?;
            let text = time_arg(arguments, 1, span)?;
            let datetime = parse_layout(layout, text, span)?;
            Ok(Value::String(rfc3339_nano(&datetime)))
        }
        Builtin::TimeFormat => {
            let time = time_arg(arguments, 0, span)?;
            let layout = time_arg(arguments, 1, span)?;
            parse_layout(layout, time, span)?;
            Ok(Value::Bool(true))
        }
        Builtin::TimeParseDuration => {
            let text = time_arg(arguments, 0, span)?;
            let nanos = parse_duration_nanos(text)
                .ok_or_else(|| LangError::conflict(span.line, span.col, "invalid duration"))?;
            Ok(Value::Int(nanos))
        }
        Builtin::TimeDuration => {
            let text = time_arg(arguments, 0, span)?;
            parse_duration_nanos(text)
                .ok_or_else(|| LangError::conflict(span.line, span.col, "invalid duration"))?;
            Ok(Value::Bool(true))
        }
        Builtin::TimeTime => {
            let text = time_arg(arguments, 0, span)?;
            time_value(text, span)?;
            Ok(Value::Bool(true))
        }
        Builtin::TimeFormatString => {
            let layout = time_arg(arguments, 0, span)?;
            let text = time_arg(arguments, 1, span)?;
            let parsed = time_value(text, span)?;
            Ok(Value::String(crate::gotime::format(layout, &parsed)))
        }
        Builtin::TimeToUnix => {
            let text = time_arg(arguments, 0, span)?;
            let datetime = time_value(text, span)?.utc();
            Ok(Value::Int(datetime.timestamp()))
        }
        Builtin::TimeToUnixNano => {
            let text = time_arg(arguments, 0, span)?;
            let datetime = time_value(text, span)?.utc();
            let nanos = i128::from(datetime.timestamp()) * 1_000_000_000
                + i128::from(datetime.timestamp_subsec_nanos());
            i64::try_from(nanos)
                .map(Value::Int)
                .map_err(|_| LangError::conflict(span.line, span.col, "invalid timestamp"))
        }
        Builtin::TimeSplit => {
            use chrono::{Datelike, Timelike};
            let text = time_arg(arguments, 0, span)?;
            let local = time_value(text, span)?.local;
            let mut parts = BTreeMap::new();
            parts.insert("year".to_owned(), Value::Int(i64::from(local.year())));
            parts.insert("month".to_owned(), Value::Int(i64::from(local.month())));
            parts.insert("day".to_owned(), Value::Int(i64::from(local.day())));
            parts.insert("hour".to_owned(), Value::Int(i64::from(local.hour())));
            parts.insert("minute".to_owned(), Value::Int(i64::from(local.minute())));
            parts.insert("second".to_owned(), Value::Int(i64::from(local.second())));
            parts.insert(
                "nanosecond".to_owned(),
                Value::Int(i64::from(local.nanosecond())),
            );
            Ok(Value::Struct(parts))
        }
        Builtin::TimeFormatDuration => {
            let nanos = match arguments.first() {
                Some(Value::Int(nanos)) => *nanos,
                Some(Value::BigInt(nanos)) => {
                    let limit = BigInt::from(u64::MAX);
                    if let Ok(value) = i64::try_from(nanos) {
                        value
                    } else if nanos > &BigInt::from(i64::MAX) {
                        if nanos > &limit {
                            return Err(LangError::conflict(
                                span.line,
                                span.col,
                                "time.FormatDuration argument overflows int64",
                            ));
                        }
                        i64::MAX
                    } else if nanos < &BigInt::from(i64::MIN) {
                        if nanos < &-limit {
                            return Err(LangError::conflict(
                                span.line,
                                span.col,
                                "time.FormatDuration argument overflows int64",
                            ));
                        }
                        i64::MIN
                    } else {
                        i64::MIN
                    }
                }
                _ => {
                    return Err(LangError::conflict(
                        span.line,
                        span.col,
                        "time.FormatDuration requires an int argument",
                    ));
                }
            };
            Ok(Value::String(crate::gotime::format_duration(nanos)))
        }
        _ => unreachable!("time builtin"),
    }
}

/// A value that still awaits runtime injection (or another incomplete
/// computation) rather than a concrete value.
fn value_is_pending(value: &Value) -> bool {
    match value {
        Value::Type(_)
        | Value::Bottom
        | Value::Bounds(_)
        | Value::Recursive(_)
        | Value::Deferred(_, _) => true,
        Value::Builtin(_)
        | Value::Null
        | Value::Bool(_)
        | Value::Int(_)
        | Value::BigInt(_)
        | Value::Float(_)
        | Value::String(_)
        | Value::Bytes(_)
        | Value::OpenList(None)
        | Value::Conflict(_) => false,
        Value::List(items) | Value::OpenListPrefix(items, _) => items.iter().any(value_is_pending),
        Value::OpenList(Some(element)) => value_is_pending(element),
        Value::Struct(fields) | Value::Definition { fields, .. } => {
            fields.values().any(value_is_pending)
        }
        Value::Disjunction(alternatives, _) => alternatives.iter().any(value_is_pending),
    }
}

/// A typed placeholder for a builtin over pending task outputs. The enclosing
/// field is re-evaluated once the producer injects its outputs.
fn deferred_builtin_value(builtin: Builtin) -> Option<Value> {
    Some(match builtin {
        Builtin::Len => Value::Type(TypeKind::Int),
        Builtin::StringsSplit => Value::OpenList(Some(Box::new(Value::Type(TypeKind::String)))),
        Builtin::StringsHasPrefix | Builtin::StringsHasSuffix => Value::Type(TypeKind::Bool),
        Builtin::StringsToUpper
        | Builtin::StringsToLower
        | Builtin::StringsTrimSpace
        | Builtin::StringsJoin => Value::Type(TypeKind::String),
        Builtin::ListRange => Value::OpenList(Some(Box::new(Value::Type(TypeKind::Int)))),
        Builtin::RegexpMatch | Builtin::RegexpValid => Value::Type(TypeKind::Bool),
        Builtin::RegexpFind | Builtin::RegexpReplaceAll | Builtin::RegexpQuoteMeta => {
            Value::Type(TypeKind::String)
        }
        Builtin::RegexpFindAll => Value::OpenList(Some(Box::new(Value::Type(TypeKind::String)))),
        Builtin::MathAbs
        | Builtin::MathFloor
        | Builtin::MathCeil
        | Builtin::MathRound
        | Builtin::MathTrunc
        | Builtin::MathSqrt
        | Builtin::MathPow
        | Builtin::MathExp
        | Builtin::MathLog => Value::Type(TypeKind::Number),
        _ => return None,
    })
}

/// A typed placeholder for a binary operator over pending task outputs.
fn deferred_binary_value(op: &BinOp, left: &Value, right: &Value) -> Value {
    let stringish =
        |value: &Value| matches!(value, Value::String(_) | Value::Type(TypeKind::String));
    let bytesish = |value: &Value| matches!(value, Value::Bytes(_) | Value::Type(TypeKind::Bytes));
    match op {
        BinOp::Add if stringish(left) || stringish(right) => Value::Type(TypeKind::String),
        BinOp::Add if bytesish(left) || bytesish(right) => Value::Type(TypeKind::Bytes),
        BinOp::Eq
        | BinOp::Ne
        | BinOp::Lt
        | BinOp::Le
        | BinOp::Gt
        | BinOp::Ge
        | BinOp::And
        | BinOp::Or => Value::Type(TypeKind::Bool),
        _ => Value::Type(TypeKind::Number),
    }
}

fn apply_builtin(callee: &Value, arguments: &[Value], span: Span) -> Result<Value, LangError> {
    let Value::Builtin(builtin) = callee else {
        return Err(LangError::conflict(
            span.line,
            span.col,
            "attempted to call a non-function value",
        ));
    };
    match builtin {
        Builtin::RegexpMatch
        | Builtin::RegexpFind
        | Builtin::RegexpFindAll
        | Builtin::RegexpReplaceAll
        | Builtin::RegexpQuoteMeta
        | Builtin::RegexpValid => apply_regexp_builtin(*builtin, arguments, span),
        Builtin::MathAbs
        | Builtin::MathFloor
        | Builtin::MathCeil
        | Builtin::MathRound
        | Builtin::MathTrunc
        | Builtin::MathSqrt
        | Builtin::MathPow
        | Builtin::MathExp
        | Builtin::MathLog => apply_math_builtin(*builtin, arguments, span),
        Builtin::TimeUnix
        | Builtin::TimeParse
        | Builtin::TimeFormat
        | Builtin::TimeParseDuration
        | Builtin::TimeDuration
        | Builtin::TimeTime
        | Builtin::TimeFormatString
        | Builtin::TimeToUnix
        | Builtin::TimeToUnixNano
        | Builtin::TimeSplit
        | Builtin::TimeFormatDuration => apply_time_builtin(*builtin, arguments, span),
        Builtin::StringsToUpper | Builtin::StringsToLower | Builtin::StringsTrimSpace => {
            if arguments.len() != 1 {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "string function requires exactly one argument",
                ));
            }
            let Some(Value::String(text)) = arguments.first() else {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "string function requires a string argument",
                ));
            };
            let result = match builtin {
                Builtin::StringsToUpper => go_case(text, true),
                Builtin::StringsToLower => go_case(text, false),
                _ => text.trim().to_owned(),
            };
            Ok(Value::String(result))
        }
        Builtin::StringsJoin => {
            if arguments.len() != 2 {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "strings.Join requires exactly two arguments",
                ));
            }
            let (Some(Value::List(values)), Some(Value::String(separator))) =
                (arguments.first(), arguments.get(1))
            else {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "strings.Join requires a list and a separator",
                ));
            };
            let mut pieces = Vec::with_capacity(values.len());
            for value in values {
                let Value::String(text) = value else {
                    return Err(LangError::conflict(
                        span.line,
                        span.col,
                        "strings.Join requires a list of strings",
                    ));
                };
                pieces.push(text.clone());
            }
            Ok(Value::String(pieces.join(separator)))
        }
        Builtin::StringsSplit => {
            if arguments.len() != 2 {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "strings.Split requires exactly two arguments",
                ));
            }
            let (Some(Value::String(text)), Some(Value::String(separator))) =
                (arguments.first(), arguments.get(1))
            else {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "strings.Split requires two strings",
                ));
            };
            if separator.is_empty() {
                return Ok(Value::List(
                    text.chars()
                        .map(|character| Value::String(character.to_string()))
                        .collect(),
                ));
            }
            Ok(Value::List(
                text.split(separator)
                    .map(|part| Value::String(part.to_owned()))
                    .collect(),
            ))
        }
        Builtin::StringsHasPrefix | Builtin::StringsHasSuffix => {
            if arguments.len() != 2 {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "string predicate requires exactly two arguments",
                ));
            }
            let (Some(Value::String(text)), Some(Value::String(needle))) =
                (arguments.first(), arguments.get(1))
            else {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "string predicate requires two strings",
                ));
            };
            Ok(Value::Bool(match builtin {
                Builtin::StringsHasPrefix => text.starts_with(needle),
                _ => text.ends_with(needle),
            }))
        }
        Builtin::ListRange => {
            if arguments.len() != 3 {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "list.Range requires exactly three arguments",
                ));
            }
            let (Some(start), Some(end), Some(step)) = (
                arguments.first().and_then(numeric_value_decimal),
                arguments.get(1).and_then(numeric_value_decimal),
                arguments.get(2).and_then(numeric_value_decimal),
            ) else {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "list.Range requires three numbers",
                ));
            };
            if step == 0 {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "list.Range step is zero",
                ));
            }
            let positive = step > 0;
            if (positive && start > end) || (!positive && start < end) {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "list.Range step has the wrong direction",
                ));
            }
            let mut out = Vec::new();
            let mut current = start;
            let mut iterations = 0usize;
            while (positive && current < end) || (!positive && current > end) {
                if iterations > 100_000 {
                    return Err(LangError::conflict(
                        span.line,
                        span.col,
                        "list.Range is too large",
                    ));
                }
                iterations += 1;
                let normalized = current.normalized();
                let text = normalized.to_string();
                if let Ok(value) = text.parse::<i64>() {
                    out.push(Value::Int(value));
                } else if let Ok(value) = text.parse::<BigInt>() {
                    out.push(Value::BigInt(value));
                } else {
                    out.push(Value::Float(current.clone()));
                }
                current += &step;
            }
            Ok(Value::List(out))
        }
        Builtin::ListConcat => {
            if arguments.len() != 1 {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "list.Concat requires exactly one list of lists",
                ));
            }
            let Some(Value::List(outer) | Value::OpenListPrefix(outer, _)) = arguments.first()
            else {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "list.Concat requires a list of lists",
                ));
            };
            let mut values = Vec::new();
            for inner in outer {
                if matches!(inner, Value::OpenList(_)) {
                    continue;
                }
                let (Value::List(inner) | Value::OpenListPrefix(inner, _)) = inner else {
                    return Err(LangError::conflict(
                        span.line,
                        span.col,
                        "list.Concat requires a list of lists",
                    ));
                };
                values.extend(inner.iter().cloned());
            }
            Ok(Value::List(values))
        }
        Builtin::Len => {
            if arguments.len() != 1 {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "len requires exactly one argument",
                ));
            }
            match arguments.first() {
                Some(Value::List(values) | Value::OpenListPrefix(values, _)) => {
                    Ok(Value::Int(i64::try_from(values.len()).unwrap_or(i64::MAX)))
                }
                Some(Value::OpenList(_)) => Ok(Value::Int(0)),
                Some(Value::Struct(values) | Value::Definition { fields: values, .. }) => {
                    let visible = values.keys().filter(|key| !is_hidden_key(key)).count();
                    Ok(Value::Int(i64::try_from(visible).unwrap_or(i64::MAX)))
                }
                Some(Value::String(value)) => {
                    Ok(Value::Int(i64::try_from(value.len()).unwrap_or(i64::MAX)))
                }
                Some(Value::Bytes(value)) => {
                    Ok(Value::Int(i64::try_from(value.len()).unwrap_or(i64::MAX)))
                }
                _ => Err(LangError::conflict(
                    span.line,
                    span.col,
                    "len requires a list, struct or string",
                )),
            }
        }
    }
}

fn binary(op: &BinOp, left: &Value, right: &Value, span: Span) -> Result<Value, LangError> {
    match op {
        BinOp::Add => {
            if let (Some(a), Some(b)) = (integer_big(left), integer_big(right)) {
                return Ok(value_from_bigint(a + b));
            }
            match (left, right) {
                (Value::Int(a), Value::Int(b)) => a
                    .checked_add(*b)
                    .map(Value::Int)
                    .ok_or_else(|| overflow(span)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(BigDecimal::from(*a) + b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + BigDecimal::from(*b))),
                (Value::BigInt(a), Value::Float(b)) => {
                    Ok(Value::Float(BigDecimal::from(a.clone()) + b))
                }
                (Value::Float(a), Value::BigInt(b)) => {
                    Ok(Value::Float(a + BigDecimal::from(b.clone())))
                }
                (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{a}{b}"))),
                (Value::Bytes(a), Value::Bytes(b)) => {
                    let mut out = a.clone();
                    out.extend_from_slice(b);
                    Ok(Value::Bytes(out))
                }
                (Value::OpenList(element), Value::List(values))
                | (Value::List(values), Value::OpenList(element)) => {
                    if let Some(element) = element {
                        for value in values {
                            let _ = unify_values(element, value, span)?;
                        }
                    }
                    Ok(Value::List(values.clone()))
                }
                (Value::OpenList(left), Value::OpenList(right)) => match (left, right) {
                    (None, None) => Ok(Value::OpenList(None)),
                    (Some(left), Some(right)) => Ok(Value::OpenList(Some(Box::new(unify_values(
                        left, right, span,
                    )?)))),
                    (Some(element), None) | (None, Some(element)) => {
                        Ok(Value::OpenList(Some(element.clone())))
                    }
                },
                (Value::List(a), Value::List(b)) => {
                    let mut out = a.clone();
                    out.extend(b.iter().cloned());
                    Ok(Value::List(out))
                }
                _ => type_error("add", left, right, span),
            }
        }
        BinOp::Sub => {
            if let (Some(a), Some(b)) = (integer_big(left), integer_big(right)) {
                return Ok(value_from_bigint(a - b));
            }
            numeric(op, left, right, span, |a, b| a - b, |a, b| a - b)
        }
        BinOp::Mul => {
            if let (Some(a), Some(b)) = (integer_big(left), integer_big(right)) {
                return Ok(value_from_bigint(a * b));
            }
            match (left, right) {
                (Value::String(text), Value::Int(count))
                | (Value::Int(count), Value::String(text)) => repeat_string(text, *count, span),
                (Value::Bytes(bytes), Value::Int(count))
                | (Value::Int(count), Value::Bytes(bytes)) => repeat_bytes(bytes, *count, span),
                _ => numeric(op, left, right, span, |a, b| a * b, |a, b| a * b),
            }
        }
        BinOp::Div => {
            let zero = matches!(right, Value::Int(0))
                || matches!(right, Value::BigInt(value) if value == &BigInt::from(0))
                || matches!(right, Value::Float(value) if value == &BigDecimal::from(0));
            if zero {
                return Err(LangError::conflict(span.line, span.col, "division by zero"));
            }
            match (numeric_value_decimal(left), numeric_value_decimal(right)) {
                (Some(left), Some(right)) => Ok(Value::Float(float_division(&left, &right))),
                _ => type_error("divide", left, right, span),
            }
        }
        BinOp::Mod => match (left, right) {
            (Value::Int(_), Value::Int(0)) => {
                Err(LangError::conflict(span.line, span.col, "modulo by zero"))
            }
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a % b)),
            _ => match (integer_big(left), integer_big(right)) {
                (Some(a), Some(b)) => Ok(value_from_bigint(a % b)),
                _ => type_error("mod", left, right, span),
            },
        },
        BinOp::Eq | BinOp::Ne => {
            if !left.is_fully_concrete() || !right.is_fully_concrete() {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "non-concrete value in operand to == or !=",
                ));
            }
            let equal = values_equal(left, right);
            Ok(Value::Bool(if matches!(op, BinOp::Eq) {
                equal
            } else {
                !equal
            }))
        }
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => compare(op, left, right, span),
        BinOp::And | BinOp::Or => Ok(Value::Bool(match op {
            BinOp::And => left.truthy() && right.truthy(),
            _ => left.truthy() || right.truthy(),
        })),
    }
}

const MAX_REPEAT_BYTES: usize = 64 * 1024 * 1024;

fn repeat_count(count: i64, span: Span) -> Result<usize, LangError> {
    usize::try_from(count).map_err(|_| {
        LangError::conflict(
            span.line,
            span.col,
            "cannot convert negative number to uint64",
        )
    })
}

fn repeat_string(text: &str, count: i64, span: Span) -> Result<Value, LangError> {
    let count = repeat_count(count, span)?;
    let length = text.len().checked_mul(count).ok_or_else(|| {
        LangError::conflict(span.line, span.col, "string repetition is too large")
    })?;
    if length > MAX_REPEAT_BYTES {
        return Err(LangError::conflict(
            span.line,
            span.col,
            "string repetition is too large",
        ));
    }
    Ok(Value::String(text.repeat(count)))
}

fn repeat_bytes(bytes: &[u8], count: i64, span: Span) -> Result<Value, LangError> {
    let count = repeat_count(count, span)?;
    let length = bytes
        .len()
        .checked_mul(count)
        .ok_or_else(|| LangError::conflict(span.line, span.col, "bytes repetition is too large"))?;
    if length > MAX_REPEAT_BYTES {
        return Err(LangError::conflict(
            span.line,
            span.col,
            "bytes repetition is too large",
        ));
    }
    Ok(Value::Bytes(bytes.repeat(count)))
}

/// Largest number of fractional zeros materialized in plain notation. Larger
/// scales use scientific notation so a short input such as `0e-100000000`
/// cannot allocate a multi-megabyte string.
const MAX_MATERIALIZED_SCALE: i64 = 1024;

fn float_text(value: &BigDecimal, scale: i64) -> String {
    if scale < 0 {
        let (coefficient, _) = value.as_bigint_and_exponent();
        let raw = coefficient.to_string();
        let negative = raw.starts_with('-');
        let digits = raw.trim_start_matches('-');
        let exponent = -i128::from(scale) + i128::try_from(digits.len()).unwrap_or(i128::MAX) - 1;
        let mantissa = if digits.len() > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits.to_owned()
        };
        let sign = if negative { "-" } else { "" };
        return format!("{sign}{mantissa}E{exponent:+}");
    }
    if scale > MAX_MATERIALIZED_SCALE {
        let (coefficient, _) = value.as_bigint_and_exponent();
        let raw = coefficient.to_string();
        let negative = raw.starts_with('-');
        let digits = raw.trim_start_matches('-');
        if digits == "0" {
            return format!("0e-{scale}");
        }
        let exponent = i128::try_from(digits.len()).unwrap_or(i128::MAX) - 1 - i128::from(scale);
        let mantissa = if digits.len() > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits.to_owned()
        };
        let sign = if negative { "-" } else { "" };
        return format!("{sign}{mantissa}E{exponent:+}");
    }
    if value == &BigDecimal::from(0) && scale > 0 {
        return format!(
            "0.{}",
            "0".repeat(usize::try_from(scale).unwrap_or_default())
        );
    }
    let mut text = value.to_string();
    if scale != 0 && !text.contains(['.', 'e', 'E']) {
        text.push_str(".0");
    }
    text
}

fn float_arithmetic(value: BigDecimal, desired_scale: i64) -> BigDecimal {
    let (_, scale) = value.as_bigint_and_exponent();
    if scale < desired_scale || (value.to_string() == "0" && scale != desired_scale) {
        value.with_scale(desired_scale)
    } else {
        value
    }
}

fn float_division(left: &BigDecimal, right: &BigDecimal) -> BigDecimal {
    let quotient = left / right;
    let (_, left_scale) = left.as_bigint_and_exponent();
    let (_, right_scale) = right.as_bigint_and_exponent();
    let reduced = quotient.normalized();
    let (coefficient, reduced_scale) = reduced.as_bigint_and_exponent();
    let coefficient_text = coefficient.to_string();
    let old_exponent = if coefficient_text == "0" {
        right_scale - left_scale
    } else {
        let digits =
            i64::try_from(coefficient_text.trim_start_matches('-').len()).unwrap_or(i64::MAX);
        digits - reduced_scale - 34
    };
    if old_exponent < 0 && reduced_scale <= 0 {
        reduced.with_scale(reduced_scale + 1)
    } else {
        reduced
    }
}

fn numeric(
    op: &BinOp,
    left: &Value,
    right: &Value,
    span: Span,
    int_op: impl Fn(i64, i64) -> i64,
    float_op: impl Fn(BigDecimal, BigDecimal) -> BigDecimal,
) -> Result<Value, LangError> {
    let scale = |value: &Value| match value {
        Value::Float(value) => value.as_bigint_and_exponent().1,
        _ => 0,
    };
    let desired_scale = match op {
        BinOp::Mul => scale(left) + scale(right),
        _ => scale(left).max(scale(right)),
    };
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(int_op(*a, *b))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_arithmetic(
            float_op(a.clone(), b.clone()),
            desired_scale,
        ))),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(float_arithmetic(
            float_op(BigDecimal::from(*a), b.clone()),
            desired_scale,
        ))),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(float_arithmetic(
            float_op(a.clone(), BigDecimal::from(*b)),
            desired_scale,
        ))),
        (Value::BigInt(a), Value::BigInt(b)) => Ok(Value::Float(float_arithmetic(
            float_op(BigDecimal::from(a.clone()), BigDecimal::from(b.clone())),
            desired_scale,
        ))),
        (Value::BigInt(a), Value::Int(b)) => Ok(Value::Float(float_arithmetic(
            float_op(BigDecimal::from(a.clone()), BigDecimal::from(*b)),
            desired_scale,
        ))),
        (Value::Int(a), Value::BigInt(b)) => Ok(Value::Float(float_arithmetic(
            float_op(BigDecimal::from(*a), BigDecimal::from(b.clone())),
            desired_scale,
        ))),
        (Value::BigInt(a), Value::Float(b)) => Ok(Value::Float(float_arithmetic(
            float_op(BigDecimal::from(a.clone()), b.clone()),
            desired_scale,
        ))),
        (Value::Float(a), Value::BigInt(b)) => Ok(Value::Float(float_arithmetic(
            float_op(a.clone(), BigDecimal::from(b.clone())),
            desired_scale,
        ))),
        _ => type_error(op_name(op), left, right, span),
    }
}

fn compare(op: &BinOp, left: &Value, right: &Value, span: Span) -> Result<Value, LangError> {
    let ordering = match (left, right) {
        (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
        (Value::BigInt(a), Value::BigInt(b)) => a.partial_cmp(b),
        (Value::Int(a), Value::BigInt(b)) => BigInt::from(*a).partial_cmp(b),
        (Value::BigInt(a), Value::Int(b)) => a.partial_cmp(&BigInt::from(*b)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Int(a), Value::Float(b)) => BigDecimal::from(*a).partial_cmp(b),
        (Value::Float(a), Value::Int(b)) => a.partial_cmp(&BigDecimal::from(*b)),
        (Value::BigInt(a), Value::Float(b)) => BigDecimal::from(a.clone()).partial_cmp(b),
        (Value::Float(a), Value::BigInt(b)) => a.partial_cmp(&BigDecimal::from(b.clone())),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        _ => return type_error("compare", left, right, span),
    };
    let Some(ordering) = ordering else {
        return Ok(Value::Bool(false));
    };
    let result = match op {
        BinOp::Lt => ordering.is_lt(),
        BinOp::Le => ordering.is_le(),
        BinOp::Gt => ordering.is_gt(),
        BinOp::Ge => ordering.is_ge(),
        _ => false,
    };
    Ok(Value::Bool(result))
}

fn op_name(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "add",
        BinOp::Sub => "subtract",
        BinOp::Mul => "multiply",
        BinOp::Div => "divide",
        BinOp::Mod => "modulo",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}

fn params_cycle_error(error: &LangError) -> bool {
    let text = error.to_string();
    text.contains("cyclic reference while evaluating `params.")
        || text.contains("circular reference while evaluating `params.")
}

fn overflow(span: Span) -> LangError {
    LangError::conflict(span.line, span.col, "integer overflow")
}

fn type_error(op: &str, left: &Value, right: &Value, span: Span) -> Result<Value, LangError> {
    Err(LangError::conflict(
        span.line,
        span.col,
        format!("cannot {op} {} and {}", left.type_name(), right.type_name()),
    ))
}

thread_local! {
    static RECURSIVE_PAIRS: RefCell<Vec<(*const RecursiveSlot, *const RecursiveSlot)>> =
        const { RefCell::new(Vec::new()) };
}

fn combine_constraints(
    left: Option<&Value>,
    right: Option<&Value>,
    span: Span,
) -> Result<Option<Value>, LangError> {
    match (left, right) {
        (Some(left), Some(right)) => Ok(Some(unify_values(left, right, span)?)),
        (Some(value), None) | (None, Some(value)) => Ok(Some(value.clone())),
        (None, None) => Ok(None),
    }
}

fn unify_one_recursive(
    slot: &Rc<RecursiveSlot>,
    constraint: Option<&Value>,
    other: &Value,
    span: Span,
) -> Result<Value, LangError> {
    let combined = match constraint {
        Some(constraint) => unify_values(constraint, other, span)?,
        None => other.clone(),
    };
    let resolved = slot.value.borrow().clone();
    match resolved {
        None => Ok(Value::Deferred(slot.clone(), Box::new(combined))),
        Some(definition) => unify_values(&definition, &combined, span),
    }
}

fn unify_recursive_pair(
    left: &Rc<RecursiveSlot>,
    left_constraint: Option<&Value>,
    right: &Rc<RecursiveSlot>,
    right_constraint: Option<&Value>,
    span: Span,
) -> Result<Value, LangError> {
    if Rc::ptr_eq(left, right) {
        let constraint = combine_constraints(left_constraint, right_constraint, span)?;
        return match (constraint, left.value.borrow().clone()) {
            (Some(constraint), Some(definition)) => unify_values(&definition, &constraint, span),
            (Some(constraint), None) => Ok(Value::Deferred(left.clone(), Box::new(constraint))),
            (None, Some(_)) => Ok(Value::Recursive(left.clone())),
            (None, None) => Ok(Value::Recursive(left.clone())),
        };
    }
    let (first, second) = if Rc::as_ptr(left) < Rc::as_ptr(right) {
        (Rc::as_ptr(left), Rc::as_ptr(right))
    } else {
        (Rc::as_ptr(right), Rc::as_ptr(left))
    };
    let seen = RECURSIVE_PAIRS.with(|pairs| {
        let pairs = pairs.borrow();
        pairs.iter().any(|(a, b)| *a == first && *b == second)
    });
    if seen {
        return Ok(Value::Bottom);
    }
    RECURSIVE_PAIRS.with(|pairs| pairs.borrow_mut().push((first, second)));
    let result = (|| {
        let left_value = left.value.borrow().clone();
        let right_value = right.value.borrow().clone();
        match (left_value, right_value) {
            (Some(left_def), Some(right_def)) => {
                let left_def = match left_constraint {
                    Some(constraint) => unify_values(&left_def, constraint, span)?,
                    None => left_def,
                };
                let right_def = match right_constraint {
                    Some(constraint) => unify_values(&right_def, constraint, span)?,
                    None => right_def,
                };
                unify_values(&left_def, &right_def, span)
            }
            (Some(left_def), None) => {
                let constraint = combine_constraints(left_constraint, right_constraint, span)?
                    .map_or_else(
                        || Ok::<Value, LangError>(left_def.clone()),
                        |value| unify_values(&left_def, &value, span),
                    )?;
                Ok(Value::Deferred(right.clone(), Box::new(constraint)))
            }
            (None, Some(right_def)) => {
                let constraint = combine_constraints(left_constraint, right_constraint, span)?
                    .map_or_else(
                        || Ok::<Value, LangError>(right_def.clone()),
                        |value| unify_values(&right_def, &value, span),
                    )?;
                Ok(Value::Deferred(left.clone(), Box::new(constraint)))
            }
            (None, None) => {
                let constraint = combine_constraints(left_constraint, right_constraint, span)?;
                match constraint {
                    Some(constraint) => Ok(Value::Deferred(left.clone(), Box::new(constraint))),
                    None => Ok(Value::Deferred(left.clone(), Box::new(Value::Bottom))),
                }
            }
        }
    })();
    RECURSIVE_PAIRS.with(|pairs| {
        pairs.borrow_mut().pop();
    });
    result
}

fn unify_values(left: &Value, right: &Value, span: Span) -> Result<Value, LangError> {
    stacker::maybe_grow(4 * 1024 * 1024, 64 * 1024 * 1024, || {
        unify_values_impl(left, right, span)
    })
}

fn unify_values_impl(left: &Value, right: &Value, span: Span) -> Result<Value, LangError> {
    match (left, right) {
        (Value::Recursive(left), Value::Recursive(right)) => {
            unify_recursive_pair(left, None, right, None, span)
        }
        (Value::Deferred(left, left_constraint), Value::Recursive(right)) => {
            unify_recursive_pair(left, Some(&**left_constraint), right, None, span)
        }
        (Value::Recursive(left), Value::Deferred(right, right_constraint)) => {
            unify_recursive_pair(left, None, right, Some(&**right_constraint), span)
        }
        (Value::Deferred(left, left_constraint), Value::Deferred(right, right_constraint)) => {
            unify_recursive_pair(
                left,
                Some(&**left_constraint),
                right,
                Some(&**right_constraint),
                span,
            )
        }
        (Value::Recursive(slot), other) | (other, Value::Recursive(slot)) => {
            unify_one_recursive(slot, None, other, span)
        }
        (Value::Deferred(slot, constraint), other) | (other, Value::Deferred(slot, constraint)) => {
            unify_one_recursive(slot, Some(constraint), other, span)
        }
        (Value::Conflict(message), _) | (_, Value::Conflict(message)) => {
            Err(LangError::conflict(span.line, span.col, message.clone()))
        }
        (Value::Bottom, value) | (value, Value::Bottom) => Ok(value.clone()),
        (Value::Bounds(a), Value::Bounds(b)) => merge_bounds(a, b, span),
        (Value::Disjunction(a, da), Value::Disjunction(b, db)) => {
            unify_disjunctions(a, da, b, db, span)
        }
        (Value::Disjunction(a, da), other) | (other, Value::Disjunction(a, da)) => {
            unify_disjunction_with(a, da, other, span)
        }
        (Value::Bounds(bounds), other) | (other, Value::Bounds(bounds)) => {
            apply_bounds(bounds, other, span)
        }
        (Value::Type(a), Value::Type(b)) => unify_types(*a, *b, span),
        (Value::Type(kind), value) | (value, Value::Type(kind)) => {
            if type_matches(*kind, value) {
                Ok(value.clone())
            } else {
                Err(LangError::conflict(
                    span.line,
                    span.col,
                    format!(
                        "{} does not satisfy {}",
                        value.type_name(),
                        kind_name(*kind)
                    ),
                ))
            }
        }
        (
            Value::Definition {
                fields: a,
                optional: optional_a,
                definitions: definitions_a,
                patterns,
                closed,
                open,
                admissions,
            },
            Value::Struct(b),
        )
        | (
            Value::Struct(b),
            Value::Definition {
                fields: a,
                optional: optional_a,
                definitions: definitions_a,
                patterns,
                closed,
                open,
                admissions,
            },
        ) => {
            let mut fields = a.clone();
            let mut optional = optional_a.clone();
            for (key, value) in b {
                if let Some(existing) = fields.get(key).cloned() {
                    fields.insert(key.clone(), unify_values(&existing, value, span)?);
                } else if let Some(constraint) = optional.remove(key) {
                    if !admitted(admissions, key, span)? {
                        return Err(closed_error(span, key));
                    }
                    fields.insert(key.clone(), unify_values(&constraint, value, span)?);
                } else if admitted(admissions, key, span)? {
                    let value =
                        pattern_constraint_closed(patterns, display_field_name(key), value, span)?
                            .unwrap_or_else(|| value.clone());
                    fields.insert(key.clone(), value);
                } else {
                    return Err(closed_error(span, key));
                }
            }
            apply_patterns(patterns, &mut fields, span)?;
            Ok(Value::Definition {
                fields,
                optional,
                definitions: definitions_a.clone(),
                patterns: patterns.clone(),
                closed: *closed,
                open: *open,
                admissions: admissions.clone(),
            })
        }
        (
            Value::Definition {
                fields: a,
                optional: optional_a,
                definitions: definitions_a,
                patterns: patterns_a,
                closed: closed_a,
                open: open_a,
                admissions: admissions_a,
            },
            Value::Definition {
                fields: b,
                optional: optional_b,
                definitions: definitions_b,
                patterns: patterns_b,
                closed: closed_b,
                open: open_b,
                admissions: admissions_b,
            },
        ) => {
            let optional_a_keys: BTreeSet<String> = optional_a.keys().cloned().collect();
            let optional_b_keys: BTreeSet<String> = optional_b.keys().cloned().collect();
            let mut fields = a.clone();
            let mut optional = optional_a.clone();
            for (key, value) in b {
                if let Some(existing) = fields.get(key).cloned() {
                    fields.insert(key.clone(), unify_values(&existing, value, span)?);
                } else if let Some(constraint) = optional.remove(key) {
                    if !admitted(admissions_a, key, span)? {
                        return Err(closed_error(span, key));
                    }
                    fields.insert(key.clone(), unify_values(&constraint, value, span)?);
                } else if admitted(admissions_a, key, span)? {
                    let value = pattern_constraint_closed(
                        patterns_a,
                        display_field_name(key),
                        value,
                        span,
                    )?
                    .unwrap_or_else(|| value.clone());
                    fields.insert(key.clone(), value);
                } else {
                    return Err(closed_error(span, key));
                }
            }
            for (key, value) in a {
                if !b.contains_key(key)
                    && !optional_b.contains_key(key)
                    && !admitted(admissions_b, key, span)?
                {
                    return Err(closed_error(span, key));
                }
                if !b.contains_key(key)
                    && !optional_b.contains_key(key)
                    && let Some(constrained) =
                        pattern_constraint_closed(patterns_b, display_field_name(key), value, span)?
                {
                    fields.insert(key.clone(), constrained);
                }
            }
            for (key, value) in optional_b {
                if let Some(existing) = fields.get(key).cloned() {
                    fields.insert(key.clone(), unify_values(&existing, value, span)?);
                } else if let Some(existing) = optional.get(key).cloned() {
                    let merged = match unify_values(&existing, value, span) {
                        Ok(value) => value,
                        Err(err) => Value::Conflict(err.to_string()),
                    };
                    optional.insert(key.clone(), merged);
                } else {
                    optional.insert(key.clone(), value.clone());
                }
            }
            for (key, value) in optional.iter_mut() {
                let mut current = value.clone();
                let mut conflict = None;
                if !optional_a_keys.contains(key) {
                    match pattern_constraint_closed(
                        patterns_a,
                        display_field_name(key),
                        &current,
                        span,
                    ) {
                        Ok(Some(constrained)) => current = constrained,
                        Ok(None) => {}
                        Err(err) if err.to_string().contains("invalid pattern regex") => {
                            return Err(err);
                        }
                        Err(err) => conflict = Some(err),
                    }
                }
                if conflict.is_none() && !optional_b_keys.contains(key) {
                    match pattern_constraint_closed(
                        patterns_b,
                        display_field_name(key),
                        &current,
                        span,
                    ) {
                        Ok(Some(constrained)) => current = constrained,
                        Ok(None) => {}
                        Err(err) if err.to_string().contains("invalid pattern regex") => {
                            return Err(err);
                        }
                        Err(err) => conflict = Some(err),
                    }
                }
                *value = conflict.map_or(current, |err| Value::Conflict(err.to_string()));
            }
            let mut definitions = definitions_a.clone();
            for (key, value) in definitions_b {
                if let Some(existing) = definitions.get(key).cloned() {
                    definitions.insert(key.clone(), unify_values(&existing, value, span)?);
                } else {
                    definitions.insert(key.clone(), value.clone());
                }
            }
            let mut patterns = patterns_a.clone();
            patterns.extend(patterns_b.clone());
            apply_patterns(&patterns, &mut fields, span)?;
            let mut admissions = admissions_a.clone();
            admissions.extend(admissions_b.clone());
            let closed = *closed_a || *closed_b;
            let open = !closed && (*open_a || *open_b);
            Ok(Value::Definition {
                fields,
                optional,
                definitions,
                patterns,
                closed,
                open,
                admissions,
            })
        }
        (Value::Null, Value::Null) => Ok(Value::Null),
        (Value::Bool(a), Value::Bool(b)) => {
            if a == b {
                Ok(Value::Bool(*a))
            } else {
                Err(conflict(span))
            }
        }
        (Value::Int(a), Value::Int(b)) => {
            if a == b {
                Ok(Value::Int(*a))
            } else {
                Err(conflict(span))
            }
        }
        (Value::BigInt(a), Value::BigInt(b)) => {
            if a == b {
                Ok(Value::BigInt(a.clone()))
            } else {
                Err(conflict(span))
            }
        }
        (Value::Int(a), Value::BigInt(b)) | (Value::BigInt(b), Value::Int(a)) => {
            if BigInt::from(*a) == *b {
                Ok(Value::BigInt(b.clone()))
            } else {
                Err(conflict(span))
            }
        }
        (Value::Float(a), Value::Float(b)) => {
            if a == b {
                Ok(Value::Float(a.clone()))
            } else {
                Err(conflict(span))
            }
        }
        (Value::String(a), Value::String(b)) => {
            if a == b {
                Ok(Value::String(a.clone()))
            } else {
                Err(conflict(span))
            }
        }
        (Value::Struct(a), Value::Struct(b)) => {
            let mut out = a.clone();
            for (key, value) in b {
                if let Some(existing) = out.get(key).cloned() {
                    out.insert(key.clone(), unify_values(&existing, value, span)?);
                } else {
                    out.insert(key.clone(), value.clone());
                }
            }
            Ok(Value::Struct(out))
        }
        (Value::OpenListPrefix(prefix, tail), Value::List(values))
        | (Value::List(values), Value::OpenListPrefix(prefix, tail)) => {
            if prefix.len() > values.len() {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "list prefix is longer than the concrete list",
                ));
            }
            let mut out = Vec::with_capacity(values.len());
            for (index, value) in values.iter().enumerate() {
                let expected = prefix.get(index).or(tail.as_deref());
                out.push(match expected {
                    Some(expected) => unify_values(expected, value, span)?,
                    None => value.clone(),
                });
            }
            Ok(Value::List(out))
        }
        (Value::OpenListPrefix(prefix, tail), Value::OpenList(element))
        | (Value::OpenList(element), Value::OpenListPrefix(prefix, tail)) => {
            let mut out = Vec::with_capacity(prefix.len());
            for value in prefix {
                out.push(match element.as_deref() {
                    Some(element) => unify_values(element, value, span)?,
                    None => value.clone(),
                });
            }
            let merged_tail = match (tail.as_deref(), element.as_deref()) {
                (Some(tail), Some(element)) => match unify_values(tail, element, span) {
                    Ok(value) => Some(Box::new(value)),
                    Err(_) => return Ok(Value::List(out)),
                },
                (Some(tail), None) => Some(Box::new(tail.clone())),
                (None, Some(element)) => Some(Box::new(element.clone())),
                (None, None) => None,
            };
            Ok(Value::OpenListPrefix(out, merged_tail))
        }
        (
            Value::OpenListPrefix(left_prefix, left_tail),
            Value::OpenListPrefix(right_prefix, right_tail),
        ) => {
            let length = left_prefix.len().max(right_prefix.len());
            let mut prefix = Vec::with_capacity(length);
            for index in 0..length {
                let left = left_prefix
                    .get(index)
                    .map_or_else(|| left_tail.as_deref(), Some);
                let right = right_prefix
                    .get(index)
                    .map_or_else(|| right_tail.as_deref(), Some);
                match (left, right) {
                    (Some(left), Some(right)) => {
                        prefix.push(unify_values(left, right, span)?);
                    }
                    (Some(value), None) | (None, Some(value)) => {
                        prefix.push(value.clone());
                    }
                    (None, None) => break,
                }
            }
            let tail = match (left_tail, right_tail) {
                (None, None) => None,
                (Some(left), Some(right)) => match unify_values(left, right, span) {
                    Ok(value) => Some(Box::new(value)),
                    Err(_) => return Ok(Value::List(prefix)),
                },
                (Some(tail), None) | (None, Some(tail)) => Some(tail.clone()),
            };
            Ok(Value::OpenListPrefix(prefix, tail))
        }
        (Value::OpenList(element), Value::List(values))
        | (Value::List(values), Value::OpenList(element)) => {
            let mut out = Vec::with_capacity(values.len());
            for value in values {
                out.push(match element.as_deref() {
                    Some(element) => unify_values(element, value, span)?,
                    None => value.clone(),
                });
            }
            Ok(Value::List(out))
        }
        (Value::OpenList(left), Value::OpenList(right)) => match (left, right) {
            (None, None) => Ok(Value::OpenList(None)),
            (Some(left), Some(right)) => match unify_values(left, right, span) {
                Ok(value) => Ok(Value::OpenList(Some(Box::new(value)))),
                Err(_) => Ok(Value::List(Vec::new())),
            },
            (Some(element), None) | (None, Some(element)) => {
                Ok(Value::OpenList(Some(element.clone())))
            }
        },
        (Value::List(a), Value::List(b)) => {
            if a.len() != b.len() {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "cannot unify lists of different lengths",
                ));
            }
            let mut out = Vec::with_capacity(a.len());
            for (left, right) in a.iter().zip(b) {
                out.push(unify_values(left, right, span)?);
            }
            Ok(Value::List(out))
        }
        (a, b) if !numeric_kind_mismatch(a, b) && values_equal(a, b) => Ok(a.clone()),
        (a, b) => Err(LangError::conflict(
            span.line,
            span.col,
            format!(
                "conflicting values: {} and {}",
                a.type_name(),
                b.type_name()
            ),
        )),
    }
}

fn unify_types(left: TypeKind, right: TypeKind, span: Span) -> Result<Value, LangError> {
    let result = match (left, right) {
        (a, b) if a == b => a,
        (TypeKind::Number, TypeKind::Int) | (TypeKind::Int, TypeKind::Number) => TypeKind::Int,
        (TypeKind::Number, TypeKind::Float) | (TypeKind::Float, TypeKind::Number) => {
            TypeKind::Float
        }
        _ => {
            return Err(LangError::conflict(
                span.line,
                span.col,
                "incompatible type constraints",
            ));
        }
    };
    Ok(Value::Type(result))
}

fn type_matches(kind: TypeKind, value: &Value) -> bool {
    match kind {
        TypeKind::String => matches!(value, Value::String(_)),
        TypeKind::Int => matches!(value, Value::Int(_) | Value::BigInt(_)),
        TypeKind::Float => matches!(value, Value::Float(_)),
        TypeKind::Number => matches!(value, Value::Int(_) | Value::BigInt(_) | Value::Float(_)),
        TypeKind::Bool => matches!(value, Value::Bool(_)),
        TypeKind::Bytes => matches!(value, Value::Bytes(_)),
    }
}

fn kind_name(kind: TypeKind) -> &'static str {
    match kind {
        TypeKind::String => "string",
        TypeKind::Int => "int",
        TypeKind::Float => "float",
        TypeKind::Number => "number",
        TypeKind::Bool => "bool",
        TypeKind::Bytes => "bytes",
    }
}

fn eval_with_override(
    expr: &Expr,
    frame: &Frame<'_>,
    override_value: &Value,
    span: Span,
) -> Result<Value, LangError> {
    let declared = eval_expr(expr, frame)?;
    unify_values(&declared, override_value, span)
}

fn collect_disjunction<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::Disjunction(left, right, _) = expr {
        collect_disjunction(left, out);
        collect_disjunction(right, out);
    } else {
        out.push(expr);
    }
}

fn is_bare_type_operand(expr: &Expr) -> bool {
    match strip_expr_parens(expr) {
        Expr::Ident(name, _) => matches!(
            name.as_str(),
            "int" | "string" | "number" | "float" | "bytes" | "bool"
        ),
        Expr::Bound(_, _, _) => true,
        _ => false,
    }
}

fn require_operand<'v>(
    frame: &Frame<'_>,
    value: &'v Value,
    expr: &Expr,
    span: Span,
) -> Result<Option<&'v Value>, LangError> {
    let concrete = value.concretize();
    if concrete.to_json().is_some() {
        return Ok(Some(concrete));
    }
    let hard = is_bare_type_operand(expr);
    if frame.defer_incomplete.get() && !hard {
        return Ok(None);
    }
    if hard {
        return Err(LangError::invalid_operand(
            span.line,
            span.col,
            format!("invalid operand `{}`", concrete.type_name()),
        ));
    }
    Err(LangError::incomplete_value(
        span.line,
        span.col,
        "non-concrete value in operand",
    ))
}

fn require_bool(frame: &Frame<'_>, value: &Value, span: Span) -> Result<bool, LangError> {
    let concrete = value.concretize();
    if concrete.to_json().is_none() {
        if frame.defer_incomplete.get() {
            return Ok(true);
        }
        return Err(LangError::incomplete_value(
            span.line,
            span.col,
            "non-concrete value in boolean context",
        ));
    }
    match concrete {
        Value::Bool(value) => Ok(*value),
        other => Err(LangError::conflict(
            span.line,
            span.col,
            format!("expected bool, found {}", other.type_name()),
        )),
    }
}

fn conflict(span: Span) -> LangError {
    LangError::conflict(span.line, span.col, "conflicting values")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DefaultMode {
    None,
    Direct,
    Inherited,
}

fn same_disjunction_alternative(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Definition { .. }, _) | (_, Value::Definition { .. }) => left == right,
        _ => values_identical(left, right),
    }
}

fn push_unique_disjunction_value(alternatives: &mut Vec<Value>, value: Value) -> usize {
    if let Some(index) = alternatives
        .iter()
        .position(|existing| same_disjunction_alternative(existing, &value))
    {
        return index;
    }
    let index = alternatives.len();
    alternatives.push(value);
    index
}

fn collect_disjunction_value(
    alternatives: &mut Vec<Value>,
    direct: &mut Vec<usize>,
    inherited: &mut Vec<usize>,
    value: Value,
    mode: DefaultMode,
) {
    match value {
        Value::Disjunction(inner, inner_defaults) => {
            for (index, alternative) in inner.into_iter().enumerate() {
                let child_mode = match mode {
                    DefaultMode::Direct if inner_defaults.is_empty() => DefaultMode::Direct,
                    DefaultMode::Direct if inner_defaults.contains(&index) => DefaultMode::Direct,
                    DefaultMode::Inherited if inner_defaults.contains(&index) => {
                        DefaultMode::Inherited
                    }
                    _ => DefaultMode::None,
                };
                collect_disjunction_value(alternatives, direct, inherited, alternative, child_mode);
            }
        }
        other => {
            let index = push_unique_disjunction_value(alternatives, other);
            match mode {
                DefaultMode::Direct if !direct.contains(&index) => direct.push(index),
                DefaultMode::Inherited if !inherited.contains(&index) => inherited.push(index),
                _ => {}
            }
        }
    }
}

fn push_disjunction_result(
    alternatives: &mut Vec<Value>,
    defaults: &mut Vec<usize>,
    value: Value,
    is_default: bool,
) {
    let mut direct = Vec::new();
    let mut inherited = Vec::new();
    collect_disjunction_value(
        alternatives,
        &mut direct,
        &mut inherited,
        value,
        if is_default {
            DefaultMode::Direct
        } else {
            DefaultMode::None
        },
    );
    for index in direct {
        if !defaults.contains(&index) {
            defaults.push(index);
        }
    }
}

fn disjunction_to_json(alternatives: &[Value], defaults: &[usize]) -> Option<Json> {
    if alternatives.len() == 1 {
        return alternatives[0].to_json();
    }
    if defaults.len() == 1 {
        return alternatives.get(defaults[0])?.to_json();
    }
    None
}

fn unify_disjunction_with(
    alternatives: &[Value],
    defaults: &[usize],
    other: &Value,
    span: Span,
) -> Result<Value, LangError> {
    let mut out = Vec::new();
    let mut out_defaults = Vec::new();
    for (index, alternative) in alternatives.iter().enumerate() {
        if let Ok(value) = unify_values(alternative, other, span) {
            push_disjunction_result(
                &mut out,
                &mut out_defaults,
                value,
                defaults.contains(&index),
            );
        }
    }
    match out.len() {
        0 => Err(LangError::conflict(
            span.line,
            span.col,
            "disjunction has no value satisfying the constraint",
        )),
        1 => Ok(out.remove(0)),
        _ => Ok(Value::Disjunction(out, out_defaults)),
    }
}

fn unify_disjunctions(
    left: &[Value],
    left_defaults: &[usize],
    right: &[Value],
    right_defaults: &[usize],
    span: Span,
) -> Result<Value, LangError> {
    let mut out = Vec::new();
    let mut out_defaults = Vec::new();
    for (left_index, left_value) in left.iter().enumerate() {
        for (right_index, right_value) in right.iter().enumerate() {
            if let Ok(value) = unify_values(left_value, right_value, span) {
                let is_default =
                    left_defaults.contains(&left_index) || right_defaults.contains(&right_index);
                push_disjunction_result(&mut out, &mut out_defaults, value, is_default);
            }
        }
    }
    match out.len() {
        0 => Err(LangError::conflict(
            span.line,
            span.col,
            "disjunction has no value satisfying the other disjunction",
        )),
        1 => Ok(out.remove(0)),
        _ => Ok(Value::Disjunction(out, out_defaults)),
    }
}

/// Convert a JSON number token whose exponent exceeds BigDecimal's range.
/// Preserving the token as a string is preferable to silently merging it as
/// zero, which would turn unrepresentable values into concrete data.
fn float_value_or_token(text: &str) -> Value {
    match text.parse::<BigDecimal>() {
        Ok(value) => Value::Float(value),
        Err(_) => Value::String(text.to_owned()),
    }
}

/// Convert concrete JSON into the evaluator's value type.
///
/// This is used when the official CUE CLI has already produced a concrete
/// export and the workflow compiler only needs the shared extraction logic.
#[must_use]
pub fn value_from_json(value: &Json) -> Value {
    match value {
        Json::Null => Value::Null,
        Json::Bool(value) => Value::Bool(*value),
        Json::Number(number) => {
            let text = number.to_string();
            let integral = !text.contains(['.', 'e', 'E']);
            if integral {
                if let Some(int) = number.as_i64() {
                    Value::Int(int)
                } else {
                    match text.parse::<BigInt>() {
                        Ok(big) => Value::BigInt(big),
                        Err(_) => float_value_or_token(&text),
                    }
                }
            } else {
                float_value_or_token(&text)
            }
        }
        Json::String(value) => Value::String(value.clone()),
        Json::Array(values) => Value::List(values.iter().map(value_from_json).collect()),
        Json::Object(map) => Value::Struct(
            map.iter()
                .map(|(key, value)| (key.clone(), value_from_json(value)))
                .collect(),
        ),
    }
}

/// Unify two evaluated values using CUE semantics.
///
/// This is the public entry point used by data-file inputs and by embedders.
///
/// # Errors
/// Returns an evaluation error for incompatible types or conflicting concrete
/// values.
pub fn unify(left: &Value, right: &Value) -> Result<Value, LangError> {
    unify_values(left, right, Span::default())
}
