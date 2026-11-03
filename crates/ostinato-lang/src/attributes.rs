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

/// One label in a tag path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagSegment {
    pub name: String,
    pub quoted: bool,
}

/// A `@tag` declaration: external name, parameter path and injection type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagInfo {
    /// Leaf parameter field name.
    pub field: String,
    /// Path from the `params` root to the tagged field.
    pub path: Vec<TagSegment>,
    /// Declaration that owns the tag (`#P`, a `let`, ...), when applicable.
    pub origin: Option<String>,
    /// Path from the origin declaration root to the tagged field.
    pub origin_path: Vec<TagSegment>,
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

struct Collector<'a> {
    tags: &'a mut BTreeMap<String, Vec<TagInfo>>,
    definitions: &'a BTreeMap<String, Vec<Expr>>,
    visiting: &'a mut Vec<String>,
}

impl Collector<'_> {
    fn expr(
        &mut self,
        expr: &Expr,
        prefix: &[TagSegment],
        origin: Option<&str>,
        origin_prefix: &[TagSegment],
    ) -> Result<(), LangError> {
        match expr {
            Expr::Struct(items, _) => self.items(items, prefix, origin, origin_prefix),
            Expr::Paren(inner, _) | Expr::Default(inner, _) => {
                self.expr(inner, prefix, origin, origin_prefix)
            }
            Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
                self.expr(left, prefix, origin, origin_prefix)?;
                self.expr(right, prefix, origin, origin_prefix)
            }
            Expr::Ident(name, _) => {
                let Some(values) = self.definitions.get(name) else {
                    return Ok(());
                };
                if self.visiting.contains(name) {
                    return Ok(());
                }
                self.visiting.push(name.clone());
                let mut result = Ok(());
                for value in values {
                    result = self.expr(value, prefix, Some(name), &[]);
                    if result.is_err() {
                        break;
                    }
                }
                self.visiting.pop();
                result
            }
            _ => Ok(()),
        }
    }

    fn items(
        &mut self,
        items: &[StructItem],
        prefix: &[TagSegment],
        origin: Option<&str>,
        origin_prefix: &[TagSegment],
    ) -> Result<(), LangError> {
        for item in items {
            let StructItem::Field(field) = item else {
                continue;
            };
            let Some(name) = label_name(field) else {
                continue;
            };
            let segment = TagSegment {
                name: name.to_owned(),
                quoted: field.quoted,
            };
            let mut path = prefix.to_vec();
            path.push(segment.clone());
            let mut owned_origin = origin_prefix.to_vec();
            owned_origin.push(segment);
            if let Some((tag_name, ty)) = tag_info(name, field)? {
                self.tags.entry(tag_name).or_default().push(TagInfo {
                    field: name.to_owned(),
                    path: path.clone(),
                    origin: origin.map(ToOwned::to_owned),
                    origin_path: if origin.is_some() {
                        owned_origin.clone()
                    } else {
                        Vec::new()
                    },
                    ty,
                });
            }
            self.expr(&field.value, &path, origin, &owned_origin)?;
        }
        Ok(())
    }
}

/// Map tag names to the parameter fields that declare them.
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
    let mut visiting = Vec::new();
    let mut collector = Collector {
        tags: &mut tags,
        definitions: &definitions,
        visiting: &mut visiting,
    };
    let mut saw_params = false;
    for item in &file.items {
        let StructItem::Field(field) = item else {
            continue;
        };
        if label_name(field) == Some("params") {
            saw_params = true;
            collector.expr(&field.value, &[], None, &[])?;
        }
    }
    if !saw_params {
        collector.items(&file.items, &[], None, &[])?;
    }
    Ok(tags)
}
