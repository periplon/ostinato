use std::collections::{HashMap, HashSet};

use bigdecimal::BigDecimal;

use crate::ast::{BinOp, CompClause, Expr, File, Label, Span, StrPart, StructItem, UnOp};
use crate::error::LangError;

/// Rewrite file-local imports, `let` bindings and aliases to unique names.
///
/// # Errors
/// Returns an error for invalid alias declarations (duplicates, field
/// conflicts, unreferenced aliases).
pub fn rename_file_locals(file: &mut File, file_index: usize) -> Result<(), LangError> {
    let import_aliases: Vec<String> = file
        .imports
        .iter()
        .map(|import| import.alias.clone())
        .collect();
    validate_aliases(&file.items, &import_aliases)?;
    let prefix = format!("__ost_f{file_index}_");
    let mut locals = HashMap::new();
    for import in &file.imports {
        locals.insert(
            import.alias.clone(),
            format!("{prefix}import_{}", import.alias),
        );
    }
    for item in &file.items {
        match item {
            StructItem::Let { name, .. } => {
                locals.insert(name.clone(), format!("{prefix}let_{name}"));
            }
            StructItem::Field(field) => {
                if let Some(alias) = &field.alias {
                    locals.insert(alias.clone(), format!("{prefix}alias_{alias}"));
                }
            }
            _ => {}
        }
    }
    if locals.is_empty() {
        return Ok(());
    }
    for import in &mut file.imports {
        if let Some(renamed) = locals.get(&import.alias) {
            import.alias.clone_from(renamed);
        }
    }
    let mut shadowed = HashSet::new();
    rename_items(&mut file.items, &locals, &mut shadowed);
    Ok(())
}

fn field_node(name: &str) -> String {
    format!("\u{2}{name}")
}

fn is_field_node(name: &str) -> bool {
    name.starts_with('\u{2}')
}

#[derive(Debug, Clone)]
enum Scalar {
    Int(i64),
    Float(BigDecimal),
    String(String),
    Bool(bool),
}

impl Scalar {
    fn compare(&self, op: BinOp, other: &Self) -> Option<bool> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Some(match op {
                BinOp::Eq => a == b,
                BinOp::Ne => a != b,
                BinOp::Lt => a < b,
                BinOp::Le => a <= b,
                BinOp::Gt => a > b,
                BinOp::Ge => a >= b,
                _ => return None,
            }),
            (Self::Float(a), Self::Float(b)) => Some(match op {
                BinOp::Eq => a == b,
                BinOp::Ne => a != b,
                BinOp::Lt => a < b,
                BinOp::Le => a <= b,
                BinOp::Gt => a > b,
                BinOp::Ge => a >= b,
                _ => return None,
            }),
            (Self::String(a), Self::String(b)) => Some(match op {
                BinOp::Eq => a == b,
                BinOp::Ne => a != b,
                BinOp::Lt => a < b,
                BinOp::Le => a <= b,
                BinOp::Gt => a > b,
                BinOp::Ge => a >= b,
                _ => return None,
            }),
            (Self::Bool(a), Self::Bool(b)) => match op {
                BinOp::Eq => Some(a == b),
                BinOp::Ne => Some(a != b),
                _ => None,
            },
            (Self::Int(a), Self::Float(b)) => {
                let a = BigDecimal::from(*a);
                Some(match op {
                    BinOp::Eq => a == *b,
                    BinOp::Ne => a != *b,
                    BinOp::Lt => a < *b,
                    BinOp::Le => a <= *b,
                    BinOp::Gt => a > *b,
                    BinOp::Ge => a >= *b,
                    _ => return None,
                })
            }
            (Self::Float(a), Self::Int(b)) => {
                let b = BigDecimal::from(*b);
                Some(match op {
                    BinOp::Eq => *a == b,
                    BinOp::Ne => *a != b,
                    BinOp::Lt => *a < b,
                    BinOp::Le => *a <= b,
                    BinOp::Gt => *a > b,
                    BinOp::Ge => *a >= b,
                    _ => return None,
                })
            }
            _ => None,
        }
    }
}

