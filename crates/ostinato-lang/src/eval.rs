use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{Map, Number, Value as Json};

use crate::ast::{BinOp, Expr, File, Span, StrPart, StructItem, UnOp};
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
    /// A type constraint (`string`, `int`, `number`, ...).
    Type(TypeKind),
    /// The top value `_` (any). Also used for incomplete values.
    Bottom,
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
            Self::Struct(_) => "struct",
            Self::Type(kind) => match kind {
                TypeKind::String => "string",
                TypeKind::Int => "int",
                TypeKind::Float => "float",
                TypeKind::Number => "number",
                TypeKind::Bool => "bool",
            },
            Self::Bottom => "_",
        }
    }

    #[must_use]
    pub fn is_concrete(&self) -> bool {
        !matches!(self, Self::Type(_) | Self::Bottom)
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
            Self::Struct(values) => {
                let mut out = Map::new();
                for (key, value) in values {
                    out.insert(key.clone(), value.to_json()?);
                }
                Some(Json::Object(out))
            }
            Self::Type(_) | Self::Bottom => None,
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
            Self::Struct(values) => !values.is_empty(),
            Self::Type(_) | Self::Bottom => false,
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
            Self::Bottom => "_".to_owned(),
            Self::List(_) | Self::Struct(_) => self.to_json().map_or_else(
                || format!("<{}>", self.type_name()),
                |json| json.to_string(),
            ),
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
    let frame = Frame::new(&file.items, None, HashMap::new())?;
    if !overrides.is_empty() {
        let Value::Struct(mut params) = frame.eval_field("params", Span::default())? else {
            return Err(LangError::eval(
                1,
                1,
                "parameters were supplied but `params` is not a struct",
            ));
        };
        for (key, value) in overrides {
            params.insert(key.clone(), value.clone());
        }
        frame
            .values
            .borrow_mut()
            .insert("params".to_owned(), Value::Struct(params));
    }
    frame.to_struct_value()
}

fn json_frame(context: &Json) -> Frame<'static> {
    let frame = Frame {
        parent: None,
        vars: HashMap::new(),
        fields: RefCell::new(BTreeMap::new()),
        values: RefCell::new(BTreeMap::new()),
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
}

struct Frame<'a> {
    parent: Option<&'a Frame<'a>>,
    vars: HashMap<String, Value>,
    fields: RefCell<BTreeMap<String, FieldDef>>,
    values: RefCell<BTreeMap<String, Value>>,
    evaluating: RefCell<HashSet<String>>,
}

impl<'a> Frame<'a> {
    fn new(
        items: &[StructItem],
        parent: Option<&'a Frame<'a>>,
        vars: HashMap<String, Value>,
    ) -> Result<Self, LangError> {
        let frame = Self {
            parent,
            vars,
            fields: RefCell::new(BTreeMap::new()),
            values: RefCell::new(BTreeMap::new()),
            evaluating: RefCell::new(HashSet::new()),
        };

        // Pass 1: static fields.
        for item in items {
            if let StructItem::Field(field) = item
                && let crate::ast::Label::Name(name, _) = &field.label
            {
                frame.insert_field(name.clone(), field.value.clone(), field.optional);
            }
        }

        // Pass 2: dynamic labels, comprehensions and conditionals.
        for item in items {
            match item {
                StructItem::Field(field) => {
                    if let crate::ast::Label::Dynamic(expr, span) = &field.label {
                        let key = eval_expr(expr, &frame)?;
                        let Value::String(key) = key else {
                            return Err(LangError::eval(
                                span.line,
                                span.col,
                                "dynamic field label must evaluate to a string",
                            ));
                        };
                        frame.insert_field(key, field.value.clone(), field.optional);
                    }
                }
                StructItem::For {
                    key,
                    value,
                    source,
                    body,
                    span,
                } => {
                    let source_value = eval_expr(source, &frame)?;
                    match source_value {
                        Value::Struct(map) => {
                            for (entry_key, entry_value) in map {
                                let mut bindings = HashMap::new();
                                if let Some(key) = key {
                                    bindings.insert(key.clone(), Value::String(entry_key));
                                }
                                bindings.insert(value.clone(), entry_value);
                                frame.expand_body(body, bindings)?;
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
                                frame.expand_body(body, bindings)?;
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
                    let condition = eval_expr(cond, &frame)?;
                    if condition.truthy() {
                        frame.expand_body(body, HashMap::new())?;
                    } else if !condition.is_concrete() {
                        return Err(LangError::eval(
                            span.line,
                            span.col,
                            "`if` condition is not concrete",
                        ));
                    }
                }
                StructItem::Ellipsis(_) => {}
            }
        }

        Ok(frame)
    }

    fn insert_field(&self, name: String, expr: Expr, optional: bool) {
        let mut fields = self.fields.borrow_mut();
        fields
            .entry(name)
            .and_modify(|def| {
                def.exprs.push(expr.clone());
                def.optional = def.optional && optional;
            })
            .or_insert(FieldDef {
                exprs: vec![expr],
                optional,
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
        let mut current = self.values.borrow_mut();
        for (key, value) in values {
            if let Some(existing) = current.get(&key).cloned() {
                let unified = unify_values(&existing, &value, Span::default())?;
                current.insert(key, unified);
            } else {
                current.insert(key, value);
            }
        }
        Ok(())
    }

    fn lookup(&self, name: &str, span: Span) -> Result<Value, LangError> {
        if let Some(value) = self.vars.get(name) {
            return Ok(value.clone());
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
            || self.vars.contains_key(name)
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
        let exprs = {
            let fields = self.fields.borrow();
            let Some(def) = fields.get(name) else {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("undefined reference `{name}`"),
                ));
            };
            def.exprs.clone()
        };
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

    fn to_struct_value(&self) -> Result<Value, LangError> {
        let names: Vec<String> = {
            let fields = self.fields.borrow();
            fields
                .keys()
                .filter(|name| !name.starts_with('#'))
                .cloned()
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
            if !name.starts_with('#') {
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
                other => Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("cannot select `{field}` from {}", other.type_name()),
                )),
            }
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
                (Value::Struct(map), Value::String(key)) => {
                    map.get(&key).cloned().ok_or_else(|| {
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
        Expr::Struct(items, _) => Frame::new(items, Some(frame), HashMap::new())?.to_struct_value(),
        Expr::List(items, _) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(eval_expr(item, frame)?);
            }
            Ok(Value::List(out))
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
        BinOp::Eq => Ok(Value::Bool(left == right)),
        BinOp::Ne => Ok(Value::Bool(left != right)),
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
        (a, b) if a == b => Ok(a.clone()),
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
        (Value::Bottom, value) | (value, Value::Bottom) => Ok(value.clone()),
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
