use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::{Map, Number, Value as Json};

use crate::ast::{BinOp, CompClause, Expr, File, Pattern, Span, StrPart, StructItem, UnOp};
use crate::error::LangError;

/// Runtime value produced by the CUE-compatible evaluator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admission {
    pub names: BTreeSet<String>,
    pub patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    List(Vec<Value>),
    Struct(BTreeMap<String, Value>),
    /// A disjunction with optional default alternatives (indices).
    Disjunction(Vec<Value>, Vec<usize>),
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    String,
    Int,
    Float,
    Number,
    Bool,
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
            Self::List(_) => "list",
            Self::Struct(_) | Self::Definition { .. } => "struct",
            Self::Disjunction(_, _) => "disjunction",
            Self::OpenList(_) | Self::OpenListPrefix(_, _) => "list",
            Self::Builtin(_) => "builtin",
            Self::Type(kind) => match kind {
                TypeKind::String => "string",
                TypeKind::Int => "int",
                TypeKind::Float => "float",
                TypeKind::Number => "number",
                TypeKind::Bool => "bool",
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
            Self::Float(value) => Number::from_f64(*value).map(Json::Number),
            Self::String(value) => Some(Json::String(value.clone())),
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
                    out.insert(key.clone(), value.to_json()?);
                }
                Some(Json::Object(out))
            }
            Self::Disjunction(alternatives, defaults) => {
                disjunction_to_json(alternatives, defaults)
            }
            Self::OpenList(_)
            | Self::OpenListPrefix(_, _)
            | Self::Builtin(_)
            | Self::Conflict(_)
            | Self::Type(_)
            | Self::Bottom => None,
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
            Self::Float(value) => *value != 0.0,
            Self::String(value) => !value.is_empty(),
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
            Self::Float(value) => value.to_string(),
            Self::String(value) => value.clone(),
            Self::Type(kind) => match kind {
                TypeKind::String => "string",
                TypeKind::Int => "int",
                TypeKind::Float => "float",
                TypeKind::Number => "number",
                TypeKind::Bool => "bool",
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
                Value::Float(number.as_f64()?)
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

struct FieldDef {
    exprs: Vec<Expr>,
    optional: bool,
    definition: bool,
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
    aliases: RefCell<BTreeMap<String, String>>,
    closed: Cell<bool>,
    overrides: RefCell<BTreeMap<String, Value>>,
    evaluating: RefCell<HashSet<String>>,
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
                        );
                        if let Some(alias) = &field.alias {
                            frame
                                .aliases
                                .borrow_mut()
                                .insert(alias.clone(), name.clone());
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
        // Pass 2: dynamic labels, comprehensions and conditionals.
        for item in items {
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
                        self.insert_field(key.clone(), field.value.clone(), field.optional, false);
                        if let Some(alias) = &field.alias {
                            self.aliases.borrow_mut().insert(alias.clone(), key);
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
                                let mut bindings = HashMap::new();
                                if let Some(key) = key {
                                    bindings.insert(key.clone(), Value::String(entry_key));
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
        }
        Ok(())
    }

    fn insert_field(&self, name: String, expr: Expr, optional: bool, definition: bool) {
        let mut fields = self.fields.borrow_mut();
        fields
            .entry(name)
            .and_modify(|def| {
                def.exprs.push(expr.clone());
                def.optional = def.optional && optional;
                def.definition = def.definition || definition;
            })
            .or_insert(FieldDef {
                exprs: vec![expr],
                optional,
                definition,
            });
    }

    fn reconcile_generated_required_fields(&self) -> Result<(), LangError> {
        let names: Vec<String> = self
            .generated_optionals
            .borrow()
            .keys()
            .filter(|name| {
                self.fields
                    .borrow()
                    .get(*name)
                    .is_some_and(|field| !field.optional)
            })
            .cloned()
            .collect();
        for name in names {
            if let Some(constraint) = self.generated_optionals.borrow_mut().remove(&name) {
                let value = self.eval_field(&name, Span::default())?;
                let merged = unify_values(&value, &constraint, Span::default())?;
                self.values.borrow_mut().insert(name.clone(), merged);
                self.materialized.borrow_mut().insert(name);
            }
        }
        Ok(())
    }

    fn merge_generated_constraint(&self, name: String, value: Value) -> Result<(), LangError> {
        let declared = self
            .fields
            .borrow()
            .get(&name)
            .map(|definition| (definition.optional, definition.definition));
        let forced_required = match declared {
            Some((false, false)) => true,
            Some(_) => false,
            None => {
                self.materialized.borrow().contains(&name)
                    || self.values.borrow().contains_key(&name)
            }
        };
        if forced_required {
            let cached = self.values.borrow().get(&name).cloned();
            let current = match cached {
                Some(value) => value,
                None => self.eval_field(&name, Span::default())?,
            };
            let merged = unify_values(&current, &value, Span::default())?;
            self.values.borrow_mut().insert(name.clone(), merged);
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
            let existing = self.values.borrow().get(&key).cloned();
            let existing = match existing {
                Some(existing) => Some(existing),
                None if self.fields.borrow().contains_key(&key) => {
                    Some(self.eval_field(&key, Span::default())?)
                }
                None => None,
            };
            let merged = match existing {
                Some(existing) => unify_values(&existing, &value, Span::default())?,
                None => value,
            };
            self.materialized.borrow_mut().insert(key.clone());
            self.values.borrow_mut().insert(key, merged);
        }
        let optional_names: Vec<String> = child
            .fields
            .borrow()
            .iter()
            .filter(|(_, definition)| definition.optional)
            .map(|(name, _)| name.clone())
            .collect();
        for name in optional_names {
            let value = child.eval_field(&name, Span::default())?;
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
            return self.eval_let(name, span);
        }
        if let Some(target) = self.aliases.borrow().get(name).cloned() {
            let optional = self
                .fields
                .borrow()
                .get(&target)
                .is_some_and(|definition| definition.optional);
            if optional {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("cannot reference optional field `{target}`"),
                ));
            }
            return self.eval_field(&target, span);
        }
        if let Some(value) = self.values.borrow().get(name) {
            return Ok(value.clone());
        }
        let optional = self
            .fields
            .borrow()
            .get(name)
            .is_some_and(|definition| definition.optional);
        if optional {
            return Err(LangError::eval(
                span.line,
                span.col,
                format!("cannot reference optional field `{name}`"),
            ));
        }
        if self.fields.borrow().contains_key(name) {
            return self.eval_field(name, span);
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
        self.values.borrow().contains_key(name)
            || self.fields.borrow().contains_key(name)
            || self.vars.borrow().contains_key(name)
            || self.lets.borrow().contains_key(name)
            || self.aliases.borrow().contains_key(name)
            || self.parent.is_some_and(|parent| parent.has_name(name))
    }

    fn eval_field(&self, name: &str, span: Span) -> Result<Value, LangError> {
        if let Some(value) = self.values.borrow().get(name) {
            return Ok(value.clone());
        }
        if self.evaluating.borrow().contains(name) {
            return Err(LangError::eval(
                span.line,
                span.col,
                format!("cyclic reference while evaluating `{name}`"),
            ));
        }
        let (exprs, is_definition) = {
            let fields = self.fields.borrow();
            let Some(def) = fields.get(name) else {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("undefined reference `{name}`"),
                ));
            };
            (def.exprs.clone(), def.definition)
        };
        if name == "params"
            && !self.overrides.borrow().is_empty()
            && let Some(items) = self.params_items(&exprs)
        {
            return self.eval_params_struct(&items, span);
        }
        if is_definition {
            if self.evaluating.borrow().contains(name) {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("cyclic reference while evaluating `{name}`"),
                ));
            }
            self.evaluating.borrow_mut().insert(name.to_owned());
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
            self.evaluating.borrow_mut().remove(name);
            let mut result = result?;
            let mut references = Vec::new();
            if flattened {
                for expr in &exprs {
                    collect_definition_references(expr, &mut references);
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
                .insert(name.to_owned(), result.clone());
            return Ok(result);
        }
        self.evaluating.borrow_mut().insert(name.to_owned());
        let mut result = Value::Bottom;
        for (index, expr) in exprs.iter().enumerate() {
            let value = eval_expr(expr, self)?;
            result = if index == 0 {
                value
            } else {
                unify_values(&result, &value, span)?
            };
        }
        self.evaluating.borrow_mut().remove(name);
        self.values
            .borrow_mut()
            .insert(name.to_owned(), result.clone());
        Ok(result)
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
        child.closed.set(closed_by_default);
        child.expand(items)?;
        child.reconcile_generated_required_fields()?;
        let mut definition_names = BTreeSet::new();
        for item in items {
            if let StructItem::Field(field) = item
                && field.definition
                && let crate::ast::Label::Name(name, _) = &field.label
            {
                definition_names.insert(name.clone());
            }
        }
        let mut names: Vec<String> = child.fields.borrow().keys().cloned().collect();
        names.extend(child.values.borrow().keys().cloned());
        names.extend(child.generated_optionals.borrow().keys().cloned());
        names.sort();
        names.dedup();
        let mut fields = BTreeMap::new();
        let mut optional = BTreeMap::new();
        let mut definitions = BTreeMap::new();
        for name in names {
            if definition_names.contains(&name) {
                let optional = child
                    .fields
                    .borrow()
                    .get(&name)
                    .is_some_and(|definition| definition.optional);
                if !optional {
                    definitions.insert(name.clone(), child.eval_field(&name, span)?);
                }
                continue;
            }
            let declared_optional = child
                .fields
                .borrow()
                .get(&name)
                .is_some_and(|definition| definition.optional);
            let generated = child.generated_optionals.borrow().get(&name).cloned();
            let materialized = child.materialized.borrow().contains(&name);
            let optional_field = !materialized && (declared_optional || generated.is_some());
            if optional_field {
                let mut value = if child.fields.borrow().contains_key(&name) {
                    match child.eval_field(&name, span) {
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
                optional.insert(name, value);
            } else {
                let mut value = child.eval_field(&name, span)?;
                if let Some(constraint) = generated {
                    value = unify_values(&value, &constraint, span)?;
                }
                fields.insert(name, value);
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
            names.extend(definitions.keys().cloned());
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
                let exprs = self
                    .fields
                    .borrow()
                    .get(name)
                    .map(|definition| definition.exprs.clone())?;
                for expr in &exprs {
                    self.collect_struct_items(expr, out, seen, depth + 1)?;
                }
                seen.remove(name);
            }
            _ => return None,
        }
        Some(())
    }

    fn eval_params_struct(&self, items: &[StructItem], span: Span) -> Result<Value, LangError> {
        let child = Frame::from_static(items, Some(self), HashMap::new())?;
        child.expand(items)?;
        let overrides = self.overrides.borrow().clone();
        for (key, value) in &overrides {
            let known =
                child.fields.borrow().contains_key(key) || child.values.borrow().contains_key(key);
            if !known {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("unknown parameter `{key}`"),
                ));
            }
            // Pre-seed every override so dependent parameters can see each
            // other regardless of declaration or alphabetical order.
            child.values.borrow_mut().insert(key.clone(), value.clone());
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
            child.values.borrow_mut().insert(key.clone(), resolved);
        }
        child.to_struct_value()
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
        // A `let` is not in scope in its own value: a same-named identifier
        // in the RHS resolves to an enclosing let or alias.
        let expr = self.lets.borrow_mut().remove(name);
        let Some(expr) = expr else {
            return Err(LangError::eval(
                span.line,
                span.col,
                format!("undefined let binding `{name}`"),
            ));
        };
        self.evaluating.borrow_mut().insert(name.to_owned());
        let result = eval_expr(&expr, self);
        self.evaluating.borrow_mut().remove(name);
        self.lets.borrow_mut().insert(name.to_owned(), expr);
        let value = result?;
        self.vars
            .borrow_mut()
            .insert(name.to_owned(), value.clone());
        Ok(value)
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
                        && !generated.contains_key(*name)
                })
                .map(|(name, _)| name.clone())
                .collect()
        };
        let mut out = BTreeMap::new();
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
            let value = self.eval_field(&name, Span::default())?;
            out.insert(name, value);
        }
        for (name, value) in self.values.borrow().iter() {
            let definition = self
                .fields
                .borrow()
                .get(name)
                .is_some_and(|definition| definition.definition);
            if (include_definitions || !definition)
                && !self.generated_optionals.borrow().contains_key(name)
            {
                out.insert(name.clone(), value.clone());
            }
        }
        Ok(Value::Struct(out))
    }
}