/// The identifier bindings a struct literal introduces for its own fields.
///
/// Static constant folding walks expressions before the full alias scope is
/// built, so it needs this lightweight environment to honour shadowing by the
/// literal's `let`s, required fields and aliases.
#[derive(Debug, Default, Clone)]
struct LocalEnv<'a> {
    lets: HashMap<&'a str, &'a Expr>,
    shadows: HashSet<&'a str>,
    parent: Option<Box<LocalEnv<'a>>>,
}

enum LocalClass<'s, 'a> {
    Let(&'s LocalEnv<'a>, &'a Expr),
    Shadow,
    Unbound,
}

impl<'a> LocalEnv<'a> {
    fn from_items(items: &'a [StructItem], parent: Option<&'a LocalEnv<'a>>) -> Self {
        let mut env = Self {
            parent: parent.cloned().map(Box::new),
            ..Self::default()
        };
        for item in items {
            match item {
                StructItem::Let { name, value, .. } => {
                    env.lets.insert(name, value);
                }
                StructItem::Field(field) => {
                    if let Some(alias) = &field.alias {
                        env.shadows.insert(alias);
                    }
                    if let Label::Name(name, _) = &field.label
                        && !field.quoted
                        && !field.optional
                    {
                        env.shadows.insert(name);
                    }
                }
                _ => {}
            }
        }
        env
    }

    fn classify<'s>(&'s self, name: &str) -> LocalClass<'s, 'a> {
        if let Some(value) = self.lets.get(name) {
            return LocalClass::Let(self, value);
        }
        if self.shadows.contains(name) {
            return LocalClass::Shadow;
        }
        match &self.parent {
            Some(parent) => parent.classify(name),
            None => LocalClass::Unbound,
        }
    }
}

#[derive(Debug)]
struct AliasScope {
    parent: Option<usize>,
    fields: HashSet<String>,
    required_fields: HashSet<String>,
    lets: HashSet<String>,
    let_values: HashMap<String, Expr>,
    let_spans: HashMap<String, Span>,
    let_used: HashSet<String>,
    bindings: HashSet<String>,
    binding_spans: Vec<(String, Span)>,
    aliases: HashMap<String, usize>,
}

#[derive(Debug)]
struct AliasInfo {
    name: String,
    target: Option<String>,
    scope: usize,
    span: Span,
    used: bool,
}

struct AliasCtx {
    scopes: Vec<AliasScope>,
    aliases: Vec<AliasInfo>,
    let_edges: Vec<((usize, String), (usize, String))>,
}

impl AliasCtx {
    fn is_ancestor(&self, ancestor: usize, scope: usize) -> bool {
        let mut current = Some(scope);
        while let Some(index) = current {
            if index == ancestor {
                return true;
            }
            current = self.scopes[index].parent;
        }
        false
    }

