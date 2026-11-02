//! File-scoped name resolution.
//!
//! CUE scopes `import` and `let` declarations to the file that contains them,
//! while fields and definitions are package-scoped. Ostinato evaluates a
//! package as one value tree, so this pass rewrites file-local bindings and
//! their references to unique names before files are merged.

use std::collections::{HashMap, HashSet};

use crate::ast::{CompClause, Expr, File, Label, StrPart, StructItem};

/// Rewrite file-local imports and `let` bindings to unique names.
pub fn rename_file_locals(file: &mut File, file_index: usize) {
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
        return;
    }
    for import in &mut file.imports {
        if let Some(renamed) = locals.get(&import.alias) {
            import.alias.clone_from(renamed);
        }
    }
    let mut shadowed = HashSet::new();
    rename_items(&mut file.items, &locals, &mut shadowed);
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
            for clause in clauses {
                match clause {
                    CompClause::For {
                        key, value, source, ..
                    } => {
                        rename_expr(source, locals, shadowed);
                        let mut scoped = shadowed.clone();
                        if let Some(key) = key {
                            scoped.insert(key.clone());
                        }
                        scoped.insert(value.clone());
                        rename_expr(body, locals, &scoped);
                    }
                    CompClause::If { cond, .. } => rename_expr(cond, locals, shadowed),
                }
            }
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
