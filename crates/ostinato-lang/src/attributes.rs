//! CUE attribute helpers (`@tag`, ...).
//!
//! Attributes are metadata: they do not change the evaluated value. The
//! workflow loader uses `@tag(name,type=...)` to type `-p` overrides.

use std::collections::BTreeMap;

use crate::ast::{CompClause, Expr, File, Label, StrPart, StructItem};
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
    pub definition: bool,
}

/// A `@tag` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagInfo {
    /// Stable identity of the declaring field.
    pub field_id: usize,
    /// Leaf parameter field name.
    pub field: String,
    /// Path from the `params` root to the tagged field (inline params).
    pub path: Vec<TagSegment>,
    /// Declaration item path that owns the tag, when it lives in a named
    /// field or local `let`.
    pub origin: Option<Vec<usize>>,
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

type DefinitionMap = BTreeMap<String, Vec<(Vec<usize>, Expr)>>;

fn select_field_value(expr: &Expr, field: &str, quoted: bool) -> Option<Expr> {
    match expr {
        Expr::Struct(items, _) => items.iter().find_map(|item| {
            let StructItem::Field(candidate) = item else {
                return None;
            };
            let Label::Name(name, _) = &candidate.label else {
                return None;
            };
            (name == field && candidate.quoted == quoted).then(|| candidate.value.clone())
        }),
        Expr::Paren(inner, _) | Expr::Default(inner, _) => select_field_value(inner, field, quoted),
        Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
            select_field_value(left, field, quoted)
                .or_else(|| select_field_value(right, field, quoted))
        }
        _ => None,
    }
}

struct Collector<'a> {
    tags: &'a mut BTreeMap<String, Vec<TagInfo>>,
    definitions: &'a DefinitionMap,
    definition_aliases: &'a BTreeMap<String, String>,
    local_definitions: Vec<DefinitionMap>,
    selectors: Vec<Expr>,
    visiting: &'a mut Vec<String>,
}

