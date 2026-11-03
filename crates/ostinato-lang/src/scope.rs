use std::collections::{HashMap, HashSet};

use crate::ast::{CompClause, Expr, File, Label, Span, StrPart, StructItem};
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

#[derive(Debug)]
struct AliasScope {
    parent: Option<usize>,
    fields: HashSet<String>,
    lets: HashSet<String>,
    let_spans: HashMap<String, Span>,
    let_used: HashSet<String>,
    bindings: HashSet<String>,
    binding_spans: Vec<(String, Span)>,
    aliases: HashMap<String, usize>,
}

#[derive(Debug)]
struct AliasInfo {
    name: String,
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

    fn scope(
        &mut self,
        items: &[StructItem],
        parent: Option<usize>,
        visible: &[usize],
        shadowed: &HashSet<String>,
    ) -> Result<usize, LangError> {
        let id = self.scopes.len();
        self.scopes.push(AliasScope {
            parent,
            fields: HashSet::new(),
            lets: HashSet::new(),
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
                        self.aliases.push(AliasInfo {
                            name: alias.clone(),
                            scope: id,
                            span: field.span,
                            used: false,
                        });
                        self.scopes[id].aliases.insert(alias.clone(), alias_id);
                    }
                }
                StructItem::Let { name, span, .. } => {
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
                    }
                    self.expr(&field.value, &chain, &local_shadow, None)?;
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
                    self.expr(value, &chain, &local_shadow, None)?;
                }
                StructItem::For {
                    key,
                    value,
                    source,
                    body,
                    span,
                } => {
                    self.expr(source, &chain, &local_shadow, None)?;
                    let mut body_shadow = local_shadow.clone();
                    if let Some(key) = key {
                        body_shadow.insert(key.clone());
                    }
                    body_shadow.insert(value.clone());
                    let body_id = self.scope(body, Some(id), &chain, &body_shadow)?;
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
                    self.expr(cond, &chain, &local_shadow, None)?;
                    self.scope(body, Some(id), &chain, &local_shadow)?;
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
                    if let Some(alias) = self.scopes[*scope].aliases.get(name) {
                        self.aliases[*alias].used = true;
                        return Ok(());
                    }
                }
            }
            Expr::Paren(inner, _) | Expr::Default(inner, _) => {
                self.expr(inner, chain, shadowed, from)?;
            }
            Expr::Select(base, _, _) => self.expr(base, chain, shadowed, from)?,
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
                self.scope(items, parent, chain, shadowed)?;
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
                    lets: HashSet::new(),
                    let_spans: HashMap::new(),
                    let_used: HashSet::new(),
                    bindings: HashSet::new(),
                    binding_spans: Vec::new(),
                    aliases: HashMap::new(),
                });
                let mut comp_chain = chain.to_vec();
                comp_chain.push(comp_id);
                let mut body_shadow = shadowed.clone();
                for clause in clauses {
                    match clause {
                        CompClause::For {
                            key,
                            value,
                            source,
                            span,
                        } => {
                            self.expr(source, chain, shadowed, from)?;
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
                            self.expr(cond, &comp_chain, &body_shadow, from)?;
                        }
                    }
                }
                self.expr(body, &comp_chain, &body_shadow, from)?;
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
            Expr::Null(_) | Expr::Bool(_, _) | Expr::Int(_, _) | Expr::Float(_, _) => {}
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
    let root = ctx.scope(items, None, &[], &HashSet::new())?;
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
    let nodes: Vec<(usize, String)> = adjacency.keys().cloned().collect();
    for node in nodes {
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
        Expr::Paren(inner, _) => rename_expr(inner, locals, shadowed),
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
        Expr::Unary(_, inner, _) | Expr::Default(inner, _) => {
            rename_expr(inner, locals, shadowed);
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let StrPart::Interp(inner) = part {
                    rename_expr(inner, locals, shadowed);
                }
            }
        }
        Expr::Null(_) | Expr::Bool(_, _) | Expr::Int(_, _) | Expr::Float(_, _) => {}
    }
}
