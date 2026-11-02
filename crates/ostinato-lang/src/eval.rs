use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::{Map, Number, Value as Json};

use crate::ast::{BinOp, CompClause, Expr, File, Span, StrPart, StructItem, UnOp};
use crate::error::LangError;

/// Runtime value produced by the CUE-compatible evaluator.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    List(Vec<Value>),
    Struct(BTreeMap<String, Value>),
    /// A CUE definition: fields plus optional-field constraints. Open.
    Definition {
        fields: BTreeMap<String, Value>,
        optional: BTreeMap<String, Value>,
        definitions: BTreeMap<String, Value>,
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

    #[must_use]
    pub fn is_concrete(&self) -> bool {
        !matches!(self, Self::Type(_) | Self::Bottom | Self::Conflict(_))
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
                    }
                }
                StructItem::Let { name, value, .. } => {
                    frame.lets.borrow_mut().insert(name.clone(), value.clone());
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
                        self.insert_field(key, field.value.clone(), field.optional, false);
                    }
                }
                StructItem::For {
                    key,
                    value,
                    source,
                    body,
                    span,
                } => {
                    let source_value = eval_expr(source, self)?;
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
                    let condition = eval_expr(cond, self)?;
                    if condition.truthy() {
                        self.expand_body(body, HashMap::new())?;
                    } else if !condition.is_concrete() {
                        return Err(LangError::eval(
                            span.line,
                            span.col,
                            "`if` condition is not concrete",
                        ));
                    }
                }
                StructItem::Let { .. } | StructItem::Ellipsis(_) => {}
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
            self.generated_optionals.borrow_mut().remove(&key);
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
            merge_generated_optional(&self.generated_optionals, &name, value)?;
        }
        let generated: Vec<(String, Value)> = child
            .generated_optionals
            .borrow()
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        for (name, value) in generated {
            merge_generated_optional(&self.generated_optionals, &name, value)?;
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
        if let Some(value) = self.values.borrow().get(name) {
            return Ok(value.clone());
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
            let result = if let Some(items) = self.params_items(&exprs) {
                self.eval_definition_items(&items, span)?
            } else if exprs.len() == 1 {
                self.eval_definition_expr(&exprs[0], span)?
            } else {
                let mut result = Value::Bottom;
                for expr in &exprs {
                    let value = self.eval_definition_expr(expr, span)?;
                    result = if matches!(result, Value::Bottom) {
                        value
                    } else {
                        unify_values(&result, &value, span)?
                    };
                }
                result
            };
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
        let child = Frame::from_static(items, Some(self), HashMap::new())?;
        child.expand(items)?;
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
                definitions.insert(name.clone(), child.eval_field(&name, span)?);
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
                optional.insert(name, value);
            } else {
                let mut value = child.eval_field(&name, span)?;
                if let Some(constraint) = generated {
                    value = unify_values(&value, &constraint, span)?;
                }
                fields.insert(name, value);
            }
        }
        Ok(Value::Definition {
            fields,
            optional,
            definitions,
        })
    }

    fn eval_definition_expr(&self, expr: &Expr, span: Span) -> Result<Value, LangError> {
        if let Expr::Struct(items, struct_span) = expr {
            return self.eval_definition_items(items, *struct_span);
        }
        let value = eval_expr(expr, self)?;
        let _ = span;
        if let Value::Struct(fields) = value {
            return Ok(Value::Definition {
                fields,
                optional: BTreeMap::new(),
                definitions: BTreeMap::new(),
            });
        }
        Ok(value)
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
            Expr::Default(inner, _) => {
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
        let expr = self.lets.borrow().get(name).cloned();
        let Some(expr) = expr else {
            return Err(LangError::eval(
                span.line,
                span.col,
                format!("undefined let binding `{name}`"),
            ));
        };
        self.evaluating.borrow_mut().insert(name.to_owned());
        let value = eval_expr(&expr, self)?;
        self.evaluating.borrow_mut().remove(name);
        self.vars
            .borrow_mut()
            .insert(name.to_owned(), value.clone());
        Ok(value)
    }

    fn to_struct_value(&self) -> Result<Value, LangError> {
        self.to_struct_value_mode(false)
    }

    fn to_struct_value_mode(&self, include_definitions: bool) -> Result<Value, LangError> {
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
                    StrPart::Interp(expr) => out.push_str(&eval_expr(expr, frame)?.display()),
                }
            }
            Ok(Value::String(out))
        }
        Expr::Ident(name, span) => frame.lookup(name, *span),
        Expr::Select(base, field, span) => {
            let base = eval_expr(base, frame)?;
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
                    ..
                } => fields
                    .get(field)
                    .or_else(|| definitions.get(field))
                    .cloned()
                    .ok_or_else(|| {
                        LangError::eval(
                            span.line,
                            span.col,
                            format!("field `{field}` not found in struct"),
                        )
                    }),
                other => Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("cannot select `{field}` from {}", other.type_name()),
                )),
            }
        }
        Expr::Call(callee, arguments, span) => {
            let callee = eval_expr(callee, frame)?;
            let mut values = Vec::with_capacity(arguments.len());
            for argument in arguments {
                values.push(eval_expr(argument, frame)?);
            }
            apply_builtin(&callee, &values, *span)
        }
        Expr::Index(base, index, span) => {
            let base = eval_expr(base, frame)?;
            let index = eval_expr(index, frame)?;
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
        Expr::Struct(items, _) => Frame::new(items, Some(frame), HashMap::new())?.to_struct_value(),
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
            let mut alternatives = Vec::new();
            collect_disjunction(expr, &mut alternatives);
            if let Some(Expr::Default(inner, _)) = alternatives
                .iter()
                .find(|alternative| matches!(alternative, Expr::Default(_, _)))
            {
                return eval_expr(inner, frame);
            }
            let mut result: Option<Value> = None;
            for alternative in alternatives {
                let value = eval_expr(alternative, frame)?;
                result = Some(match result {
                    None => value,
                    Some(current) => combine_disjunction(&current, &value, *span)?,
                });
            }
            result.ok_or_else(|| LangError::eval(span.line, span.col, "empty disjunction"))
        }
        Expr::Default(inner, _) => eval_expr(inner, frame),
        Expr::Binary(op, left, right, span) => {
            if matches!(op, BinOp::And) {
                let left = eval_expr(left, frame)?;
                if !left.truthy() {
                    return Ok(Value::Bool(false));
                }
                return Ok(Value::Bool(eval_expr(right, frame)?.truthy()));
            }
            if matches!(op, BinOp::Or) {
                let left = eval_expr(left, frame)?;
                if left.truthy() {
                    return Ok(Value::Bool(true));
                }
                return Ok(Value::Bool(eval_expr(right, frame)?.truthy()));
            }
            let left = eval_expr(left, frame)?;
            let right = eval_expr(right, frame)?;
            binary(op, &left, &right, *span)
        }
        Expr::Unary(op, inner, span) => {
            let value = eval_expr(inner, frame)?;
            match (op, value) {
                (UnOp::Not, value) => Ok(Value::Bool(!value.truthy())),
                (UnOp::Neg, Value::Int(value)) => value
                    .checked_neg()
                    .map(Value::Int)
                    .ok_or_else(|| LangError::eval(span.line, span.col, "integer overflow")),
                (UnOp::Neg, Value::Float(value)) => Ok(Value::Float(-value)),
                (UnOp::Neg, other) => Err(LangError::eval(
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
            let source = eval_expr(source, frame)?;
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
        CompClause::If { cond, .. } => {
            if eval_expr(cond, frame)?.truthy() {
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
        return Err(LangError::eval(
            span.line,
            span.col,
            "attempted to call a non-function value",
        ));
    };
    match builtin {
        Builtin::StringsToUpper | Builtin::StringsToLower | Builtin::StringsTrimSpace => {
            if arguments.len() != 1 {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "string function requires exactly one argument",
                ));
            }
            let Some(Value::String(text)) = arguments.first() else {
                return Err(LangError::eval(
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
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "strings.Join requires exactly two arguments",
                ));
            }
            let (Some(Value::List(values)), Some(Value::String(separator))) =
                (arguments.first(), arguments.get(1))
            else {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "strings.Join requires a list and a separator",
                ));
            };
            let mut pieces = Vec::with_capacity(values.len());
            for value in values {
                let Value::String(text) = value else {
                    return Err(LangError::eval(
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
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "strings.Split requires exactly two arguments",
                ));
            }
            let (Some(Value::String(text)), Some(Value::String(separator))) =
                (arguments.first(), arguments.get(1))
            else {
                return Err(LangError::eval(
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
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "string predicate requires exactly two arguments",
                ));
            }
            let (Some(Value::String(text)), Some(Value::String(needle))) =
                (arguments.first(), arguments.get(1))
            else {
                return Err(LangError::eval(
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
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "list.Range requires exactly three arguments",
                ));
            }
            let (Some(Value::Int(start)), Some(Value::Int(end)), Some(Value::Int(step))) =
                (arguments.first(), arguments.get(1), arguments.get(2))
            else {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "list.Range requires three integers",
                ));
            };
            if *step == 0 {
                return Err(LangError::eval(
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
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "list.Concat requires exactly one list of lists",
                ));
            }
            let Some(Value::List(outer)) = arguments.first() else {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "list.Concat requires a list of lists",
                ));
            };
            let mut values = Vec::new();
            for inner in outer {
                let Value::List(inner) = inner else {
                    return Err(LangError::eval(
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
                return Err(LangError::eval(
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
                _ => Err(LangError::eval(
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
                Err(LangError::eval(span.line, span.col, "division by zero"))
            }
            (Value::Int(a), Value::Int(b)) => Ok(Value::Float(*a as f64 / *b as f64)),
            _ => numeric(op, left, right, span, |a, b| a / b, |a, b| a / b),
        },
        BinOp::Mod => match (left, right) {
            (Value::Int(_), Value::Int(0)) => {
                Err(LangError::eval(span.line, span.col, "modulo by zero"))
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
    LangError::eval(span.line, span.col, "integer overflow")
}

fn type_error(op: &str, left: &Value, right: &Value, span: Span) -> Result<Value, LangError> {
    Err(LangError::eval(
        span.line,
        span.col,
        format!("cannot {op} {} and {}", left.type_name(), right.type_name()),
    ))
}

fn unify_values(left: &Value, right: &Value, span: Span) -> Result<Value, LangError> {
    match (left, right) {
        (Value::Conflict(message), _) | (_, Value::Conflict(message)) => {
            Err(LangError::eval(span.line, span.col, message.clone()))
        }
        (Value::Bottom, value) | (value, Value::Bottom) => Ok(value.clone()),
        (Value::Type(a), Value::Type(b)) => unify_types(*a, *b, span),
        (Value::Type(kind), value) | (value, Value::Type(kind)) => {
            if type_matches(*kind, value) {
                Ok(value.clone())
            } else {
                Err(LangError::eval(
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
            },
            Value::Struct(b),
        )
        | (
            Value::Struct(b),
            Value::Definition {
                fields: a,
                optional: optional_a,
                definitions: definitions_a,
            },
        ) => {
            let mut fields = a.clone();
            let mut optional = optional_a.clone();
            for (key, value) in b {
                if let Some(existing) = fields.get(key).cloned() {
                    fields.insert(key.clone(), unify_values(&existing, value, span)?);
                } else if let Some(constraint) = optional.remove(key) {
                    fields.insert(key.clone(), unify_values(&constraint, value, span)?);
                } else {
                    fields.insert(key.clone(), value.clone());
                }
            }
            Ok(Value::Definition {
                fields,
                optional,
                definitions: definitions_a.clone(),
            })
        }
        (
            Value::Definition {
                fields: a,
                optional: optional_a,
                definitions: definitions_a,
            },
            Value::Definition {
                fields: b,
                optional: optional_b,
                definitions: definitions_b,
            },
        ) => {
            let mut fields = a.clone();
            let mut optional = optional_a.clone();
            for (key, value) in b {
                if let Some(existing) = fields.get(key).cloned() {
                    fields.insert(key.clone(), unify_values(&existing, value, span)?);
                } else if let Some(constraint) = optional.remove(key) {
                    // `b` makes the field required; `a`'s optional constraint
                    // still applies.
                    fields.insert(key.clone(), unify_values(&constraint, value, span)?);
                } else {
                    fields.insert(key.clone(), value.clone());
                }
            }
            for (key, value) in optional_b {
                if let Some(existing) = fields.get(key).cloned() {
                    // `a` makes the field required; `b`'s optional constraint
                    // still applies.
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
            let mut definitions = definitions_a.clone();
            for (key, value) in definitions_b {
                if let Some(existing) = definitions.get(key).cloned() {
                    definitions.insert(key.clone(), unify_values(&existing, value, span)?);
                } else {
                    definitions.insert(key.clone(), value.clone());
                }
            }
            Ok(Value::Definition {
                fields,
                optional,
                definitions,
            })
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
        (a, b) => Err(LangError::eval(
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
            return Err(LangError::eval(
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
    let mut alternatives = Vec::new();
    collect_disjunction(expr, &mut alternatives);
    if alternatives.len() == 1 {
        let declared = eval_expr(expr, frame)?;
        return unify_values(&declared, override_value, span);
    }
    // Prefer non-default alternatives so an explicit override can select a
    // branch that differs from the default.
    let mut ordered: Vec<&Expr> = alternatives
        .iter()
        .copied()
        .filter(|alternative| !matches!(alternative, Expr::Default(_, _)))
        .collect();
    ordered.extend(
        alternatives
            .iter()
            .copied()
            .filter(|alternative| matches!(alternative, Expr::Default(_, _))),
    );
    let mut last_error = None;
    for alternative in ordered {
        let candidate = match alternative {
            Expr::Default(inner, _) => eval_expr(inner, frame),
            other => eval_expr(other, frame),
        };
        match candidate.and_then(|candidate| unify_values(&candidate, override_value, span)) {
            Ok(value) => return Ok(value),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        LangError::eval(
            span.line,
            span.col,
            "no parameter alternative matched the override",
        )
    }))
}

fn collect_disjunction<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::Disjunction(left, right, _) = expr {
        collect_disjunction(left, out);
        collect_disjunction(right, out);
    } else {
        out.push(expr);
    }
}

fn combine_disjunction(left: &Value, right: &Value, span: Span) -> Result<Value, LangError> {
    match (left, right) {
        (Value::Conflict(message), _) | (_, Value::Conflict(message)) => {
            Err(LangError::eval(span.line, span.col, message.clone()))
        }
        (Value::Bottom, value) | (value, Value::Bottom) => Ok(value.clone()),
        (Value::Definition { .. }, Value::Definition { .. })
        | (Value::Definition { .. }, Value::Struct(_))
        | (Value::Struct(_), Value::Definition { .. }) => unify_values(left, right, span),
        (Value::Type(a), Value::Type(b)) => unify_types(*a, *b, span),
        (Value::Type(kind), value) | (value, Value::Type(kind)) => {
            if type_matches(*kind, value) {
                Ok(value.clone())
            } else {
                Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("disjunction has no value satisfying {}", kind_name(*kind)),
                ))
            }
        }
        (a, b) if a == b => Ok(a.clone()),
        _ => Err(LangError::eval(
            span.line,
            span.col,
            "disjunction is not concrete; add a default with `*`",
        )),
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
