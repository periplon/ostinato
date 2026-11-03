//! CUE attribute helpers (`@tag`, ...).
//!
//! Attributes are metadata: they do not change the evaluated value. The
//! workflow loader uses `@tag(name,type=...)` to type `-p` overrides.

use std::collections::BTreeMap;

use crate::ast::{Expr, File, Label, StructItem};
use crate::error::LangError;

/// Injection type declared by `@tag(...,type=...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagType {
    String,
    Int,
    Bool,
    Number,
}

/// A `@tag` declaration: external name, parameter field and injection type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagInfo {
    /// Leaf parameter field name.
    pub field: String,
    /// Path from the `params` root to the tagged field.
    pub path: Vec<String>,
    pub ty: TagType,
}

fn label_name(field: &crate::ast::Field) -> Option<&str> {
    match &field.label {
        Label::Name(name, _) => Some(name),
        Label::Dynamic(_, _) => None,
    }
}

fn tag_info(
    field_name: &str,
    field: &crate::ast::Field,
) -> Result<Option<(String, TagType)>, LangError> {
    let Some(attribute) = field
        .attributes
        .iter()
        .find(|attribute| attribute.name == "tag")
    else {
        return Ok(None);
    };
    let mut ty = TagType::String;
    let mut name = field_name.to_owned();
    let mut saw_name = false;
    for argument in &attribute.args {
        if let Some((key, value)) = argument.split_once('=') {
            match key.trim() {
                "type" => {
                    ty = match value.trim() {
                        "string" => TagType::String,
                        "int" => TagType::Int,
                        "bool" => TagType::Bool,
                        "number" => TagType::Number,
                        other => {
                            return Err(LangError::eval(
                                attribute.span.line,
                                attribute.span.col,
                                format!("unknown @tag type `{other}`"),
                            ));
                        }
                    };
                }
                "name" => {
                    name = value.trim().to_owned();
                    saw_name = true;
                }
                _ => {}
            }
        } else if !saw_name {
            name = argument.trim().to_owned();
            saw_name = true;
        }
    }
    Ok(Some((name, ty)))
}

/// Map tag names to the top-level parameter fields that declare them.
///
/// # Errors
/// Returns an evaluation error for unknown `@tag` types.
pub fn parameter_tags(file: &File) -> Result<BTreeMap<String, Vec<TagInfo>>, LangError> {
    let mut definitions: BTreeMap<String, Vec<Expr>> = BTreeMap::new();
    for item in &file.items {
        match item {
            StructItem::Field(field) => {
                if let Some(name) = label_name(field) {
                    definitions
                        .entry(name.to_owned())
                        .or_default()
                        .push(field.value.clone());
                }
                if let Some(alias) = &field.alias {
                    definitions
                        .entry(alias.clone())
                        .or_default()
                        .push(field.value.clone());
                }
            }
            StructItem::Let { name, value, .. } => {
                definitions
                    .entry(name.clone())
                    .or_default()
                    .push(value.clone());
            }
            _ => {}
        }
    }
    let mut tags: BTreeMap<String, Vec<TagInfo>> = BTreeMap::new();
    let mut saw_params = false;
    for item in &file.items {
        let StructItem::Field(field) = item else {
            continue;
        };
        if label_name(field) == Some("params") {
            saw_params = true;
            collect_expr_tags(&field.value, &[], &mut tags, &definitions, &mut Vec::new())?;
        }
    }
    if !saw_params {
        collect_tags(&file.items, &[], &mut tags, &definitions)?;
    }
    Ok(tags)
}

fn collect_expr_tags(
    expr: &Expr,
    prefix: &[String],
    tags: &mut BTreeMap<String, Vec<TagInfo>>,
    definitions: &BTreeMap<String, Vec<Expr>>,
    visiting: &mut Vec<String>,
) -> Result<(), LangError> {
    match expr {
        Expr::Struct(items, _) => collect_tags(items, prefix, tags, definitions),
        Expr::Paren(inner, _) | Expr::Default(inner, _) => {
            collect_expr_tags(inner, prefix, tags, definitions, visiting)
        }
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            collect_expr_tags(left, prefix, tags, definitions, visiting)?;
            collect_expr_tags(right, prefix, tags, definitions, visiting)
        }
        Expr::Ident(name, _) => {
            let Some(values) = definitions.get(name) else {
                return Ok(());
            };
            if visiting.contains(name) {
                return Ok(());
            }
            visiting.push(name.clone());
            let mut result = Ok(());
            for value in values {
                result = collect_expr_tags(value, prefix, tags, definitions, visiting);
                if result.is_err() {
                    break;
                }
            }
            visiting.pop();
            result
        }
        _ => Ok(()),
    }
}

fn collect_tags(
    items: &[StructItem],
    prefix: &[String],
    tags: &mut BTreeMap<String, Vec<TagInfo>>,
    definitions: &BTreeMap<String, Vec<Expr>>,
) -> Result<(), LangError> {
    for item in items {
        let StructItem::Field(field) = item else {
            continue;
        };
        let Some(name) = label_name(field) else {
            continue;
        };
        let mut path = prefix.to_vec();
        path.push(name.to_owned());
        if let Some((tag_name, ty)) = tag_info(name, field)? {
            tags.entry(tag_name).or_default().push(TagInfo {
                field: name.to_owned(),
                path: path.clone(),
                ty,
            });
        }
        match &field.value {
            Expr::Struct(items, _) => {
                collect_tags(items, &path, tags, definitions)?;
            }
            Expr::Paren(inner, _) | Expr::Default(inner, _) => {
                collect_expr_tags(inner, &path, tags, definitions, &mut Vec::new())?;
            }
            Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
                collect_expr_tags(left, &path, tags, definitions, &mut Vec::new())?;
                collect_expr_tags(right, &path, tags, definitions, &mut Vec::new())?;
            }
            Expr::Ident(definition, _) => {
                if let Some(values) = definitions.get(definition) {
                    for value in values {
                        collect_expr_tags(value, &path, tags, definitions, &mut Vec::new())?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