fn eval_expr(expr: &Expr, frame: &Frame<'_>) -> Result<Value, LangError> {
    match expr {
        Expr::Null(_) => Ok(Value::Null),
        Expr::Bool(value, _) => Ok(Value::Bool(*value)),
        Expr::Int(value, _) => Ok(Value::Int(*value)),
        Expr::Float(value, _) => Ok(Value::Float(*value)),
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
                        out.push_str(&concrete.display());
                    }
                }
            }
            Ok(Value::String(out))
        }
        Expr::Ident(name, span) => frame.lookup(name, *span),
        Expr::Select(base, field, span) => {
            let base = eval_expr(base, frame)?.concretize().clone();
            match base {
                Value::Struct(map) => map.get(field).cloned().ok_or_else(|| {
                    LangError::eval(
                        span.line,
                        span.col,
                        format!("field `{field}` not found in struct"),
                    )
                }),
                Value::Definition {
                    fields,
                    definitions,
                    optional,
                    ..
                } => {
                    if let Some(value) = fields
                        .get(field)
                        .or_else(|| definitions.get(field))
                        .cloned()
                    {
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
            let base = eval_expr(base, frame)?.concretize().clone();
            let index = eval_expr(index, frame)?.concretize().clone();
            match (base, index) {
                (Value::List(list), Value::Int(index)) => {
                    let index = usize::try_from(index).map_err(|_| {
                        LangError::eval(span.line, span.col, "list index must be non-negative")
                    })?;
                    list.get(index).cloned().ok_or_else(|| {
                        LangError::eval(span.line, span.col, "list index out of range")
                    })
                }
                (
                    Value::Struct(map) | Value::Definition { fields: map, .. },
                    Value::String(key),
                ) => map.get(&key).cloned().ok_or_else(|| {
                    LangError::eval(
                        span.line,
                        span.col,
                        format!("field `{key}` not found in struct"),
                    )
                }),
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
            require_concrete(&left, *span)?;
            require_concrete(&right, *span)?;
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
                    .map(|(key, value)| (Some(Value::String(key)), value))
                    .collect(),
                Value::List(values) => values
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| {
                        (
                            Some(Value::Int(i64::try_from(index).unwrap_or(i64::MAX))),
                            value,
                        )
                    })
                    .collect(),
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

fn collect_definition_references(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Ident(name, _) if name.starts_with('#') => out.push(name.clone()),
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            collect_definition_references(left, out);
            collect_definition_references(right, out);
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) => {
            collect_definition_references(inner, out);
        }
        _ => {}
    }
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
                names.extend(definitions.keys().cloned());
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
                names.extend(definitions.keys().cloned());
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

fn admitted(admissions: &[Admission], key: &str, span: Span) -> Result<bool, LangError> {
    if admissions.is_empty() {
        return Ok(true);
    }
    for admission in admissions {
        if admission.names.contains(key) {
            continue;
        }
        let mut matched = false;
        for pattern in &admission.patterns {
            if pattern_matches(pattern, key, span)? {
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
        Pattern::Regex(regex) => regex::Regex::new(regex)
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
        if let Some(constrained) = pattern_constraint(patterns, key, value, span)? {
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
        for (pattern, _) in patterns {
            pattern_matches(pattern, key, span)?;
        }
        if matches!(value, Value::Conflict(_)) {
            continue;
        }
        match pattern_constraint(patterns, key, value, span) {
            Ok(Some(constrained)) => *value = constrained,
            Ok(None) => {}
            Err(err) if err.to_string().contains("invalid pattern regex") => return Err(err),
            Err(err) => *value = Value::Conflict(err.to_string()),
        }
    }
    Ok(())
}

fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (
            Value::Struct(a) | Value::Definition { fields: a, .. },
            Value::Struct(b) | Value::Definition { fields: b, .. },
        ) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| values_equal(value, other)))
        }
        (Value::List(a), Value::List(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| values_equal(a, b))
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

fn go_case(text: &str, upper: bool) -> String {
    text.chars()
        .map(|character| {
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
        _ => return None,
    };
    Some(Value::Struct(
        entries
            .iter()
            .map(|(name, builtin)| ((*name).to_owned(), Value::Builtin(*builtin)))
            .collect(),
    ))
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
            let (Some(Value::Int(start)), Some(Value::Int(end)), Some(Value::Int(step))) =
                (arguments.first(), arguments.get(1), arguments.get(2))
            else {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "list.Range requires three integers",
                ));
            };
            if *step == 0 {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "list.Range step is zero",
                ));
            }
            let mut out = Vec::new();
            let mut current = *start;
            while (step > &0 && current < *end) || (step < &0 && current > *end) {
                out.push(Value::Int(current));
                current = current.saturating_add(*step);
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
            let Some(Value::List(outer)) = arguments.first() else {
                return Err(LangError::conflict(
                    span.line,
                    span.col,
                    "list.Concat requires a list of lists",
                ));
            };
            let mut values = Vec::new();
            for inner in outer {
                let Value::List(inner) = inner else {
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
                Some(Value::List(values)) => {
                    Ok(Value::Int(i64::try_from(values.len()).unwrap_or(i64::MAX)))
                }
                Some(Value::Struct(values) | Value::Definition { fields: values, .. }) => {
                    Ok(Value::Int(i64::try_from(values.len()).unwrap_or(i64::MAX)))
                }
                Some(Value::String(value)) => {
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
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
            (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{a}{b}"))),
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
        BinOp::Div => match (left, right) {
            (Value::Int(_), Value::Int(0)) => {
                Err(LangError::conflict(span.line, span.col, "division by zero"))
            }
            (Value::Int(a), Value::Int(b)) => Ok(Value::Float(*a as f64 / *b as f64)),
            _ => numeric(op, left, right, span, |a, b| a / b, |a, b| a / b),
        },
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
    float_op: impl Fn(f64, f64) -> f64,
) -> Result<Value, LangError> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(int_op(*a, *b))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_op(*a, *b))),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(float_op(*a as f64, *b))),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(float_op(*a, *b as f64))),
        _ => type_error(op_name(op), left, right, span),
    }
}

