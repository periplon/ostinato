//! Render JSON data as CUE source.
//!
//! This is the pure conversion used by `ostinato import` and by embedders. It
//! emits object fields, structs, lists and scalars in input order. Formatting
//! is not byte-identical to `cue import`, but the value is preserved.

use serde_json::{Map, Value as Json};

/// Render a JSON value as a CUE file body.
///
/// Object roots are emitted as top-level fields. Lists and scalars are emitted
/// as a single top-level value, which CUE accepts. When `package` is set, a
/// `package <name>` header is prepended.
#[must_use]
pub fn render_cue_data(value: &Json, package: Option<&str>) -> String {
    let body = match value {
        Json::Object(fields) if !fields.is_empty() => render_fields(fields, 0),
        _ => format!("{}\n", render_value(value, 0)),
    };
    match package {
        Some(package) => format!("package {package}\n\n{body}"),
        None => body,
    }
}

/// Whether a string is a valid CUE package identifier.
#[must_use]
pub fn is_valid_package(package: &str) -> bool {
    is_identifier(package) && !is_keyword(package)
}

fn render_fields(fields: &Map<String, Json>, indent: usize) -> String {
    let mut out = String::new();
    for (key, value) in fields {
        out.push_str(&"    ".repeat(indent));
        out.push_str(&render_label(key));
        out.push_str(": ");
        out.push_str(&render_value(value, indent));
        out.push('\n');
    }
    out
}

fn render_label(key: &str) -> String {
    if is_identifier(key) && !is_keyword(key) {
        key.to_owned()
    } else {
        serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_owned())
    }
}

fn render_value(value: &Json, indent: usize) -> String {
    match value {
        Json::Null => "null".to_owned(),
        Json::Bool(value) => value.to_string(),
        Json::Number(number) => number.to_string(),
        Json::String(text) => serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned()),
        Json::Array(values) => render_array(values, indent),
        Json::Object(fields) => {
            if fields.is_empty() {
                "{}".to_owned()
            } else {
                let mut out = String::from("{\n");
                out.push_str(&render_fields(fields, indent + 1));
                out.push_str(&"    ".repeat(indent));
                out.push('}');
                out
            }
        }
    }
}

fn render_array(values: &[Json], indent: usize) -> String {
    if values.is_empty() {
        return "[]".to_owned();
    }
    let scalars: Vec<String> = values
        .iter()
        .map(|value| render_value(value, indent + 1))
        .collect();
    let compact = values.iter().all(is_scalar)
        && scalars.iter().map(String::len).sum::<usize>() + values.len() * 2 <= 72;
    if compact {
        return format!("[{}]", scalars.join(", "));
    }
    let mut out = String::from("[\n");
    for scalar in scalars {
        out.push_str(&"    ".repeat(indent + 1));
        out.push_str(&scalar);
        out.push_str(",\n");
    }
    out.push_str(&"    ".repeat(indent));
    out.push(']');
    out
}

fn is_scalar(value: &Json) -> bool {
    !matches!(value, Json::Array(_) | Json::Object(_))
}

fn is_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first != '_' && !first.is_ascii_alphabetic() {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_keyword(key: &str) -> bool {
    matches!(
        key,
        "package" | "import" | "for" | "in" | "if" | "let" | "true" | "false" | "null"
    ) || key == "_"
        || key.starts_with('#')
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn renders_objects_in_input_order() {
        let value = serde_json::json!({
            "z": 1,
            "a-b": "x",
            "nested": {"b": true, "a": [1, 2]},
        });
        let cue = render_cue_data(&value, None);
        assert_eq!(
            cue,
            "z: 1\n\"a-b\": \"x\"\nnested: {\n    b: true\n    a: [1, 2]\n}\n"
        );
    }

    #[test]
    fn renders_scalars_and_lists() {
        assert_eq!(render_cue_data(&serde_json::json!(5), None), "5\n");
        assert_eq!(
            render_cue_data(&serde_json::json!([1, "x"]), None),
            "[1, \"x\"]\n"
        );
    }

    #[test]
    fn package_header_and_validation() {
        assert_eq!(
            render_cue_data(&serde_json::json!({"a": 1}), Some("demo")),
            "package demo\n\na: 1\n"
        );
        assert!(is_valid_package("demo"));
        assert!(!is_valid_package("9bad"));
    }

    #[test]
    fn big_integers_are_verbatim() {
        let value: Json = serde_json::from_str("123456789012345678901234567890").expect("json");
        assert_eq!(
            render_cue_data(&value, None),
            "123456789012345678901234567890\n"
        );
    }
}