impl Collector<'_> {
    fn expr(
        &mut self,
        expr: &Expr,
        prefix: &[TagSegment],
        origin: Option<&[usize]>,
        origin_prefix: &[TagSegment],
        item_base: &[usize],
    ) -> Result<(), LangError> {
        match expr {
            Expr::Struct(items, _) => self.items(items, prefix, origin, origin_prefix, item_base),
            Expr::Paren(inner, _) | Expr::Default(inner, _) => {
                self.expr(inner, prefix, origin, origin_prefix, item_base)
            }
            Expr::Unify(left, right, _) | Expr::Disjunction(left, right, _) => {
                self.expr(left, prefix, origin, origin_prefix, item_base)?;
                self.expr(right, prefix, origin, origin_prefix, item_base)
            }
            Expr::Select(_, _, _, _) | Expr::Index(_, _, _) => {
                if self.selectors.contains(expr) {
                    return Ok(());
                }
                self.selectors.push(expr.clone());
                let result = if let Some((path, value, relative)) = self.resolve_selector(expr) {
                    self.expr(&value, prefix, Some(&path), &relative, &path)
                } else {
                    Ok(())
                };
                self.selectors.pop();
                result
            }
            Expr::Ident(name, _) => {
                let target = self
                    .definition_aliases
                    .get(name)
                    .map_or(name.as_str(), String::as_str);
                let local = self
                    .local_definitions
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(target));
                let values = match local {
                    Some(values) => values.clone(),
                    None => {
                        let Some(values) = self.definitions.get(target) else {
                            return Ok(());
                        };
                        values.clone()
                    }
                };
                if self.visiting.iter().any(|value| value == target) {
                    return Ok(());
                }
                self.visiting.push(target.to_owned());
                let mut result = Ok(());
                for (path, value) in &values {
                    result = self.expr(value, prefix, Some(path), &[], path);
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

    fn lookup(&self, name: &str) -> Option<Vec<(Vec<usize>, Expr)>> {
        let target = self
            .definition_aliases
            .get(name)
            .map_or(name, String::as_str);
        if let Some(values) = self
            .local_definitions
            .iter()
            .rev()
            .find_map(|scope| scope.get(target))
        {
            return Some(values.clone());
        }
        self.definitions.get(target).cloned()
    }

    fn resolve_selector(&self, expr: &Expr) -> Option<(Vec<usize>, Expr, Vec<TagSegment>)> {
        match expr {
            Expr::Ident(name, _) => {
                let values = self.lookup(name)?;
                let (path, value) = values.first()?;
                Some((path.clone(), value.clone(), Vec::new()))
            }
            Expr::Select(base, field, quoted, _) => {
                let (path, value, mut relative) = self.resolve_selector(base)?;
                let selected = select_field_value(&value, field, *quoted)?;
                relative.push(TagSegment {
                    name: field.clone(),
                    quoted: *quoted,
                    definition: false,
                });
                Some((path, selected, relative))
            }
            Expr::Index(base, index, _) => {
                let Expr::Str(parts, _) = &**index else {
                    return None;
                };
                if parts
                    .iter()
                    .any(|part| matches!(part, crate::ast::StrPart::Interp(_)))
                {
                    return None;
                }
                let field: String = parts
                    .iter()
                    .filter_map(|part| match part {
                        crate::ast::StrPart::Text(text) => Some(text.as_str()),
                        crate::ast::StrPart::Interp(_) => None,
                    })
                    .collect();
                let (path, value, mut relative) = self.resolve_selector(base)?;
                let selected = select_field_value(&value, &field, true)?;
                relative.push(TagSegment {
                    name: field,
                    quoted: true,
                    definition: false,
                });
                Some((path, selected, relative))
            }
            Expr::Paren(inner, _) | Expr::Default(inner, _) => self.resolve_selector(inner),
            _ => None,
        }
    }

    fn items(
        &mut self,
        items: &[StructItem],
        prefix: &[TagSegment],
        origin: Option<&[usize]>,
        origin_prefix: &[TagSegment],
        item_base: &[usize],
    ) -> Result<(), LangError> {
        let mut local = DefinitionMap::new();
        for (index, item) in items.iter().enumerate() {
            if let StructItem::Let { name, value, .. } = item {
                let mut path = item_base.to_vec();
                path.push(index);
                local
                    .entry(name.clone())
                    .or_default()
                    .push((path, value.clone()));
            }
        }
        self.local_definitions.push(local);
        let result = self.items_inner(items, prefix, origin, origin_prefix, item_base);
        self.local_definitions.pop();
        result
    }

    fn items_inner(
        &mut self,
        items: &[StructItem],
        prefix: &[TagSegment],
        origin: Option<&[usize]>,
        origin_prefix: &[TagSegment],
        item_base: &[usize],
    ) -> Result<(), LangError> {
        for (index, item) in items.iter().enumerate() {
            let StructItem::Field(field) = item else {
                continue;
            };
            let Some(name) = label_name(field) else {
                continue;
            };
            let segment = TagSegment {
                name: name.to_owned(),
                quoted: field.quoted,
                definition: field.definition,
            };
            let mut path = prefix.to_vec();
            path.push(segment.clone());
            let mut owned_origin = origin_prefix.to_vec();
            owned_origin.push(segment.clone());
            if let Some((tag_name, ty)) = tag_info(name, field)? {
                self.tags.entry(tag_name).or_default().push(TagInfo {
                    field_id: field.id,
                    field: name.to_owned(),
                    path: path.clone(),
                    origin: origin.map(<[usize]>::to_vec),
                    origin_path: if origin.is_some() {
                        owned_origin.clone()
                    } else {
                        Vec::new()
                    },
                    ty,
                });
            }
            let mut item_path = item_base.to_vec();
            item_path.push(index);
            if field.definition {
                self.expr(&field.value, &path, Some(&item_path), &[], &item_path)?;
            } else {
                self.expr(&field.value, &path, origin, &owned_origin, &item_path)?;
            }
        }
        Ok(())
    }
}

/// Map tag names to the parameter fields that declare them.
///
/// # Errors
/// Returns an evaluation error for unknown `@tag` types.
pub fn parameter_tags(file: &File) -> Result<BTreeMap<String, Vec<TagInfo>>, LangError> {
    let mut definitions: DefinitionMap = BTreeMap::new();
    let mut definition_aliases: BTreeMap<String, String> = BTreeMap::new();
    for (index, item) in file.items.iter().enumerate() {
        match item {
            StructItem::Field(field) => {
                if let Some(name) = label_name(field) {
                    definitions
                        .entry(name.to_owned())
                        .or_default()
                        .push((vec![index], field.value.clone()));
                }
                if let Some(alias) = &field.alias {
                    definitions
                        .entry(alias.clone())
                        .or_default()
                        .push((vec![index], field.value.clone()));
                    if let Label::Name(name, _) = &field.label
                        && name.starts_with('#')
                    {
                        definition_aliases.insert(alias.clone(), name.clone());
                    }
                }
            }
            StructItem::Let { name, value, .. } => {
                definitions
                    .entry(name.clone())
                    .or_default()
                    .push((vec![index], value.clone()));
            }
            _ => {}
        }
    }
    let mut tags: BTreeMap<String, Vec<TagInfo>> = BTreeMap::new();
    let mut visiting = Vec::new();
    let mut collector = Collector {
        tags: &mut tags,
        definitions: &definitions,
        definition_aliases: &definition_aliases,
        local_definitions: Vec::new(),
        selectors: Vec::new(),
        visiting: &mut visiting,
    };
    let mut saw_params = false;
    for (index, item) in file.items.iter().enumerate() {
        let StructItem::Field(field) = item else {
            continue;
        };
        if label_name(field) == Some("params") {
            saw_params = true;
            collector.expr(&field.value, &[], None, &[], &[index])?;
        }
    }
    if !saw_params {
        collector.items(&file.items, &[], None, &[], &[])?;
    }
    Ok(tags)
}

/// Assign stable ids to every field in the package AST.
pub fn assign_field_ids(file: &mut File) {
    let mut next = 0usize;
    for item in &mut file.items {
        assign_item(item, &mut next);
    }
}

fn assign_item(item: &mut StructItem, next: &mut usize) {
    match item {
        StructItem::Field(field) => {
            field.id = *next;
            *next += 1;
            assign_expr(&mut field.value, next);
        }
        StructItem::Pattern { value, .. } => assign_expr(value, next),
        StructItem::Let { value, .. } => assign_expr(value, next),
        StructItem::For { source, body, .. } => {
            assign_expr(source, next);
            for item in body {
                assign_item(item, next);
            }
        }
        StructItem::If { cond, body, .. } => {
            assign_expr(cond, next);
            for item in body {
                assign_item(item, next);
            }
        }
        StructItem::Ellipsis(_) => {}
    }
}

fn assign_expr(expr: &mut Expr, next: &mut usize) {
    match expr {
        Expr::Select(base, _, _, _) | Expr::Bound(_, base, _) | Expr::Unary(_, base, _) => {
            assign_expr(base, next);
        }
        Expr::Index(base, index, _) => {
            assign_expr(base, next);
            assign_expr(index, next);
        }
        Expr::Call(callee, arguments, _) => {
            assign_expr(callee, next);
            for argument in arguments {
                assign_expr(argument, next);
            }
        }
        Expr::Paren(inner, _) | Expr::Default(inner, _) => assign_expr(inner, next),
        Expr::Struct(items, _) => {
            for item in items {
                assign_item(item, next);
            }
        }
        Expr::List(items, _) | Expr::OpenListPrefix(items, _, _) => {
            for item in items {
                assign_expr(item, next);
            }
        }
        Expr::OpenList(Some(inner), _) => assign_expr(inner, next),
        Expr::ListComp { clauses, body, .. } => {
            for clause in clauses {
                match clause {
                    CompClause::For { source, .. } => assign_expr(source, next),
                    CompClause::If { cond, .. } => assign_expr(cond, next),
                }
            }
            assign_expr(body, next);
        }
        Expr::Unify(left, right, _)
        | Expr::Disjunction(left, right, _)
        | Expr::Binary(_, left, right, _) => {
            assign_expr(left, next);
            assign_expr(right, next);
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let StrPart::Interp(inner) = part {
                    assign_expr(inner, next);
                }
            }
        }
        Expr::Null(_)
        | Expr::Bool(_, _)
        | Expr::Int(_, _)
        | Expr::Float(_, _)
        | Expr::Bytes(_, _)
        | Expr::Ident(_, _)
        | Expr::OpenList(None, _) => {}
    }
}