    fn const_scalar(
        &self,
        expr: &Expr,
        chain: &[usize],
        local: Option<&LocalEnv<'_>>,
        depth: usize,
    ) -> Option<Scalar> {
        if depth > 32 {
            return None;
        }
        match expr {
            Expr::Int(value, _) => Some(Scalar::Int(*value)),
            Expr::Float(value, _) => Some(Scalar::Float(value.clone())),
            Expr::Bool(value, _) => Some(Scalar::Bool(*value)),
            Expr::Paren(inner, _) => self.const_scalar(inner, chain, local, depth + 1),
            Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
                StrPart::Text(text) => Some(Scalar::String(text.clone())),
                StrPart::Interp(_) => None,
            },
            Expr::Unary(UnOp::Neg, inner, _) => {
                match self.const_scalar(inner, chain, local, depth + 1)? {
                    Scalar::Int(value) => Some(Scalar::Int(-value)),
                    Scalar::Float(value) => Some(Scalar::Float(-value)),
                    _ => None,
                }
            }
            Expr::Ident(name, _) => {
                if let Some(local) = local {
                    match local.classify(name) {
                        LocalClass::Let(env, value) => {
                            return self.const_scalar(value, chain, Some(env), depth + 1);
                        }
                        LocalClass::Shadow => return None,
                        LocalClass::Unbound => {}
                    }
                }
                for scope in chain.iter().rev() {
                    if let Some(value) = self.scopes[*scope].let_values.get(name) {
                        return self.const_scalar(value, chain, local, depth + 1);
                    }
                    if self.scopes[*scope].required_fields.contains(name)
                        || self.scopes[*scope].aliases.contains_key(name)
                    {
                        return None;
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn const_bool(
        &self,
        expr: &Expr,
        chain: &[usize],
        local: Option<&LocalEnv<'_>>,
        depth: usize,
    ) -> Option<bool> {
        if depth > 32 {
            return None;
        }
        match expr {
            Expr::Bool(value, _) => Some(*value),
            Expr::Paren(inner, _) => self.const_bool(inner, chain, local, depth + 1),
            Expr::Unary(UnOp::Not, inner, _) => self
                .const_bool(inner, chain, local, depth + 1)
                .map(|value| !value),
            Expr::Ident(name, _) => {
                if let Some(local) = local {
                    match local.classify(name) {
                        LocalClass::Let(env, value) => {
                            return self.const_bool(value, chain, Some(env), depth + 1);
                        }
                        LocalClass::Shadow => return None,
                        LocalClass::Unbound => {}
                    }
                }
                for scope in chain.iter().rev() {
                    if let Some(value) = self.scopes[*scope].let_values.get(name) {
                        return self.const_bool(value, chain, local, depth + 1);
                    }
                    if self.scopes[*scope].required_fields.contains(name)
                        || self.scopes[*scope].aliases.contains_key(name)
                    {
                        return None;
                    }
                }
                None
            }
            Expr::Binary(op, left, right, _) => match op {
                BinOp::And => Some(
                    self.const_bool(left, chain, local, depth + 1)?
                        && self.const_bool(right, chain, local, depth + 1)?,
                ),
                BinOp::Or => Some(
                    self.const_bool(left, chain, local, depth + 1)?
                        || self.const_bool(right, chain, local, depth + 1)?,
                ),
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    let left = self.const_scalar(left, chain, local, depth + 1)?;
                    let right = self.const_scalar(right, chain, local, depth + 1)?;
                    left.compare(*op, &right)
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn static_non_empty(
        &self,
        expr: &Expr,
        chain: &[usize],
        local: Option<&LocalEnv<'_>>,
        depth: usize,
    ) -> bool {
        if depth > 32 {
            return false;
        }
        match expr {
            Expr::List(items, _) => !items.is_empty(),
            Expr::Struct(items, _) => {
                let env = LocalEnv::from_items(items, local);
                self.struct_has_required(items, chain, Some(&env), depth + 1)
            }
            Expr::ListComp { clauses, .. } => clauses.iter().all(|clause| match clause {
                CompClause::For { source, .. } => {
                    self.static_non_empty(source, chain, local, depth + 1)
                }
                CompClause::If { cond, .. } => {
                    self.const_bool(cond, chain, local, depth + 1) == Some(true)
                }
            }),
            Expr::Unify(left, right, _) => {
                self.static_non_empty(left, chain, local, depth + 1)
                    || self.static_non_empty(right, chain, local, depth + 1)
            }
            Expr::Paren(inner, _) => self.static_non_empty(inner, chain, local, depth + 1),
            Expr::Ident(name, _) => {
                if let Some(local) = local {
                    match local.classify(name) {
                        LocalClass::Let(env, value) => {
                            return self.static_non_empty(value, chain, Some(env), depth + 1);
                        }
                        LocalClass::Shadow => return false,
                        LocalClass::Unbound => {}
                    }
                }
                for scope in chain.iter().rev() {
                    if let Some(value) = self.scopes[*scope].let_values.get(name) {
                        return self.static_non_empty(value, chain, local, depth + 1);
                    }
                    if self.scopes[*scope].required_fields.contains(name)
                        || self.scopes[*scope].aliases.contains_key(name)
                    {
                        return false;
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn struct_has_required(
        &self,
        items: &[StructItem],
        chain: &[usize],
        local: Option<&LocalEnv<'_>>,
        depth: usize,
    ) -> bool {
        // Body-local bindings shadow the enclosing literal's bindings.
        for item in items {
            match item {
                StructItem::Field(field) => {
                    if !field.definition && !field.optional {
                        return true;
                    }
                }
                StructItem::If { cond, body, .. } => {
                    let env = LocalEnv::from_items(body, local);
                    if self.const_bool(cond, chain, local, depth + 1) == Some(true)
                        && self.struct_has_required(body, chain, Some(&env), depth + 1)
                    {
                        return true;
                    }
                }
                StructItem::For { source, body, .. } => {
                    let env = LocalEnv::from_items(body, local);
                    if self.static_non_empty(source, chain, local, depth + 1)
                        && self.struct_has_required(body, chain, Some(&env), depth + 1)
                    {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn scope(
        &mut self,
        items: &[StructItem],
        parent: Option<usize>,
        visible: &[usize],
        shadowed: &HashSet<String>,
        from: Option<&(usize, String)>,
    ) -> Result<usize, LangError> {
        let id = self.scopes.len();
        self.scopes.push(AliasScope {
            parent,
            fields: HashSet::new(),
            required_fields: HashSet::new(),
            lets: HashSet::new(),
            let_values: HashMap::new(),
            let_spans: HashMap::new(),
            let_used: HashSet::new(),
            bindings: HashSet::new(),
            binding_spans: Vec::new(),
            aliases: HashMap::new(),
        });

        for item in items {
            match item {
                StructItem::Field(field) => {
                    if let Label::Name(name, _) = &field.label
                        && !field.quoted
                    {
                        self.scopes[id].fields.insert(name.clone());
                        if !field.optional {
                            self.scopes[id].required_fields.insert(name.clone());
                            self.scopes[id]
                                .let_spans
                                .insert(field_node(name), field.span);
                        }
                    }
                    if let Some(alias) = &field.alias {
                        if alias == "_" {
                            return Err(LangError::eval(
                                field.span.line,
                                field.span.col,
                                "cannot use _ as alias or let clause",
                            ));
                        }
                        if self.scopes[id].aliases.contains_key(alias) {
                            return Err(LangError::eval(
                                field.span.line,
                                field.span.col,
                                format!("alias `{alias}` redeclared in same scope"),
                            ));
                        }
                        let alias_id = self.aliases.len();
                        let target = match &field.label {
                            Label::Name(name, _) => Some(name.clone()),
                            Label::Dynamic(_, _) => None,
                        };
                        self.aliases.push(AliasInfo {
                            name: alias.clone(),
                            target,
                            scope: id,
                            span: field.span,
                            used: false,
                        });
                        self.scopes[id].aliases.insert(alias.clone(), alias_id);
                    }
                }
                StructItem::Let {
                    name, value, span, ..
                } => {
                    if name == "_" {
                        return Err(LangError::eval(
                            span.line,
                            span.col,
                            "cannot use _ as alias or let clause",
                        ));
                    }
                    if !self.scopes[id].lets.insert(name.clone()) {
                        return Err(LangError::eval(
                            span.line,
                            span.col,
                            format!("alias `{name}` redeclared in same scope"),
                        ));
                    }
                    self.scopes[id].let_spans.insert(name.clone(), *span);
                    self.scopes[id]
                        .let_values
                        .insert(name.clone(), value.clone());
                }
                _ => {}
            }
        }

        let mut chain = visible.to_vec();
        chain.push(id);
        let local_shadow = shadowed.clone();

        for item in items {
            match item {
                StructItem::Field(field) => {
                    if let Label::Dynamic(expr, _) = &field.label {
                        self.expr(expr, &chain, &local_shadow, None)?;
                        self.expr(&field.value, &chain, &local_shadow, None)?;
                        continue;
                    }
                    let required = if let Label::Name(name, _) = &field.label {
                        !field.quoted && !field.optional && {
                            let node = (id, field_node(name));
                            if let Some(from) = from {
                                let edge = (from.clone(), node.clone());
                                if !self.let_edges.contains(&edge) {
                                    self.let_edges.push(edge);
                                }
                            }
                            true
                        }
                    } else {
                        false
                    };
                    let field_from = if required {
                        if let Label::Name(name, _) = &field.label {
                            Some((id, field_node(name)))
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    self.expr(&field.value, &chain, &local_shadow, field_from.as_ref())?;
                }
                StructItem::Let { name, value, .. } => {
                    // A `let` is not in scope in its own value; a same-named
                    // identifier resolves to an enclosing let or alias.
                    let hidden = self.scopes[id].lets.remove(name);
                    let from = (id, name.clone());
                    let result = self.expr(value, &chain, &local_shadow, Some(&from));
                    if hidden {
                        self.scopes[id].lets.insert(name.clone());
                    }
                    result?;
                }
                StructItem::Pattern { value, .. } => {
                    self.expr(value, &chain, &local_shadow, from)?;
                }
                StructItem::For {
                    key,
                    value,
                    source,
                    body,
                    span,
                } => {
                    self.expr(source, &chain, &local_shadow, from)?;
                    let mut body_shadow = local_shadow.clone();
                    if let Some(key) = key {
                        body_shadow.insert(key.clone());
                    }
                    body_shadow.insert(value.clone());
                    let body_from = if self.static_non_empty(source, &chain, None, 0) {
                        from
                    } else {
                        None
                    };
                    let body_id = self.scope(body, Some(id), &chain, &body_shadow, body_from)?;
                    if let Some(key) = key {
                        self.scopes[body_id].bindings.insert(key.clone());
                        self.scopes[body_id]
                            .binding_spans
                            .push((key.clone(), *span));
                    }
                    self.scopes[body_id].bindings.insert(value.clone());
                    self.scopes[body_id]
                        .binding_spans
                        .push((value.clone(), *span));
                }
                StructItem::If { cond, body, .. } => {
                    self.expr(cond, &chain, &local_shadow, from)?;
                    let body_from = if self.const_bool(cond, &chain, None, 0) == Some(true) {
                        from
                    } else {
                        None
                    };
                    self.scope(body, Some(id), &chain, &local_shadow, body_from)?;
                }
                StructItem::Ellipsis(_) => {}
            }
        }
        Ok(id)
    }

    fn expr(
        &mut self,
        expr: &Expr,
        chain: &[usize],
        shadowed: &HashSet<String>,
        from: Option<&(usize, String)>,
    ) -> Result<(), LangError> {
        match expr {
            Expr::Ident(name, _) => {
                if shadowed.contains(name) {
                    return Ok(());
                }
                for scope in chain.iter().rev() {
                    if self.scopes[*scope].lets.contains(name) {
                        self.scopes[*scope].let_used.insert(name.clone());
                        if let Some(from) = from {
                            let edge = (from.clone(), (*scope, name.clone()));
                            if !self.let_edges.contains(&edge) {
                                self.let_edges.push(edge);
                            }
                        }
                        return Ok(());
                    }
                    if self.scopes[*scope].required_fields.contains(name) {
                        if let Some(from) = from {
                            let edge = (from.clone(), (*scope, field_node(name)));
                            if !self.let_edges.contains(&edge) {
                                self.let_edges.push(edge);
                            }
                        }
                        return Ok(());
                    }
                    if let Some(alias) = self.scopes[*scope].aliases.get(name) {
                        let alias = *alias;
                        self.aliases[alias].used = true;
                        if let Some(from) = from
                            && let Some(target) = self.aliases[alias].target.clone()
                        {
                            let alias_scope = self.aliases[alias].scope;
                            if self.scopes[alias_scope].required_fields.contains(&target) {
                                let edge = (from.clone(), (alias_scope, field_node(&target)));
                                if !self.let_edges.contains(&edge) {
                                    self.let_edges.push(edge);
                                }
                            }
                        }
                        return Ok(());
                    }
                }
            }
            Expr::Paren(inner, _) | Expr::Default(inner, _) => {
                self.expr(inner, chain, shadowed, from)?;
            }
            Expr::Bound(_, inner, _) => self.expr(inner, chain, shadowed, from)?,
            Expr::Select(base, _, _) => {
                if !matches!(&**base, Expr::Ident(name, _) if name == "params") {
                    self.expr(base, chain, shadowed, from)?;
                }
            }
            Expr::Index(base, index, _) => {
                self.expr(base, chain, shadowed, from)?;
                self.expr(index, chain, shadowed, from)?;
            }
            Expr::Call(callee, arguments, _) => {
                self.expr(callee, chain, shadowed, from)?;
                for argument in arguments {
                    self.expr(argument, chain, shadowed, from)?;
                }
            }
            Expr::Struct(items, _) => {
                let parent = chain.last().copied();
                self.scope(items, parent, chain, shadowed, from)?;
            }
            Expr::List(items, _) => {
                for item in items {
                    self.expr(item, chain, shadowed, from)?;
                }
            }
            Expr::OpenList(element, _) => {
                if let Some(element) = element {
                    self.expr(element, chain, shadowed, from)?;
                }
            }
            Expr::OpenListPrefix(prefix, tail, _) => {
                for item in prefix {
                    self.expr(item, chain, shadowed, from)?;
                }
                if let Some(tail) = tail {
                    self.expr(tail, chain, shadowed, from)?;
                }
            }
            Expr::ListComp { clauses, body, .. } => {
                let comp_id = self.scopes.len();
                self.scopes.push(AliasScope {
                    parent: chain.last().copied(),
                    fields: HashSet::new(),
                    required_fields: HashSet::new(),
                    lets: HashSet::new(),
                    let_values: HashMap::new(),
                    let_spans: HashMap::new(),
                    let_used: HashSet::new(),
                    bindings: HashSet::new(),
                    binding_spans: Vec::new(),
                    aliases: HashMap::new(),
                });
                let mut comp_chain = chain.to_vec();
                comp_chain.push(comp_id);
                let mut body_shadow = shadowed.clone();
                let mut may_run = true;
                for clause in clauses {
                    match clause {
                        CompClause::For {
                            key,
                            value,
                            source,
                            span,
                        } => {
                            self.expr(source, chain, shadowed, if may_run { from } else { None })?;
                            may_run = may_run && self.static_non_empty(source, chain, None, 0);
                            if let Some(key) = key {
                                body_shadow.insert(key.clone());
                                self.scopes[comp_id].bindings.insert(key.clone());
                                self.scopes[comp_id]
                                    .binding_spans
                                    .push((key.clone(), *span));
                            }
                            body_shadow.insert(value.clone());
                            self.scopes[comp_id].bindings.insert(value.clone());
                            self.scopes[comp_id]
                                .binding_spans
                                .push((value.clone(), *span));
                        }
                        CompClause::If { cond, .. } => {
                            self.expr(
                                cond,
                                &comp_chain,
                                &body_shadow,
                                if may_run { from } else { None },
                            )?;
                            may_run = may_run
                                && self.const_bool(cond, &comp_chain, None, 0) == Some(true);
                        }
                    }
                }
                self.expr(
                    body,
                    &comp_chain,
                    &body_shadow,
                    if may_run { from } else { None },
                )?;
            }
            Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
                self.expr(left, chain, shadowed, from)?;
                self.expr(right, chain, shadowed, from)?;
            }
            Expr::Binary(_, left, right, _) => {
                self.expr(left, chain, shadowed, from)?;
                self.expr(right, chain, shadowed, from)?;
            }
            Expr::Unary(_, inner, _) => self.expr(inner, chain, shadowed, from)?,
            Expr::Str(parts, _) => {
                for part in parts {
                    if let StrPart::Interp(inner) = part {
                        self.expr(inner, chain, shadowed, from)?;
                    }
                }
            }
            Expr::Null(_)
            | Expr::Bool(_, _)
            | Expr::Int(_, _)
            | Expr::Float(_, _)
            | Expr::Bytes(_, _) => {}
        }
        Ok(())
    }
}

/// Validate alias declarations, usage and alias/field conflicts.
///
/// # Errors
/// Returns an evaluation error for duplicate aliases, alias/field conflicts in
/// enclosing scopes, and unreferenced aliases.
pub fn validate_aliases(items: &[StructItem], import_aliases: &[String]) -> Result<(), LangError> {
    let mut ctx = AliasCtx {
        scopes: Vec::new(),
        aliases: Vec::new(),
        let_edges: Vec::new(),
    };
    let root = ctx.scope(items, None, &[], &HashSet::new(), None)?;
    detect_let_cycles(&ctx)?;
    for alias in import_aliases {
        ctx.scopes[root].bindings.insert(alias.clone());
    }

    for alias in &ctx.aliases {
        if ctx.scopes[alias.scope].lets.contains(&alias.name) {
            return Err(LangError::eval(
                alias.span.line,
                alias.span.col,
                format!(
                    "cannot have both alias and field with name `{}` in the same scope",
                    alias.name
                ),
            ));
        }
        for (scope_id, scope) in ctx.scopes.iter().enumerate() {
            if (scope.fields.contains(&alias.name) || scope.bindings.contains(&alias.name))
                && (ctx.is_ancestor(alias.scope, scope_id)
                    || ctx.is_ancestor(scope_id, alias.scope))
            {
                return Err(LangError::eval(
                    alias.span.line,
                    alias.span.col,
                    format!(
                        "cannot have both alias and field with name `{}` in the same scope",
                        alias.name
                    ),
                ));
            }
        }
        if !alias.used {
            return Err(LangError::eval(
                alias.span.line,
                alias.span.col,
                format!("unreferenced alias `{}`", alias.name),
            ));
        }
    }
    for (scope_id, scope) in ctx.scopes.iter().enumerate() {
        for name in &scope.lets {
            let span = scope
                .let_spans
                .get(name)
                .copied()
                .unwrap_or(Span::default());
            if import_aliases.contains(name) {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    format!(
                        "cannot have both alias and field with name `{name}` in the same scope"
                    ),
                ));
            }
            for (other_id, other) in ctx.scopes.iter().enumerate() {
                if (ctx.is_ancestor(scope_id, other_id) || ctx.is_ancestor(other_id, scope_id))
                    && other.fields.contains(name)
                {
                    return Err(LangError::eval(
                        span.line,
                        span.col,
                        format!(
                            "cannot have both alias and field with name `{name}` in the same scope"
                        ),
                    ));
                }
            }
            if !scope.let_used.contains(name) {
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    format!("unreferenced alias or let clause `{name}`"),
                ));
            }
        }
        for (name, span) in &scope.binding_spans {
            for (other_id, other) in ctx.scopes.iter().enumerate() {
                if (ctx.is_ancestor(scope_id, other_id) || ctx.is_ancestor(other_id, scope_id))
                    && other.lets.contains(name)
                {
                    return Err(LangError::eval(
                        span.line,
                        span.col,
                        format!(
                            "cannot have both alias and field with name `{name}` in the same scope"
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn detect_let_cycles(ctx: &AliasCtx) -> Result<(), LangError> {
    use std::collections::HashMap as Map;

    let mut adjacency: Map<(usize, String), Vec<(usize, String)>> = Map::new();
    for (from, to) in &ctx.let_edges {
        adjacency.entry(from.clone()).or_default().push(to.clone());
    }
    let mut state: Map<(usize, String), u8> = Map::new();
    fn visit(
        node: &(usize, String),
        adjacency: &Map<(usize, String), Vec<(usize, String)>>,
        state: &mut Map<(usize, String), u8>,
        ctx: &AliasCtx,
    ) -> Result<(), LangError> {
        match state.get(node).copied().unwrap_or(0) {
            1 => {
                let span = ctx.scopes[node.0]
                    .let_spans
                    .get(&node.1)
                    .copied()
                    .unwrap_or(Span::default());
                return Err(LangError::eval(
                    span.line,
                    span.col,
                    "cyclic references in let clause or alias",
                ));
            }
            2 => return Ok(()),
            _ => {}
        }
        state.insert(node.clone(), 1);
        if let Some(neighbors) = adjacency.get(node) {
            for neighbor in neighbors {
                visit(neighbor, adjacency, state, ctx)?;
            }
        }
        state.insert(node.clone(), 2);
        Ok(())
    }
    let alias_targets: HashSet<(usize, String)> = ctx
        .aliases
        .iter()
        .filter_map(|alias| {
            alias.target.as_ref().and_then(|target| {
                ctx.scopes[alias.scope]
                    .required_fields
                    .contains(target)
                    .then(|| (alias.scope, field_node(target)))
            })
        })
        .collect();
    let roots: Vec<(usize, String)> = adjacency
        .keys()
        .filter(|node| !is_field_node(&node.1) || alias_targets.contains(*node))
        .cloned()
        .collect();
    for node in roots {
        visit(&node, &adjacency, &mut state, ctx)?;
    }
    Ok(())
}

fn rename_items(
    items: &mut [StructItem],
    locals: &HashMap<String, String>,
    shadowed: &mut HashSet<String>,
) {
    for item in items {
        match item {
            StructItem::Field(field) => {
                if let Some(alias) = &mut field.alias
                    && let Some(renamed) = locals.get(alias)
                {
                    alias.clone_from(renamed);
                }
                if let Label::Dynamic(expr, _) = &mut field.label {
                    rename_expr(expr, locals, shadowed);
                }
                rename_expr(&mut field.value, locals, shadowed);
            }
            StructItem::Pattern { value, .. } => rename_expr(value, locals, shadowed),
            StructItem::Let { name, value, .. } => {
                if let Some(renamed) = locals.get(name) {
                    name.clone_from(renamed);
                }
                rename_expr(value, locals, shadowed);
            }
            StructItem::For {
                key,
                value,
                source,
                body,
                ..
            } => {
                rename_expr(source, locals, shadowed);
                let mut scoped = shadowed.clone();
                if let Some(key) = key {
                    scoped.insert(key.clone());
                }
                scoped.insert(value.clone());
                rename_items(body, locals, &mut scoped);
            }
            StructItem::If { cond, body, .. } => {
                rename_expr(cond, locals, shadowed);
                rename_items(body, locals, shadowed);
            }
            StructItem::Ellipsis(_) => {}
        }
    }
}

fn rename_expr(expr: &mut Expr, locals: &HashMap<String, String>, shadowed: &HashSet<String>) {
    match expr {
        Expr::Ident(name, _) => {
            if !shadowed.contains(name)
                && let Some(renamed) = locals.get(name)
            {
                name.clone_from(renamed);
            }
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) | Expr::Bound(_, inner, _) => {
            rename_expr(inner, locals, shadowed);
        }
        Expr::Select(base, _, _) => rename_expr(base, locals, shadowed),
        Expr::Index(base, index, _) => {
            rename_expr(base, locals, shadowed);
            rename_expr(index, locals, shadowed);
        }
        Expr::Call(callee, arguments, _) => {
            rename_expr(callee, locals, shadowed);
            for argument in arguments {
                rename_expr(argument, locals, shadowed);
            }
        }
        Expr::Struct(items, _) => {
            let mut scoped = shadowed.clone();
            rename_items(items, locals, &mut scoped);
        }
        Expr::List(items, _) => {
            for item in items {
                rename_expr(item, locals, shadowed);
            }
        }
        Expr::OpenList(element, _) => {
            if let Some(element) = element {
                rename_expr(element, locals, shadowed);
            }
        }
        Expr::ListComp { clauses, body, .. } => {
            let mut scoped = shadowed.clone();
            for clause in clauses {
                match clause {
                    CompClause::For {
                        key, value, source, ..
                    } => {
                        rename_expr(source, locals, &scoped);
                        if let Some(key) = key {
                            scoped.insert(key.clone());
                        }
                        scoped.insert(value.clone());
                    }
                    CompClause::If { cond, .. } => rename_expr(cond, locals, &scoped),
                }
            }
            rename_expr(body, locals, &scoped);
        }
        Expr::OpenListPrefix(prefix, tail, _) => {
            for item in prefix {
                rename_expr(item, locals, shadowed);
            }
            if let Some(tail) = tail {
                rename_expr(tail, locals, shadowed);
            }
        }
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            rename_expr(left, locals, shadowed);
            rename_expr(right, locals, shadowed);
        }
        Expr::Binary(_, left, right, _) => {
            rename_expr(left, locals, shadowed);
            rename_expr(right, locals, shadowed);
        }
        Expr::Unary(_, inner, _) => {
            rename_expr(inner, locals, shadowed);
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let StrPart::Interp(inner) = part {
                    rename_expr(inner, locals, shadowed);
                }
            }
        }
        Expr::Null(_)
        | Expr::Bool(_, _)
        | Expr::Int(_, _)
        | Expr::Float(_, _)
        | Expr::Bytes(_, _) => {}
    }
}
