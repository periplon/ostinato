use serde_json::Value as Json;

use crate::error::LangError;

/// Render `{{ path.to.value }}` placeholders against a JSON context.
///
/// # Errors
/// Returns [`LangError::Template`] for unterminated placeholders or unknown
/// references.
pub fn render(template: &str, context: &Json) -> Result<String, LangError> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            return Err(LangError::Template(
                "unterminated `{{ ... }}` placeholder".to_owned(),
            ));
        };
        let path = after[..end].trim();
        if path.is_empty() {
            return Err(LangError::Template("empty placeholder".to_owned()));
        }
        let value = lookup_path(context, path)?;
        out.push_str(&stringify(value));
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Resolve a dotted path in a JSON value. Array indices are numeric segments.
///
/// # Errors
/// Returns [`LangError::Template`] when the path does not exist.
pub fn lookup_path<'a>(context: &'a Json, path: &str) -> Result<&'a Json, LangError> {
    let mut current = context;
    for segment in path.split('.') {
        current = match current {
            Json::Object(map) => map.get(segment).ok_or_else(|| {
                LangError::Template(format!("unknown template reference `{path}`"))
            })?,
            Json::Array(list) => {
                let index = segment.parse::<usize>().map_err(|_| {
                    LangError::Template(format!(
                        "`{segment}` is not a valid array index in `{path}`"
                    ))
                })?;
                list.get(index).ok_or_else(|| {
                    LangError::Template(format!("array index out of range in `{path}`"))
                })?
            }
            _ => {
                return Err(LangError::Template(format!(
                    "cannot descend into `{segment}` in `{path}`"
                )));
            }
        };
    }
    Ok(current)
}

fn stringify(value: &Json) -> String {
    match value {
        Json::Null => String::new(),
        Json::String(value) => value.clone(),
        Json::Bool(value) => value.to_string(),
        Json::Number(value) => value.to_string(),
        Json::Array(_) | Json::Object(_) => value.to_string(),
    }
}
