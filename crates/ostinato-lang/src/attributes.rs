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
    pub field: String,
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
    let mut tags: BTreeMap<String, Vec<TagInfo>> = BTreeMap::new();
    let mut saw_params = false;
    for item in &file.items {
        let StructItem::Field(field) = item else {
            continue;
        };
        if label_name(field) == Some("params")
            && let Expr::Struct(items, _) = &field.value
        {
            saw_params = true;
            collect_tags(items, &mut tags)?;
        }
    }
    if !saw_params {
        collect_tags(&file.items, &mut tags)?;
    }
    Ok(tags)
}

fn collect_tags(
    items: &[StructItem],
    tags: &mut BTreeMap<String, Vec<TagInfo>>,
) -> Result<(), LangError> {
    for item in items {
        let StructItem::Field(field) = item else {
            continue;
        };
        let Some(name) = label_name(field) else {
            continue;
        };
        if let Some((tag_name, ty)) = tag_info(name, field)? {
            tags.entry(tag_name).or_default().push(TagInfo {
                field: name.to_owned(),
                ty,
            });
        }
    }
    Ok(())
}