fn compare(op: &BinOp, left: &Value, right: &Value, span: Span) -> Result<Value, LangError> {
    let ordering = match (left, right) {
        (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Int(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
        (Value::Float(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
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
        (Value::Disjunction(a, da), Value::Disjunction(b, db)) => {
            unify_disjunctions(a, da, b, db, span)
        }
        (Value::Disjunction(a, da), other) | (other, Value::Disjunction(a, da)) => {
            unify_disjunction_with(a, da, other, span)
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
                    let value = pattern_constraint_closed(patterns, key, value, span)?
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
                    let value = pattern_constraint_closed(patterns_a, key, value, span)?
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
                        pattern_constraint_closed(patterns_b, key, value, span)?
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
                    match pattern_constraint_closed(patterns_a, key, &current, span) {
                        Ok(Some(constrained)) => current = constrained,
                        Ok(None) => {}
                        Err(err) if err.to_string().contains("invalid pattern regex") => {
                            return Err(err);
                        }
                        Err(err) => conflict = Some(err),
                    }
                }
                if conflict.is_none() && !optional_b_keys.contains(key) {
                    match pattern_constraint_closed(patterns_b, key, &current, span) {
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
                Ok(Value::Float(*a))
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
        (a, b) if values_equal(a, b) => Ok(a.clone()),
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
    }
}

fn kind_name(kind: TypeKind) -> &'static str {
    match kind {
        TypeKind::String => "string",
        TypeKind::Int => "int",
        TypeKind::Float => "float",
        TypeKind::Number => "number",
        TypeKind::Bool => "bool",
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
        _ => values_equal(left, right),
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
                Value::Float(number.as_f64().unwrap_or(0.0))
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
