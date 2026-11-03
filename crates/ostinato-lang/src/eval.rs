use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use bigdecimal::{BigDecimal, RoundingMode};
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

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
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
            Self::Int(_) => "int",
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
    pub fn is_concrete(&self) -> bool {
        match self {
            Self::Disjunction(_, _) => self.to_json().is_some(),
            Self::Bounds(_) => false,
            _ => !matches!(self, Self::Type(_) | Self::Bottom | Self::Conflict(_)),
        }
    }

    /// Convert to JSON, returning `None` for incomplete values.
    #[must_use]
    pub fn to_json(&self) -> Option<Json> {
        match self {
            Self::Null => Some(Json::Null),
            Self::Bool(value) => Some(Json::Bool(*value)),
            Self::Int(value) => Some(Json::Number(Number::from(*value))),
            Self::Float(value) => {
                let mut text = value.to_string();
                if !text.contains(['.', 'e', 'E']) {
                    text.push_str(".0");
                }
                text.parse::<Number>().ok().map(Json::Number)
            }
            Self::String(value) => Some(Json::String(value.clone())),
            Self::Bytes(value) => Some(Json::String(base64_encode(value))),
            Self::List(values) => {
                let mut out = Vec::with_capacity(values.len());
                for value in values {
                    out.push(value.to_json()?);
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
                    out.insert(key.to_owned(), value.to_json()?);
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
                    out.push(value.to_json()?);
                }
                Some(Json::Array(out))
            }
            Self::OpenList(_) => Some(Json::Array(Vec::new())),
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
            Self::Builtin(_) | Self::Type(_) | Self::Bottom | Self::Conflict(_) => false,
        }
    }

    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::Null => "null".to_owned(),
            Self::Bool(value) => value.to_string(),
            Self::Int(value) => value.to_string(),
            Self::Float(value) => {
                let mut text = value.to_string();
                if !text.contains(['.', 'e', 'E']) {
                    text.push_str(".0");
                }
                text
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
            Self::List(_) | Self::Struct(_) | Self::Definition { .. } => {
                self.to_json().map_or_else(
                    || format!("<{}>", self.type_name()),
                    |json| json.to_string(),
                )
            }
        }
    }
}

