//! CUE attribute helpers (`@tag`, ...).
//!
//! Attributes are metadata: they do not change the evaluated value. The
//! workflow loader uses `@tag(name,type=...)` to type `-p` overrides.

use std::collections::BTreeMap;

use crate::ast::{Attribute, Expr, File, Label, StructItem};

/// Injection type declared by `@tag(...,type=...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagType {
    String,
    Int,
    Bool,
    Number,
}

fn label_name(field: &crate::ast::Field) -> Option<&str> {
    match &field.label {
        Label::Name(name, _) => Some(name),
        Label::Dynamic(_, _) => None,
    }
}

fn tag_type(field: &crate::ast::Field) -> Option<TagType> {
    let attribute = field
        .attributes
        .iter()
        .find(|attribute| attribute.name == "tag")?;
    let mut ty = TagType::String;
    for argument in &attribute.args {
        if let Some((key, value)) = argument.split_once('=')
            && key.trim() == "type"
        {
            ty = match value.trim() {
                "int" => TagType::Int,
                "bool" => TagType::Bool,
                "number" => TagType::Number,
                _ => TagType::String,
            };
        }
    }
    Some(ty)
}

/// Map parameter field names to their `@tag` injection type.
#[must_use]
pub fn parameter_tags(file: &File) -> BTreeMap<String, TagType> {
    let mut tags = BTreeMap::new();
    let mut saw_params = false;
    for item in &file.items {
        let StructItem::Field(field) = item else {
            continue;
        };
        if label_name(field) == Some("params")
            && let Expr::Struct(items, _) = &field.value
        {
            saw_params = true;
            collect_tags(items, &mut tags);
        }
    }
    if !saw_params {
        collect_tags(&file.items, &mut tags);
    }
    tags
}

fn collect_tags(items: &[StructItem], tags: &mut BTreeMap<String, TagType>) {
    for item in items {
        let StructItem::Field(field) = item else {
            continue;
        };
        let Some(name) = label_name(field) else {
            continue;
        };
        if let Some(ty) = tag_type(field) {
            tags.insert(name.to_owned(), ty);
        }
        if let Expr::Struct(inner, _) = &field.value {
            collect_tags(inner, tags);
        }
    }
}

/// Whether an attribute is one of the known metadata attributes.
#[must_use]
pub fn is_known_attribute(attribute: &Attribute) -> bool {
    matches!(
        attribute.name.as_str(),
        "tag" | "if" | "deprecated" | "extern" | "hidden" | "go" | "jsonschema"
    )
}