/// Evaluate a parsed file into a concrete-ish value tree.
///
/// # Errors
/// Returns evaluation errors for undefined references, cyclic references,
/// conflicting values and non-concrete disjunctions.
pub fn eval_file(file: &File) -> Result<Value, LangError> {
    crate::scope::validate_aliases(&file.items, &[])?;
    let frame = Frame::new(&file.items, None, HashMap::new())?;
    frame.ensure_params_context();
    frame.to_struct_value()
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
    let import_aliases: Vec<String> = imports.keys().cloned().collect();
    crate::scope::validate_aliases(&file.items, &import_aliases)?;
    let mut vars = HashMap::new();
    for (name, value) in imports {
        vars.insert(name.clone(), value.clone());
    }
    let frame = Frame::from_static(&file.items, None, vars)?;
    for (key, value) in overrides {
        frame
            .overrides
            .borrow_mut()
            .insert(key.clone(), value.clone());
    }
    frame.ensure_params_context();
    frame.expand(&file.items)?;
    frame.to_struct_value_mode(include_definitions)
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
                Value::Float(number.to_string().parse::<BigDecimal>().ok()?)
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
                let condition = require_bool(&eval_expr(cond, self)?, *span)?;
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
            return fields
                .get(name)
                .is_some_and(|definition| definition.definition)
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
        let Value::Struct(values) = child.to_struct_value()? else {
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
            if optional {
                return Err(LangError::eval(
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
            if optional {
                return Err(LangError::eval(
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
                        return Err(LangError::eval(
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

    fn struct_conjunct(expr: &Expr) -> bool {
        match expr {
            Expr::Struct(_, _) => true,
            Expr::Paren(inner, _) | Expr::Default(inner, _) => Self::struct_conjunct(inner),
            Expr::Unify(left, right, _) => {
                Self::struct_conjunct(left) && Self::struct_conjunct(right)
            }
            _ => false,
        }
    }

    fn eval_field_key(&self, key: &str, span: Span) -> Result<Value, LangError> {
        let name = display_field_name(key);
        if let Some(value) = self.values.borrow().get(key) {
            return Ok(value.clone());
        }
        if self.evaluating.borrow().contains(key) {
            return Err(LangError::eval(
                span.line,
                span.col,
                format!("cyclic reference while evaluating `{name}`"),
            ));
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
            if self.evaluating.borrow().contains(key) {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("cyclic reference while evaluating `{name}`"),
                ));
            }
            self.evaluating.borrow_mut().insert(key.to_owned());
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
                    Err(err) if err.to_string().contains("cannot reference optional field") => {}
                    Err(err) => return Err(err),
                }
            }
            self.values
                .borrow_mut()
                .insert(key.to_owned(), result.clone());
            return Ok(result);
        }
        if exprs.len() > 1
            && exprs.iter().all(Self::struct_conjunct)
            && let Some(items) = self.params_items(&exprs)
        {
            self.evaluating.borrow_mut().insert(key.to_owned());
            let result = self.eval_struct_items(&items, span, false);
            self.evaluating.borrow_mut().remove(key);
            let result = result?;
            self.values
                .borrow_mut()
                .insert(key.to_owned(), result.clone());
            return Ok(result);
        }
        self.evaluating.borrow_mut().insert(key.to_owned());
        let segment = self
            .within_params()
            .then(|| display_field_name(key).to_owned());
        let mut result = Value::Bottom;
        for (index, expr) in exprs.iter().enumerate() {
            let previous = segment
                .as_ref()
                .map(|segment| self.params_segment.replace(Some(segment.clone())));
            let value = eval_expr(expr, self);
            if let Some(previous) = previous {
                *self.params_segment.borrow_mut() = previous;
            }
            let value = value?;
            result = if index == 0 {
                value
            } else {
                unify_values(&result, &value, span)?
            };
        }
        self.evaluating.borrow_mut().remove(key);
        self.values
            .borrow_mut()
            .insert(key.to_owned(), result.clone());
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
                    definition.exprs.clone()
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
                || child.values.borrow().contains_key(key)
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
                let value = if self.overrides.borrow().is_empty() {
                    self.lookup(name, span)?
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
                    self.lookup(name, span)?
                };
                if matches!(value, Value::Definition { closed: true, .. }) {
                    *result = unify_values(result, &value, span)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn eval_let(&self, name: &str, span: Span) -> Result<Value, LangError> {
        if let Some(value) = self.vars.borrow().get(name) {
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
        self.vars
            .borrow_mut()
            .insert(name.to_owned(), value.clone());
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
            let value = self.eval_field_key(&name, Span::default())?;
            let display = output_field_name(&name);
            if self
                .fields
                .borrow()
                .get(&name)
                .is_some_and(|def| def.definition)
            {
                definitions_out.insert(display, value);
            } else {
                fields_out.insert(display, value);
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
    match expr {
        Expr::Null(_) => Ok(Value::Null),
        Expr::Bool(value, _) => Ok(Value::Bool(*value)),
        Expr::Int(value, _) => Ok(Value::Int(*value)),
        Expr::Float(value, _) => Ok(Value::Float(value.clone())),
        Expr::Bytes(value, _) => Ok(Value::Bytes(value.clone())),
        Expr::Str(parts, _) => {
            let mut out = String::new();
            for part in parts {
                match part {
                    StrPart::Text(text) => out.push_str(text),
                    StrPart::Interp(expr) => {
                        let value = eval_expr(expr, frame)?;
                        let concrete = value.concretize();
                        if concrete.to_json().is_none() {
                            return Err(LangError::eval(
                                1,
                                1,
                                "non-concrete value in string interpolation",
                            ));
                        }
                        match &concrete {
                            Value::Bytes(bytes) => {
                                out.push_str(&String::from_utf8_lossy(bytes));
                            }
                            Value::String(_) | Value::Bool(_) | Value::Int(_) | Value::Float(_) => {
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
            if let Some(path) = params_select_path(expr, frame)
                && !path.is_empty()
                && frame.params_context.borrow().is_some()
                && !frame.params_shadowed()
            {
                return frame.params_path_value(&path, *span);
            }
            let base = eval_expr(base, frame)?.concretize().clone();
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
                        Err(LangError::eval(
                            span.line,
                            span.col,
                            format!("cannot reference optional field `{field}`"),
                        ))
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
            apply_builtin(&callee, &values, *span)
        }
        Expr::Index(base, index, span) => {
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
            match (base, index) {
                (Value::List(list) | Value::OpenListPrefix(list, _), Value::Int(index)) => {
                    let index = usize::try_from(index).map_err(|_| {
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
            for alternative in ast_alternatives {
                match alternative {
                    Expr::Default(inner, _) => {
                        let value = match eval_expr(inner, frame) {
                            Ok(value) => value,
                            Err(err) if err.is_conflict() => continue,
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
                return Err(LangError::eval(span.line, span.col, "empty disjunction"));
            }
            let defaults = if direct.is_empty() { inherited } else { direct };
            Ok(Value::Disjunction(alternatives, defaults))
        }
        Expr::Default(inner, _) => eval_expr(inner, frame),
        Expr::Binary(op, left, right, span) => {
            if matches!(op, BinOp::And | BinOp::Or) {
                let left = require_bool(&eval_expr(left, frame)?, *span)?;
                let right = require_bool(&eval_expr(right, frame)?, *span)?;
                return Ok(Value::Bool(if matches!(op, BinOp::And) {
                    left && right
                } else {
                    left || right
                }));
            }
            let left = eval_expr(left, frame)?.concretize().clone();
            let right = eval_expr(right, frame)?.concretize().clone();
            if !matches!(op, BinOp::Eq | BinOp::Ne)
                || matches!(
                    (&left, &right),
                    (Value::Disjunction(_, _), _)
                        | (_, Value::Disjunction(_, _))
                        | (Value::Bounds(_), _)
                        | (_, Value::Bounds(_))
                )
            {
                require_concrete(&left, *span)?;
                require_concrete(&right, *span)?;
            }
            binary(op, &left, &right, *span)
        }
        Expr::Unary(op, inner, span) => {
            let value = eval_expr(inner, frame)?.concretize().clone();
            require_concrete(&value, *span)?;
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
            if require_bool(&eval_expr(cond, frame)?, *span)? {
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
        Value::Int(_) | Value::Float(_) => Some(BoundClass::Numeric),
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
            matches!(operand, Value::Int(_) | Value::Float(_) | Value::String(_))
        }
        BoundKind::Match | BoundKind::NotMatch => {
            matches!(operand, Value::Int(_) | Value::Float(_) | Value::String(_))
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
                matches!(operand, Value::Int(_) | Value::Float(_))
            }
            TypeKind::Bool | TypeKind::Bytes => false,
        },
        BoundKind::Match | BoundKind::NotMatch => match ty {
            TypeKind::String => matches!(operand, Value::String(_)),
            TypeKind::Int | TypeKind::Float | TypeKind::Number => {
                matches!(operand, Value::Int(_) | Value::Float(_))
            }
            TypeKind::Bool | TypeKind::Bytes => false,
        },
        BoundKind::Ne => match ty {
            TypeKind::String => matches!(operand, Value::String(_) | Value::Null),
            TypeKind::Int | TypeKind::Float | TypeKind::Number => {
                matches!(operand, Value::Int(_) | Value::Float(_) | Value::Null)
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
                Some(BoundClass::Numeric) => matches!(value, Value::Int(_) | Value::Float(_)),
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
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Int(a), Value::Float(b)) => BigDecimal::from(*a).partial_cmp(b),
        (Value::Float(a), Value::Int(b)) => a.partial_cmp(&BigDecimal::from(*b)),
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
        (Value::Int(_), Value::Float(_)) | (Value::Float(_), Value::Int(_))
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
    match (left, right) {
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
        ],
        _ => return None,
    };
    let mut values: BTreeMap<String, Value> = entries
        .iter()
        .map(|(name, builtin)| ((*name).to_owned(), Value::Builtin(*builtin)))
        .collect();
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
            let Some(Value::Int(limit)) = arguments.get(2) else {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "FindAll requires an integer limit",
                ));
            };
            let values: Vec<Value> = if *limit < 0 {
                regex
                    .find_iter(text)
                    .map(|found| Value::String(found.as_str().to_owned()))
                    .collect()
            } else {
                regex
                    .find_iter(text)
                    .take(usize::try_from(*limit).unwrap_or(0))
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
        Value::Float(value) => value.to_string().parse::<f64>().ok(),
        _ => None,
    }
}

fn numeric_value_decimal(value: &Value) -> Option<BigDecimal> {
    match value {
        Value::Int(value) => Some(BigDecimal::from(*value)),
        Value::Float(value) => Some(value.clone()),
        _ => None,
    }
}

fn math_decimal(arguments: &[Value], index: usize, span: Span) -> Result<BigDecimal, LangError> {
    match arguments.get(index) {
        Some(Value::Int(value)) => Ok(BigDecimal::from(*value)),
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
            Some(Value::Float(value)) => Ok(Value::Float(value.abs())),
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
        Builtin::MathSqrt => Ok(math_number(math_arg(arguments, 0, span)?.sqrt())),
        Builtin::MathPow => Ok(math_number(
            math_arg(arguments, 0, span)?.powf(math_arg(arguments, 1, span)?),
        )),
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
        let mut fraction: i128 = 0;
        let mut fraction_digits = 0u32;
        if let Some(after) = rest.strip_prefix('.') {
            let digit_len = after.chars().take_while(char::is_ascii_digit).count();
            if integer_len == 0 && digit_len == 0 {
                return None;
            }
            if digit_len > 0 {
                fraction = after[..digit_len].parse().ok()?;
                fraction_digits = u32::try_from(digit_len).ok()?;
            }
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
        if fraction_digits > 0 {
            let divisor = 10i128.checked_pow(fraction_digits)?;
            value = value.checked_add(fraction.checked_mul(scale)?.checked_div(divisor)?)?;
        }
        total = total.checked_add(value)?;
    }
    total = total.checked_mul(sign)?;
    i64::try_from(total).ok()
}

fn rfc3339_nano(datetime: &chrono::DateTime<chrono::Utc>) -> String {
    let text = datetime.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    let Some(dot) = text.find('.') else {
        return text;
    };
    let end = text[dot..]
        .find(['Z', '+', '-'])
        .map_or(text.len(), |offset| dot + offset);
    let fraction = &text[dot + 1..end];
    let trimmed = fraction.trim_end_matches('0');
    if trimmed.is_empty() {
        format!("{}{}", &text[..dot], &text[end..])
    } else {
        format!("{}.{}{}", &text[..dot], trimmed, &text[end..])
    }
}

fn valid_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.len() == 3
        && parts[0].len() == 4
        && parts[1].len() == 2
        && parts[2].len() == 2
        && parts
            .iter()
            .all(|part| part.chars().all(|value| value.is_ascii_digit()))
}

fn valid_clock(text: &str) -> bool {
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() != 3 {
        return false;
    }
    let hour_ok =
        (1..=2).contains(&parts[0].len()) && parts[0].chars().all(|value| value.is_ascii_digit());
    let minute_ok = parts[1].len() == 2 && parts[1].chars().all(|value| value.is_ascii_digit());
    let second_ok = parts[2].len() == 2
        && parts[2].chars().all(|value| value.is_ascii_digit())
        && parts[2].parse::<u8>().is_ok_and(|second| second < 60);
    hour_ok && minute_ok && second_ok
}

fn parse_ampm(text: &str) -> Option<(u32, u32)> {
    let (body, suffix) = match text.strip_suffix("AM") {
        Some(body) => (body, false),
        None => (text.strip_suffix("PM")?, true),
    };
    let (hour, minute) = body.split_once(':')?;
    if !(1..=2).contains(&hour.len()) || minute.len() != 2 {
        return None;
    }
    let hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;
    if hour > 12 || minute > 59 {
        return None;
    }
    let hour = (hour % 12) + if suffix { 12 } else { 0 };
    Some((hour, minute))
}

fn valid_rfc3339_shape(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|slice| slice.iter().all(u8::is_ascii_digit))
    };
    if !digits(0..4) || !digits(5..7) || !digits(8..10) || !digits(11..13) || !digits(14..16) {
        return false;
    }
    if !digits(17..19)
        || text[17..19]
            .parse::<u8>()
            .map_or(true, |second| second >= 60)
    {
        return false;
    }
    if text.ends_with('Z') {
        return true;
    }
    let rest = text.get(19..).unwrap_or("");
    let rest = match rest.strip_prefix(['.', ',']) {
        Some(fraction) => {
            let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 {
                return false;
            }
            &fraction[digits..]
        }
        None => rest,
    };
    rest.starts_with('+') || rest.starts_with('-')
}

fn split_fraction(text: &str) -> Option<(&str, i64)> {
    let Some(index) = text.find(['.', ',']) else {
        return Some((text, 0));
    };
    let base = &text[..index];
    let fraction = &text[index + 1..];
    if fraction.is_empty() || !fraction.chars().all(|value| value.is_ascii_digit()) {
        return None;
    }
    let mut nanos = 0i64;
    let mut scale = 100_000_000i64;
    for digit in fraction.chars().take(9) {
        if let Some(value) = digit.to_digit(10) {
            nanos += i64::from(value) * scale;
        }
        scale /= 10;
    }
    Some((base, nanos))
}

fn parse_layout(
    layout: &str,
    text: &str,
    span: Span,
) -> Result<chrono::DateTime<chrono::Utc>, LangError> {
    use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
    let invalid = || LangError::conflict(span.line, span.col, format!("invalid time `{text}`"));
    match layout {
        "2006-01-02" => {
            if !valid_date(text) {
                return Err(invalid());
            }
            let date = NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| invalid())?;
            Ok(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).ok_or_else(invalid)?))
        }
        "15:04:05" => {
            let (base, nanos) = split_fraction(text).ok_or_else(invalid)?;
            if !valid_clock(base) {
                return Err(invalid());
            }
            let time = NaiveTime::parse_from_str(base, "%H:%M:%S").map_err(|_| invalid())?;
            let date = NaiveDate::from_ymd_opt(0, 1, 1).ok_or_else(invalid)?;
            Ok(Utc.from_utc_datetime(&date.and_time(time)) + chrono::Duration::nanoseconds(nanos))
        }
        "3:04PM" => {
            let (hour, minute) = parse_ampm(text).ok_or_else(invalid)?;
            let time = NaiveTime::from_hms_opt(hour, minute, 0).ok_or_else(invalid)?;
            let date = NaiveDate::from_ymd_opt(0, 1, 1).ok_or_else(invalid)?;
            Ok(Utc.from_utc_datetime(&date.and_time(time)))
        }
        "2006-01-02 15:04:05" => {
            let (base, nanos) = split_fraction(text).ok_or_else(invalid)?;
            let (date, clock) = base.split_once(' ').ok_or_else(invalid)?;
            if !valid_date(date) || !valid_clock(clock) {
                return Err(invalid());
            }
            let naive = chrono::NaiveDateTime::parse_from_str(base, "%Y-%m-%d %H:%M:%S")
                .map_err(|_| invalid())?;
            Ok(Utc.from_utc_datetime(&naive) + chrono::Duration::nanoseconds(nanos))
        }
        "2006-01-02T15:04:05Z07:00" => {
            let normalized = text.replacen(',', ".", 1);
            if !valid_rfc3339_shape(&normalized) {
                return Err(invalid());
            }
            DateTime::parse_from_rfc3339(&normalized)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|_| invalid())
        }
        _ => Err(LangError::conflict(
            span.line,
            span.col,
            format!("unsupported time layout `{layout}`"),
        )),
    }
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
        _ => unreachable!("time builtin"),
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
        | Builtin::TimeDuration => apply_time_builtin(*builtin, arguments, span),
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
                if let Ok(value) = normalized.to_string().parse::<i64>() {
                    out.push(Value::Int(value));
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
        BinOp::Add => match (left, right) {
            (Value::Int(a), Value::Int(b)) => a
                .checked_add(*b)
                .map(Value::Int)
                .ok_or_else(|| overflow(span)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(BigDecimal::from(*a) + b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + BigDecimal::from(*b))),
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
        },
        BinOp::Sub => numeric(op, left, right, span, |a, b| a - b, |a, b| a - b),
        BinOp::Mul => numeric(op, left, right, span, |a, b| a * b, |a, b| a * b),
        BinOp::Div => {
            let zero = matches!(right, Value::Int(0))
                || matches!(right, Value::Float(value) if value == &BigDecimal::from(0));
            if zero {
                return Err(LangError::conflict(span.line, span.col, "division by zero"));
            }
            match (left, right) {
                (Value::Int(a), Value::Int(b)) => {
                    Ok(Value::Float(BigDecimal::from(*a) / BigDecimal::from(*b)))
                }
                _ => numeric(op, left, right, span, |a, b| a / b, |a, b| a / b),
            }
        }
        BinOp::Mod => match (left, right) {
            (Value::Int(_), Value::Int(0)) => {
                Err(LangError::conflict(span.line, span.col, "modulo by zero"))
            }
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a % b)),
            _ => type_error("mod", left, right, span),
        },
        BinOp::Eq => Ok(Value::Bool(values_equal(left, right))),
        BinOp::Ne => Ok(Value::Bool(!values_equal(left, right))),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => compare(op, left, right, span),
        BinOp::And | BinOp::Or => Ok(Value::Bool(match op {
            BinOp::And => left.truthy() && right.truthy(),
            _ => left.truthy() || right.truthy(),
        })),
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
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(int_op(*a, *b))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_op(a.clone(), b.clone()))),
        (Value::Int(a), Value::Float(b)) => {
            Ok(Value::Float(float_op(BigDecimal::from(*a), b.clone())))
        }
        (Value::Float(a), Value::Int(b)) => {
            Ok(Value::Float(float_op(a.clone(), BigDecimal::from(*b))))
        }
        _ => type_error(op_name(op), left, right, span),
    }
}

fn compare(op: &BinOp, left: &Value, right: &Value, span: Span) -> Result<Value, LangError> {
    let ordering = match (left, right) {
        (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Int(a), Value::Float(b)) => BigDecimal::from(*a).partial_cmp(b),
        (Value::Float(a), Value::Int(b)) => a.partial_cmp(&BigDecimal::from(*b)),
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

fn unify_values(left: &Value, right: &Value, span: Span) -> Result<Value, LangError> {
    match (left, right) {
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
            for (expected, value) in prefix.iter().zip(values) {
                let _ = unify_values(expected, value, span)?;
            }
            if let Some(tail) = tail {
                for value in values.iter().skip(prefix.len()) {
                    let _ = unify_values(tail, value, span)?;
                }
            }
            Ok(Value::List(values.clone()))
        }
        (Value::OpenListPrefix(prefix, tail), Value::OpenList(element))
        | (Value::OpenList(element), Value::OpenListPrefix(prefix, tail)) => {
            let mut out = prefix.clone();
            if let Some(tail) = tail {
                out.push((**tail).clone());
            }
            let mut merged_tail = element.clone();
            if let Some(element) = element
                && let Some(tail) = tail
            {
                merged_tail = Some(Box::new(unify_values(tail, element, span)?));
            }
            Ok(Value::OpenListPrefix(out, merged_tail))
        }
        (
            Value::OpenListPrefix(left_prefix, left_tail),
            Value::OpenListPrefix(right_prefix, right_tail),
        ) => {
            if left_prefix.len() != right_prefix.len() {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "cannot unify list prefixes of different lengths",
                ));
            }
            let mut prefix = Vec::with_capacity(left_prefix.len());
            for (left, right) in left_prefix.iter().zip(right_prefix) {
                prefix.push(unify_values(left, right, span)?);
            }
            let tail = match (left_tail, right_tail) {
                (None, None) => None,
                (Some(left), Some(right)) => Some(Box::new(unify_values(left, right, span)?)),
                (Some(tail), None) | (None, Some(tail)) => Some(tail.clone()),
            };
            Ok(Value::OpenListPrefix(prefix, tail))
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
        TypeKind::Int => matches!(value, Value::Int(_)),
        TypeKind::Float => matches!(value, Value::Float(_)),
        TypeKind::Number => matches!(value, Value::Int(_) | Value::Float(_)),
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

fn require_concrete(value: &Value, span: Span) -> Result<&Value, LangError> {
    let concrete = value.concretize();
    if concrete.to_json().is_none() {
        return Err(LangError::eval(
            span.line,
            span.col,
            "unresolved disjunction or non-concrete value",
        ));
    }
    Ok(concrete)
}

fn require_bool(value: &Value, span: Span) -> Result<bool, LangError> {
    let concrete = require_concrete(value, span)?;
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
            if let Some(int) = number.as_i64() {
                Value::Int(int)
            } else {
                Value::Float(
                    number
                        .to_string()
                        .parse::<BigDecimal>()
                        .unwrap_or_else(|_| BigDecimal::from(0)),
                )
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
