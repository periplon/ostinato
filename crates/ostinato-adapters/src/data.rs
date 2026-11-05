//! JSON and YAML data ingestion and emission.
//!
//! This adapter is deliberately independent from the workflow engine: it turns
//! bytes on disk into [`serde_json::Value`]s and back. The CLI uses it for
//! `import`, `export`, `--input` and `-p @file` handling.
//!
//! # Security policy
//!
//! * Input files are limited to [`MAX_INPUT_BYTES`] (16 MiB).
//! * Documents are limited to [`MAX_DEPTH`] levels of nesting.
//! * Duplicate JSON or YAML mapping keys are rejected; CUE also rejects them.
//! * Custom YAML tags are rejected.
//! * YAML aliases are resolved by the parser with a bounded node budget
//!   (`saphyr`'s default of 100,000 nodes), which rejects billion-laughs style
//!   expansion. Documents that stay within the budget are accepted, matching
//!   Go's YAML decoder used by the CUE CLI.
//! * Non-finite YAML floats (`.inf`, `.nan`) are rejected because JSON has no
//!   representation for them.
//! * JSON numbers are kept in their original arbitrary-precision form, so
//!   integers above 2^53 are never rounded.
//!
//! YAML parsing and emission use [`saphyr`], a maintained YAML 1.2
//! implementation. `serde_yaml` is deprecated and `serde_yml` is a deprecated
//! shim, so neither is used. Emission builds a `saphyr::Yaml` tree and keeps
//! number tokens as raw scalars to preserve arbitrary precision.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use saphyr::{Mapping, Scalar, ScalarStyle, Yaml, YamlEmitter, YamlLoader};
use saphyr_parser::{BufferedInput, Event, Parser, Span, SpannedEventReceiver};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Number, Value as Json};
use thiserror::Error;

/// Maximum accepted size of one JSON or YAML input file.
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;

/// Maximum accepted nesting depth of a parsed document.
pub const MAX_DEPTH: usize = 128;

/// Maximum number of YAML documents in one file.
pub const MAX_YAML_DOCUMENTS: usize = 256;

/// Maximum number of YAML events (nodes) in one file.
pub const MAX_YAML_NODES: usize = 1_000_000;

/// Maximum number of aliases in one YAML document.
pub const MAX_YAML_ALIASES: usize = 50_000;

/// Maximum node budget produced by alias expansion (matches CUE's 50k cap).
pub const MAX_ALIAS_EXPANSION_NODES: usize = 50_000;

/// Data format inferred from a file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    Json,
    Yaml,
}

impl DataFormat {
    /// Infer the format from a path extension.
    ///
    /// # Errors
    /// Returns [`DataError::UnsupportedFormat`] for unknown extensions.
    pub fn from_path(path: &Path) -> Result<Self, DataError> {
        match path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("json") => Ok(Self::Json),
            Some("yaml" | "yml") => Ok(Self::Yaml),
            _ => Err(DataError::UnsupportedFormat {
                path: path.display().to_string(),
            }),
        }
    }

    /// Whether a path looks like a supported data file.
    #[must_use]
    pub fn is_data_path(path: &Path) -> bool {
        Self::from_path(path).is_ok()
    }

    /// Human-readable name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Yaml => "yaml",
        }
    }
}

/// Typed data-format failures.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DataError {
    #[error("cannot read {path}: {message}")]
    Io { path: String, message: String },
    #[error("{path}: input is {bytes} bytes, maximum is {max} bytes")]
    TooLarge {
        path: String,
        bytes: u64,
        max: usize,
    },
    #[error("{path}: unsupported data format (expected .json, .yaml or .yml)")]
    UnsupportedFormat { path: String },
    #[error("{path}: input is not valid UTF-8")]
    InvalidEncoding { path: String },
    #[error("{path}: invalid JSON: {message}")]
    InvalidJson { path: String, message: String },
    #[error("{path}: invalid YAML: {message}")]
    InvalidYaml { path: String, message: String },
    #[error("{path}: duplicate key `{key}`")]
    DuplicateKey { path: String, key: String },
    #[error("{path}: custom YAML tag `{tag}` is not supported")]
    UnsupportedTag { path: String, tag: String },
    #[error("{path}: complex YAML mapping keys are not supported")]
    ComplexKey { path: String },
    #[error("{path}: non-finite YAML float `{value}` cannot be represented as JSON")]
    NonFiniteFloat { path: String, value: String },
    #[error("{path}: document nesting exceeds {max} levels")]
    TooDeep { path: String, max: usize },
    #[error("{path}: expected a mapping/object at the document root, found {found}")]
    ExpectedObject { path: String, found: &'static str },
    #[error("{path}: reference `{at}` not found")]
    MissingPath { path: String, at: String },
    #[error("{path}: `{at}` is not a list index")]
    BadIndex { path: String, at: String },
    #[error("{path}: conflicting values while unifying documents at `{at}`")]
    Conflict { path: String, at: String },
    #[error("{path}: cannot encode YAML: {message}")]
    Encode { path: String, message: String },
}

impl DataError {
    fn io(path: &Path, error: &std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            message: error.to_string(),
        }
    }
}

/// Read and decode every document in a JSON or YAML file.
///
/// A JSON file always yields exactly one document. YAML may yield several
/// documents separated by `---`.
///
/// # Errors
/// Returns a typed [`DataError`] for missing files, unsupported extensions,
/// oversized input, malformed documents, duplicate keys, tags or excessive
/// nesting.
pub fn read_documents(path: &Path) -> Result<Vec<Json>, DataError> {
    let format = DataFormat::from_path(path)?;
    let metadata = std::fs::metadata(path).map_err(|error| DataError::io(path, &error))?;
    if metadata.len() > MAX_INPUT_BYTES as u64 {
        return Err(DataError::TooLarge {
            path: path.display().to_string(),
            bytes: metadata.len(),
            max: MAX_INPUT_BYTES,
        });
    }
    let bytes = std::fs::read(path).map_err(|error| DataError::io(path, &error))?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(DataError::TooLarge {
            path: path.display().to_string(),
            bytes: bytes.len() as u64,
            max: MAX_INPUT_BYTES,
        });
    }
    let text = String::from_utf8(bytes).map_err(|_| DataError::InvalidEncoding {
        path: path.display().to_string(),
    })?;
    parse_documents(format, &text, path)
}

/// Decode documents from an in-memory string.
///
/// # Errors
/// See [`read_documents`].
pub fn parse_documents(
    format: DataFormat,
    text: &str,
    origin: &Path,
) -> Result<Vec<Json>, DataError> {
    match format {
        DataFormat::Json => {
            if text.trim().is_empty() {
                return Ok(vec![Json::Null]);
            }
            let value: StrictJson =
                serde_json::from_str(text).map_err(|error| DataError::InvalidJson {
                    path: origin.display().to_string(),
                    message: error.to_string(),
                })?;
            check_json_depth(&value.0, 0, origin)?;
            Ok(vec![value.0])
        }
        DataFormat::Yaml => parse_yaml_documents(text, origin),
    }
}

/// Read a single object document, used for `-p @file`.
///
/// Multiple YAML documents are unified in order; every document must be an
/// object.
///
/// # Errors
/// Propagates decoding errors and rejects non-object roots or conflicts.
pub fn read_object_document(path: &Path) -> Result<Json, DataError> {
    let documents = read_documents(path)?;
    unify_objects(&documents, path)
}

/// Unify object documents in order, merging nested objects.
///
/// This is the data-file analogue of CUE's multi-file unification: later
/// documents may add fields, but two concrete values for the same leaf are a
/// conflict.
///
/// # Errors
/// Rejects non-object roots, empty input and concrete conflicts.
pub fn unify_objects(documents: &[Json], origin: &Path) -> Result<Json, DataError> {
    let mut iter = documents.iter();
    let Some(first) = iter.next() else {
        return Err(DataError::ExpectedObject {
            path: origin.display().to_string(),
            found: "nothing",
        });
    };
    if !first.is_object() {
        return Err(DataError::ExpectedObject {
            path: origin.display().to_string(),
            found: json_type_name(first),
        });
    }
    let mut merged = first.clone();
    for document in iter {
        if !document.is_object() {
            return Err(DataError::ExpectedObject {
                path: origin.display().to_string(),
                found: json_type_name(document),
            });
        }
        let at = "$".to_owned();
        merged = merge_json(&merged, document, &at, origin)?;
    }
    Ok(merged)
}

/// Encode a JSON value as YAML.
///
/// Object keys are emitted in sorted order. Every string value is
/// double-quoted, which keeps the output valid for any JSON string. Numbers
/// are emitted as their original JSON token, preserving arbitrary precision.
///
/// # Errors
/// Returns [`DataError::Encode`] if the YAML emitter fails.
pub fn to_yaml(value: &Json) -> Result<String, DataError> {
    let yaml = json_to_yaml(value);
    let mut out = String::new();
    {
        let mut emitter = YamlEmitter::new(&mut out);
        emitter.quoted_value(true);
        emitter.dump(&yaml).map_err(|error| DataError::Encode {
            path: "<output>".to_owned(),
            message: error.to_string(),
        })?;
    }
    let body = out.strip_prefix("---\n").unwrap_or(&out);
    let body = body.trim_end_matches('\n');
    Ok(format!("{body}\n"))
}

/// Encode a JSON value as pretty JSON with a trailing newline.
///
/// # Errors
/// Returns [`DataError::Encode`] if serialization fails.
pub fn to_json(value: &Json) -> Result<String, DataError> {
    let mut out = serde_json::to_string_pretty(value).map_err(|error| DataError::Encode {
        path: "<output>".to_owned(),
        message: error.to_string(),
    })?;
    out.push('\n');
    Ok(out)
}

/// Encode with the selected format.
///
/// # Errors
/// Propagates encoder errors.
pub fn encode(format: DataFormat, value: &Json) -> Result<String, DataError> {
    match format {
        DataFormat::Json => to_json(value),
        DataFormat::Yaml => to_yaml(value),
    }
}

fn parse_yaml_documents(text: &str, origin: &Path) -> Result<Vec<Json>, DataError> {
    // Parse scalars lazily so the original token is available. This keeps
    // integers larger than i64 (which YAML parsers often coerce to f64)
    // exact by feeding them to serde_json's arbitrary-precision Number.
    let mut parser = Parser::new(BufferedInput::new(text.chars()));
    let mut budget = BudgetLoader::new();
    parser
        .load(&mut budget, true)
        .map_err(|error| DataError::InvalidYaml {
            path: origin.display().to_string(),
            message: error.to_string(),
        })?;
    if let Some(violation) = budget.violation.take() {
        return Err(DataError::InvalidYaml {
            path: origin.display().to_string(),
            message: violation,
        });
    }
    if let Some(error) = budget.loader.error() {
        return Err(DataError::InvalidYaml {
            path: origin.display().to_string(),
            message: error.to_string(),
        });
    }
    let documents = budget.loader.into_documents();
    if documents.is_empty() {
        // An empty YAML stream is an empty document. CUE represents it as
        // null for data purposes.
        return Ok(vec![Json::Null]);
    }
    let mut out = Vec::with_capacity(documents.len());
    for document in &documents {
        out.push(yaml_to_json(document, 0, "$", origin)?);
    }
    Ok(out)
}

/// A parser receiver that bounds documents, nodes, aliases and alias cycles
/// before delegating to the ordinary YAML loader.
struct BudgetLoader<'a> {
    loader: YamlLoader<'a, Yaml<'a>>,
    documents: usize,
    nodes: usize,
    aliases: usize,
    stack: Vec<Option<usize>>,
    started: BTreeSet<usize>,
    finished: BTreeSet<usize>,
    violation: Option<String>,
}

impl<'a> BudgetLoader<'a> {
    fn new() -> Self {
        let mut loader = YamlLoader::<Yaml>::default();
        loader.early_parse(false);
        loader.alias_limit(MAX_ALIAS_EXPANSION_NODES);
        Self {
            loader,
            documents: 0,
            nodes: 0,
            aliases: 0,
            stack: Vec::new(),
            started: BTreeSet::new(),
            finished: BTreeSet::new(),
            violation: None,
        }
    }

    fn violate(&mut self, message: impl Into<String>) {
        if self.violation.is_none() {
            self.violation = Some(message.into());
        }
    }
}

impl<'input> SpannedEventReceiver<'input> for BudgetLoader<'input> {
    fn on_event(&mut self, event: Event<'input>, span: Span) {
        match &event {
            Event::DocumentStart(_) => {
                self.documents += 1;
                if self.documents > MAX_YAML_DOCUMENTS {
                    self.violate(format!(
                        "document count exceeds the maximum of {MAX_YAML_DOCUMENTS}"
                    ));
                }
            }
            Event::Scalar(_, _, anchor, _) => {
                self.nodes += 1;
                if *anchor > 0 {
                    self.started.insert(*anchor);
                    self.finished.insert(*anchor);
                }
            }
            Event::SequenceStart(anchor, _) | Event::MappingStart(anchor, _) => {
                self.nodes += 1;
                if *anchor > 0 {
                    self.started.insert(*anchor);
                    self.stack.push(Some(*anchor));
                } else {
                    self.stack.push(None);
                }
            }
            Event::SequenceEnd | Event::MappingEnd => {
                if let Some(Some(anchor)) = self.stack.pop() {
                    self.finished.insert(anchor);
                }
            }
            Event::Alias(anchor) => {
                self.aliases += 1;
                if self.aliases > MAX_YAML_ALIASES {
                    self.violate(format!(
                        "alias count exceeds the maximum of {MAX_YAML_ALIASES}"
                    ));
                }
                if self.started.contains(anchor) && !self.finished.contains(anchor) {
                    self.violate("cyclic YAML alias is not supported");
                }
            }
            _ => {}
        }
        if self.nodes > MAX_YAML_NODES {
            self.violate(format!(
                "node count exceeds the maximum of {MAX_YAML_NODES}"
            ));
        }
        self.loader.on_event(event, span);
    }
}

fn yaml_to_json(node: &Yaml<'_>, depth: usize, at: &str, origin: &Path) -> Result<Json, DataError> {
    if depth > MAX_DEPTH {
        return Err(DataError::TooDeep {
            path: origin.display().to_string(),
            max: MAX_DEPTH,
        });
    }
    match node {
        Yaml::Value(Scalar::Null) => Ok(Json::Null),
        Yaml::Value(Scalar::Boolean(value)) => Ok(Json::Bool(*value)),
        Yaml::Value(Scalar::Integer(value)) => Ok(Json::Number(Number::from(*value))),
        Yaml::Value(Scalar::FloatingPoint(value)) => {
            let value = value.into_inner();
            let Some(number) = Number::from_f64(value) else {
                return Err(DataError::NonFiniteFloat {
                    path: origin.display().to_string(),
                    value: value.to_string(),
                });
            };
            Ok(Json::Number(number))
        }
        Yaml::Value(Scalar::String(value)) => Ok(Json::String(value.to_string())),
        Yaml::Representation(value, style, tag) => {
            resolve_yaml_scalar(value, *style, tag.as_deref(), origin)
        }
        Yaml::Sequence(sequence) => {
            let mut out = Vec::with_capacity(sequence.len());
            for (index, item) in sequence.iter().enumerate() {
                out.push(yaml_to_json(
                    item,
                    depth + 1,
                    &format!("{at}[{index}]"),
                    origin,
                )?);
            }
            Ok(Json::Array(out))
        }
        Yaml::Mapping(mapping) => yaml_mapping_to_json(mapping, depth, at, origin),
        Yaml::Tagged(tag, _) => Err(DataError::UnsupportedTag {
            path: origin.display().to_string(),
            tag: tag.to_string(),
        }),
        Yaml::Alias(alias) => Err(DataError::InvalidYaml {
            path: origin.display().to_string(),
            message: format!("unresolved alias `*{alias}`"),
        }),
        Yaml::BadValue => Err(DataError::InvalidYaml {
            path: origin.display().to_string(),
            message: "invalid YAML value".to_owned(),
        }),
    }
}

fn yaml_mapping_to_json(
    mapping: &Mapping<'_>,
    depth: usize,
    at: &str,
    origin: &Path,
) -> Result<Json, DataError> {
    // Merge keys (`<<`) are resolved after explicit fields are known. Per the
    // YAML merge-key specification, explicit keys win and earlier merged maps
    // take precedence over later ones.
    let mut explicit = Vec::with_capacity(mapping.len());
    let mut merged: Vec<(String, Json)> = Vec::new();
    let mut seen_explicit = BTreeSet::new();
    for (key, value) in mapping {
        let key = yaml_key(key, origin)?;
        if key == "<<" {
            merge_yaml_mapping(value, depth, at, origin, &mut merged)?;
            continue;
        }
        if !seen_explicit.insert(key.clone()) {
            return Err(DataError::DuplicateKey {
                path: origin.display().to_string(),
                key,
            });
        }
        explicit.push((key, value));
    }
    let mut fields = Map::new();
    for (key, value) in merged {
        fields.entry(key).or_insert(value);
    }
    for (key, value) in explicit {
        let child_at = format!("{at}.{key}");
        let converted = yaml_to_json(value, depth + 1, &child_at, origin)?;
        fields.insert(key, converted);
    }
    Ok(Json::Object(fields))
}

fn merge_yaml_mapping(
    value: &Yaml<'_>,
    depth: usize,
    at: &str,
    origin: &Path,
    merged: &mut Vec<(String, Json)>,
) -> Result<(), DataError> {
    match value {
        Yaml::Mapping(mapping) => {
            let Json::Object(object) = yaml_mapping_to_json(mapping, depth + 1, at, origin)? else {
                return Ok(());
            };
            merged.extend(object);
            Ok(())
        }
        Yaml::Sequence(sequence) => {
            for item in sequence {
                merge_yaml_mapping(item, depth + 1, at, origin, merged)?;
            }
            Ok(())
        }
        _ => Err(DataError::InvalidYaml {
            path: origin.display().to_string(),
            message: "YAML merge key `<<` expects a mapping or a list of mappings".to_owned(),
        }),
    }
}

fn resolve_yaml_scalar(
    text: &str,
    style: ScalarStyle,
    tag: Option<&saphyr_parser::Tag>,
    origin: &Path,
) -> Result<Json, DataError> {
    if let Some(tag) = tag {
        if !tag.is_yaml_core_schema() {
            return Err(DataError::UnsupportedTag {
                path: origin.display().to_string(),
                tag: tag.to_string(),
            });
        }
        return match tag.suffix.as_str() {
            "str" => Ok(Json::String(text.to_owned())),
            "null" => Ok(Json::Null),
            "bool" => parse_yaml_bool(text)
                .map(Json::Bool)
                .ok_or_else(|| DataError::InvalidYaml {
                    path: origin.display().to_string(),
                    message: format!("invalid `!!bool` value `{text}`"),
                }),
            "int" => parse_yaml_integer(text).ok_or_else(|| DataError::InvalidYaml {
                path: origin.display().to_string(),
                message: format!("invalid `!!int` value `{text}`"),
            }),
            "float" => parse_yaml_float(text, origin),
            _ => Err(DataError::UnsupportedTag {
                path: origin.display().to_string(),
                tag: tag.to_string(),
            }),
        };
    }
    if style != ScalarStyle::Plain {
        return Ok(Json::String(text.to_owned()));
    }
    if text.is_empty() || matches!(text, "~" | "null" | "Null" | "NULL") {
        return Ok(Json::Null);
    }
    if let Some(value) = parse_yaml_bool(text) {
        return Ok(Json::Bool(value));
    }
    if is_yaml_integer(text) {
        return parse_yaml_integer(text).ok_or_else(|| DataError::InvalidYaml {
            path: origin.display().to_string(),
            message: format!("integer `{text}` is out of range"),
        });
    }
    if is_yaml_float(text) {
        return parse_yaml_float(text, origin);
    }
    Ok(Json::String(text.to_owned()))
}

fn parse_yaml_bool(text: &str) -> Option<bool> {
    match text {
        "true" | "True" | "TRUE" => Some(true),
        "false" | "False" | "FALSE" => Some(false),
        _ => None,
    }
}

fn is_yaml_integer(text: &str) -> bool {
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    if let Some(rest) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        return !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_hexdigit());
    }
    if let Some(rest) = unsigned
        .strip_prefix("0o")
        .or_else(|| unsigned.strip_prefix("0O"))
    {
        return !rest.is_empty() && rest.chars().all(|ch| matches!(ch, '0'..='7'));
    }
    if let Some(rest) = unsigned
        .strip_prefix("0b")
        .or_else(|| unsigned.strip_prefix("0B"))
    {
        return !rest.is_empty() && rest.chars().all(|ch| matches!(ch, '0' | '1'));
    }
    !unsigned.is_empty() && unsigned.chars().all(|ch| ch.is_ascii_digit() || ch == '_')
}

fn parse_yaml_integer(text: &str) -> Option<Json> {
    let cleaned: String = text.chars().filter(|ch| *ch != '_').collect();
    let (negative, unsigned) = match cleaned.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, cleaned.strip_prefix('+').unwrap_or(&cleaned)),
    };
    let number = if let Some(rest) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        u128::from_str_radix(rest, 16)
            .ok()
            .map(|value| value.to_string())
    } else if let Some(rest) = unsigned
        .strip_prefix("0o")
        .or_else(|| unsigned.strip_prefix("0O"))
    {
        u128::from_str_radix(rest, 8)
            .ok()
            .map(|value| value.to_string())
    } else if let Some(rest) = unsigned
        .strip_prefix("0b")
        .or_else(|| unsigned.strip_prefix("0B"))
    {
        u128::from_str_radix(rest, 2)
            .ok()
            .map(|value| value.to_string())
    } else {
        let trimmed = unsigned.trim_start_matches('0');
        Some(if trimmed.is_empty() {
            "0".to_owned()
        } else {
            trimmed.to_owned()
        })
    }?;
    let signed = if negative {
        format!("-{number}")
    } else {
        number
    };
    signed.parse::<Number>().ok().map(Json::Number)
}

fn is_yaml_float(text: &str) -> bool {
    if matches!(
        text,
        ".inf"
            | ".Inf"
            | ".INF"
            | "-.inf"
            | "-.Inf"
            | "-.INF"
            | "+.inf"
            | "+.Inf"
            | "+.INF"
            | ".nan"
            | ".NaN"
            | ".NAN"
    ) {
        return true;
    }
    if text.contains(['.', 'e', 'E']) && text.chars().any(|ch| ch.is_ascii_digit()) {
        return normalized_float(text).is_some();
    }
    false
}

fn parse_yaml_float(text: &str, origin: &Path) -> Result<Json, DataError> {
    if matches!(
        text,
        ".inf"
            | ".Inf"
            | ".INF"
            | "-.inf"
            | "-.Inf"
            | "-.INF"
            | "+.inf"
            | "+.Inf"
            | "+.INF"
            | ".nan"
            | ".NaN"
            | ".NAN"
    ) {
        return Err(DataError::NonFiniteFloat {
            path: origin.display().to_string(),
            value: text.to_owned(),
        });
    }
    let normalized = normalized_float(text).ok_or_else(|| DataError::InvalidYaml {
        path: origin.display().to_string(),
        message: format!("invalid float `{text}`"),
    })?;
    normalized
        .parse::<Number>()
        .map(Json::Number)
        .map_err(|error| DataError::InvalidYaml {
            path: origin.display().to_string(),
            message: format!("invalid float `{text}`: {error}"),
        })
}

fn normalized_float(text: &str) -> Option<String> {
    let mut out = text.trim().to_owned();
    if out.starts_with('+') {
        out.remove(0);
    }
    if out.starts_with("-.") {
        out.insert(1, '0');
    } else if out.starts_with('.') {
        out.insert(0, '0');
    }
    if out.ends_with('.') {
        out.push('0');
    }
    if out.contains(".e") {
        out = out.replace(".e", ".0e");
    }
    if out.contains(".E") {
        out = out.replace(".E", ".0E");
    }
    let (sign, rest) = match out.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", out.as_str()),
    };
    let end = rest.find(['.', 'e', 'E']).unwrap_or(rest.len());
    let (integer, tail) = rest.split_at(end);
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let normalized = format!("{sign}{integer}{tail}");
    normalized.parse::<Number>().ok().map(|_| normalized)
}

fn yaml_key(node: &Yaml<'_>, origin: &Path) -> Result<String, DataError> {
    match node {
        Yaml::Value(Scalar::String(value)) => Ok(value.to_string()),
        Yaml::Value(Scalar::Boolean(value)) => Ok(value.to_string()),
        Yaml::Value(Scalar::Integer(value)) => Ok(value.to_string()),
        Yaml::Value(Scalar::FloatingPoint(value)) => Ok(value.into_inner().to_string()),
        Yaml::Value(Scalar::Null) => Ok("null".to_owned()),
        Yaml::Representation(value, _, None) => Ok(value.to_string()),
        Yaml::Representation(_, _, Some(tag)) => Err(DataError::UnsupportedTag {
            path: origin.display().to_string(),
            tag: tag.to_string(),
        }),
        Yaml::Tagged(tag, _) => Err(DataError::UnsupportedTag {
            path: origin.display().to_string(),
            tag: tag.to_string(),
        }),
        _ => Err(DataError::ComplexKey {
            path: origin.display().to_string(),
        }),
    }
}

fn json_to_yaml(value: &Json) -> Yaml<'static> {
    match value {
        Json::Null => Yaml::Value(Scalar::Null),
        Json::Bool(value) => Yaml::Value(Scalar::Boolean(*value)),
        Json::Number(number) => {
            Yaml::Representation(Cow::Owned(number.to_string()), ScalarStyle::Plain, None)
        }
        Json::String(value) => Yaml::Value(Scalar::String(Cow::Owned(value.clone()))),
        Json::Array(values) => Yaml::Sequence(values.iter().map(json_to_yaml).collect()),
        Json::Object(fields) => {
            let mut mapping = Mapping::default();
            for (key, value) in fields {
                mapping.insert(
                    Yaml::Value(Scalar::String(Cow::Owned(key.clone()))),
                    json_to_yaml(value),
                );
            }
            Yaml::Mapping(mapping)
        }
    }
}

fn check_json_depth(value: &Json, depth: usize, origin: &Path) -> Result<(), DataError> {
    if depth > MAX_DEPTH {
        return Err(DataError::TooDeep {
            path: origin.display().to_string(),
            max: MAX_DEPTH,
        });
    }
    match value {
        Json::Array(values) => {
            for item in values {
                check_json_depth(item, depth + 1, origin)?;
            }
        }
        Json::Object(fields) => {
            for item in fields.values() {
                check_json_depth(item, depth + 1, origin)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn merge_json(left: &Json, right: &Json, at: &str, origin: &Path) -> Result<Json, DataError> {
    match (left, right) {
        (Json::Object(left), Json::Object(right)) => {
            let mut out = left.clone();
            for (key, value) in right {
                let child_at = format!("{at}.{key}");
                match out.get(key) {
                    Some(existing) => {
                        let merged = merge_json(existing, value, &child_at, origin)?;
                        out.insert(key.clone(), merged);
                    }
                    None => {
                        out.insert(key.clone(), value.clone());
                    }
                }
            }
            Ok(Json::Object(out))
        }
        (Json::Array(left), Json::Array(right)) if left.len() == right.len() => {
            let mut out = Vec::with_capacity(left.len());
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                out.push(merge_json(left, right, &format!("{at}[{index}]"), origin)?);
            }
            Ok(Json::Array(out))
        }
        _ if left == right => Ok(left.clone()),
        _ => Err(DataError::Conflict {
            path: origin.display().to_string(),
            at: at.to_owned(),
        }),
    }
}

/// Walk a JSON value with a dotted selector. Numeric segments index arrays.
///
/// Square brackets are accepted as an alternative to dots (`list[1]` is the
/// same as `list.1`).
///
/// # Errors
/// Returns [`DataError::Conflict`] when a path does not exist.
pub fn select_json_path<'a>(value: &'a Json, expression: &str) -> Result<&'a Json, DataError> {
    let segments = normalize_selector(expression).map_err(|message| DataError::MissingPath {
        path: "<expression>".to_owned(),
        at: message,
    })?;
    let mut current = value;
    for segment in segments {
        current = match current {
            Json::Object(fields) => fields.get(&segment).ok_or_else(|| DataError::MissingPath {
                path: "<expression>".to_owned(),
                at: segment.clone(),
            })?,
            Json::Array(values) => {
                let index = segment.parse::<usize>().map_err(|_| DataError::BadIndex {
                    path: "<expression>".to_owned(),
                    at: segment.clone(),
                })?;
                values.get(index).ok_or_else(|| DataError::BadIndex {
                    path: "<expression>".to_owned(),
                    at: segment.clone(),
                })?
            }
            _ => {
                return Err(DataError::MissingPath {
                    path: "<expression>".to_owned(),
                    at: segment,
                });
            }
        };
    }
    Ok(current)
}

fn normalize_selector(expression: &str) -> Result<Vec<String>, String> {
    let expression = expression.trim();
    if expression.is_empty() {
        return Err("empty expression".to_owned());
    }
    let mut segments = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = expression.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '.' => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                } else if segments.is_empty() {
                    return Err("empty path segment".to_owned());
                }
                index += 1;
            }
            '[' => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
                let Some(end) = chars[index + 1..].iter().position(|ch| *ch == ']') else {
                    return Err("unterminated `[` in expression".to_owned());
                };
                let inner: String = chars[index + 1..index + 1 + end].iter().collect();
                if inner.is_empty() {
                    return Err("empty `[]` index".to_owned());
                }
                segments.push(inner);
                index += end + 2;
            }
            other => {
                current.push(other);
                index += 1;
            }
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    if segments.is_empty() {
        return Err("empty expression".to_owned());
    }
    Ok(segments)
}

fn json_type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "null",
        Json::Bool(_) => "bool",
        Json::Number(_) => "number",
        Json::String(_) => "string",
        Json::Array(_) => "list",
        Json::Object(_) => "object",
    }
}

/// A file plus its decoded documents, used by `import` and export.
#[derive(Debug, Clone)]
pub struct DataFile {
    pub path: PathBuf,
    pub documents: Vec<Json>,
}

impl DataFile {
    /// Read all documents from a file.
    ///
    /// # Errors
    /// Propagates [`read_documents`] failures.
    pub fn read(path: &Path) -> Result<Self, DataError> {
        Ok(Self {
            path: path.to_path_buf(),
            documents: read_documents(path)?,
        })
    }
}

// ---------------------------------------------------------------------------
// Strict JSON decoding
// ---------------------------------------------------------------------------

/// A JSON value tree that rejects duplicate object keys.
///
/// `serde_json` normally keeps the last duplicate. CUE rejects duplicates, so
/// the adapter rejects them too. The visitor mirrors serde_json's
/// arbitrary-precision number protocol (`$serde_json::private::Number`).
struct StrictJson(Json);

impl<'de> Deserialize<'de> for StrictJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictVisitor)
    }
}

struct StrictVisitor;

/// serde_json's arbitrary-precision number protocol sends numbers through a
/// single-entry map with this key. The workspace always enables the feature;
/// the constant is harmless when it is disabled because the key never appears.
const NUMBER_TOKEN: &str = "$serde_json::private::Number";

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = StrictJson;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictJson(Json::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictJson(Json::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictJson(Json::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let Some(number) = Number::from_f64(value) else {
            return Err(E::custom("non-finite JSON number"));
        };
        Ok(StrictJson(Json::Number(number)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StrictJson(Json::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictJson(Json::String(value)))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictJson(Json::Null))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictJson(Json::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut out = Vec::new();
        while let Some(item) = sequence.next_element::<StrictJson>()? {
            out.push(item.0);
        }
        Ok(StrictJson(Json::Array(out)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let Some(first_key) = map.next_key::<String>()? else {
            return Ok(StrictJson(Json::Object(Map::new())));
        };
        if first_key == NUMBER_TOKEN {
            let raw: String = map.next_value()?;
            let number = raw
                .parse::<Number>()
                .map_err(|error| de::Error::custom(format!("invalid JSON number: {error}")))?;
            return Ok(StrictJson(Json::Number(number)));
        }
        let first_value: StrictJson = map.next_value()?;
        let mut out = Map::new();
        out.insert(first_key.clone(), first_value.0);
        while let Some(key) = map.next_key::<String>()? {
            if out.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate key `{key}`")));
            }
            let value: StrictJson = map.next_value()?;
            out.insert(key, value.0);
        }
        Ok(StrictJson(Json::Object(out)))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn parse(format: DataFormat, text: &str) -> Vec<Json> {
        parse_documents(format, text, Path::new("test")).expect("parse")
    }

    #[test]
    fn json_roundtrip_preserves_structure_and_big_integers() {
        let source = r#"{"a":1,"b":[true,null,"x"],"c":{"d":2.5},"big":9007199254740993,"huge":123456789012345678901234567890}"#;
        let value = parse(DataFormat::Json, source).remove(0);
        let yaml = to_yaml(&value).expect("yaml");
        assert!(yaml.contains("9007199254740993"), "{yaml}");
        assert!(yaml.contains("123456789012345678901234567890"), "{yaml}");
        let back = parse(DataFormat::Yaml, &yaml).remove(0);
        assert_eq!(back, value);
        let json = to_json(&value).expect("json");
        let back = parse(DataFormat::Json, &json).remove(0);
        assert_eq!(back, value);
    }

    #[test]
    fn yaml_scalars_match_cue_resolution() {
        let docs = parse(
            DataFormat::Yaml,
            "yes_val: yes\nno_val: no\non_val: on\noff_val: off\nt: true\nn: null\ns: \"123\"\nf: 1.5\n",
        );
        let Json::Object(fields) = &docs[0] else {
            panic!("object")
        };
        assert_eq!(fields["yes_val"], Json::String("yes".to_owned()));
        assert_eq!(fields["no_val"], Json::String("no".to_owned()));
        assert_eq!(fields["on_val"], Json::String("on".to_owned()));
        assert_eq!(fields["off_val"], Json::String("off".to_owned()));
        assert_eq!(fields["t"], Json::Bool(true));
        assert_eq!(fields["n"], Json::Null);
        assert_eq!(fields["s"], Json::String("123".to_owned()));
    }

    #[test]
    fn multi_document_yaml_is_decoded_in_order() {
        let docs = parse(DataFormat::Yaml, "a: 1\n---\nb: 2\n");
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0]["a"], 1);
        assert_eq!(docs[1]["b"], 2);
    }

    #[test]
    fn empty_yaml_document_is_null() {
        let docs = parse(DataFormat::Yaml, "---\n---\n");
        assert_eq!(docs, vec![Json::Null, Json::Null]);
    }

    #[test]
    fn duplicate_json_keys_are_rejected() {
        let error = parse_documents(DataFormat::Json, r#"{"a":1,"a":2}"#, Path::new("t"))
            .expect_err("duplicate");
        assert!(matches!(error, DataError::InvalidJson { .. }));
        assert!(error.to_string().contains("duplicate key"));
    }

    #[test]
    fn duplicate_yaml_keys_are_rejected() {
        let error = parse_documents(DataFormat::Yaml, "a: 1\na: 2\n", Path::new("t"))
            .expect_err("duplicate");
        assert!(matches!(error, DataError::InvalidYaml { .. }));
        assert!(error.to_string().contains("duplicated key"));
    }

    #[test]
    fn custom_yaml_tags_are_rejected() {
        let error =
            parse_documents(DataFormat::Yaml, "x: !custom 5\n", Path::new("t")).expect_err("tag");
        assert!(matches!(error, DataError::UnsupportedTag { .. }));
    }

    #[test]
    fn non_finite_yaml_floats_are_rejected() {
        for text in ["x: .inf\n", "x: -.inf\n", "x: .nan\n"] {
            let error = parse_documents(DataFormat::Yaml, text, Path::new("t")).expect_err("float");
            assert!(matches!(error, DataError::NonFiniteFloat { .. }));
        }
    }

    #[test]
    fn merge_keys_are_expanded_with_explicit_fields_winning() {
        let docs = parse(
            DataFormat::Yaml,
            "base: &b\n  x: 1\n  y: 2\nchild:\n  <<: *b\n  y: 3\n",
        );
        assert_eq!(docs[0]["child"]["x"], 1);
        assert_eq!(docs[0]["child"]["y"], 3);
    }

    #[test]
    fn yaml_alias_bomb_is_bounded() {
        // Ten nested anchors with ten aliases each exceed the 100k node budget.
        let mut text = String::from("a0: &a0 [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]\n");
        for level in 1..10 {
            text.push_str(&format!("a{level}: &a{level} ["));
            for index in 0..10 {
                if index > 0 {
                    text.push_str(", ");
                }
                text.push_str(&format!("*a{}", level - 1));
            }
            text.push_str("]\n");
        }
        let error = parse_documents(DataFormat::Yaml, &text, Path::new("t")).expect_err("bomb");
        assert!(error.to_string().contains("billion-laughs"), "{error}");
    }

    #[test]
    fn depth_limit_is_enforced_for_json_and_yaml() {
        let deep_json = format!(
            "{}1{}",
            "[".repeat(MAX_DEPTH + 2),
            "]".repeat(MAX_DEPTH + 2)
        );
        let error =
            parse_documents(DataFormat::Json, &deep_json, Path::new("t")).expect_err("depth");
        assert!(matches!(
            error,
            DataError::TooDeep { .. } | DataError::InvalidJson { .. }
        ));
        let deep_yaml = format!(
            "{}x: 1\n",
            (0..=MAX_DEPTH)
                .map(|level| format!("{}key{level}:\n", "  ".repeat(level)))
                .collect::<String>()
        );
        let error =
            parse_documents(DataFormat::Yaml, &deep_yaml, Path::new("t")).expect_err("yaml depth");
        assert!(matches!(error, DataError::TooDeep { .. }), "{error}");
    }

    #[test]
    fn input_size_limit_is_enforced() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("large.json");
        std::fs::write(&path, "1".repeat(MAX_INPUT_BYTES + 1)).expect("write");
        let error = read_documents(&path).expect_err("size");
        assert!(matches!(error, DataError::TooLarge { .. }), "{error}");
    }

    #[test]
    fn unified_documents_merge_objects_and_reject_conflicts() {
        let merged = unify_objects(
            &[
                serde_json::json!({"a": 1, "nested": {"x": 1}}),
                serde_json::json!({"b": 2, "nested": {"y": 2}}),
            ],
            Path::new("t"),
        )
        .expect("merge");
        assert_eq!(
            merged,
            serde_json::json!({"a": 1, "b": 2, "nested": {"x": 1, "y": 2}})
        );
        let error = unify_objects(
            &[serde_json::json!({"a": 1}), serde_json::json!({"a": 2})],
            Path::new("t"),
        )
        .expect_err("conflict");
        assert!(matches!(error, DataError::Conflict { .. }));
    }

    #[test]
    fn non_string_yaml_keys_are_stringified() {
        let docs = parse(DataFormat::Yaml, "1: a\ntrue: b\n1.5: c\n");
        let Json::Object(fields) = &docs[0] else {
            panic!("object")
        };
        assert_eq!(fields["1"], Json::String("a".to_owned()));
        assert_eq!(fields["true"], Json::String("b".to_owned()));
        assert_eq!(fields["1.5"], Json::String("c".to_owned()));
    }

    #[test]
    fn select_json_path_supports_dots_and_indexes() {
        let value = serde_json::json!({"a": {"b": [10, 20]}});
        assert_eq!(select_json_path(&value, "a.b[1]").expect("path"), &20);
        assert!(select_json_path(&value, "a.nope").is_err());
    }

    #[test]
    fn empty_files_decode_to_null_and_empty_containers_are_preserved() {
        assert_eq!(parse(DataFormat::Json, ""), vec![Json::Null]);
        assert_eq!(parse(DataFormat::Yaml, ""), vec![Json::Null]);
        assert_eq!(parse(DataFormat::Json, "{}"), vec![serde_json::json!({})]);
        assert_eq!(parse(DataFormat::Json, "[]"), vec![serde_json::json!([])]);
    }

    #[test]
    fn ordering_is_preserved_for_json_and_yaml() {
        let value = parse(DataFormat::Json, r#"{"z":1,"a":2,"m":{"y":3,"b":4}}"#).remove(0);
        let json = to_json(&value).expect("json");
        assert!(json.find("\"z\"").expect("z") < json.find("\"a\"").expect("a"));
        let yaml = to_yaml(&value).expect("yaml");
        assert!(yaml.find("z: 1").expect("z") < yaml.find("a: 2").expect("a"));
        let back = parse(DataFormat::Yaml, &yaml).remove(0);
        assert_eq!(back, value);
    }

    #[test]
    fn lexical_numbers_keep_precision() {
        let docs = parse(
            DataFormat::Json,
            r#"{"thirty":123456789012345678901234567890,"huge":1e400,"small":1.25}"#,
        );
        assert_eq!(
            docs[0]["thirty"].to_string(),
            "123456789012345678901234567890"
        );
        assert!(docs[0]["huge"].to_string().contains("400"));
        let docs = parse(
            DataFormat::Yaml,
            "thirty: 123456789012345678901234567890\nhuge: 1e400\n",
        );
        assert_eq!(
            docs[0]["thirty"].to_string(),
            "123456789012345678901234567890"
        );
        assert!(docs[0]["huge"].to_string().contains("400"));
    }

    #[test]
    fn yaml_timestamps_stay_strings_and_binary_is_rejected() {
        let docs = parse(DataFormat::Yaml, "t: 2020-01-02T03:04:05Z\n");
        assert_eq!(
            docs[0]["t"],
            Json::String("2020-01-02T03:04:05Z".to_owned())
        );
        let error = parse_documents(
            DataFormat::Yaml,
            "b: !!binary \"aGVsbG8=\"\n",
            Path::new("t"),
        )
        .expect_err("binary");
        assert!(matches!(error, DataError::UnsupportedTag { .. }));
    }

    #[test]
    fn yaml_key_collisions_are_rejected() {
        let error = parse_documents(DataFormat::Yaml, "1: a\n\"1\": b\n", Path::new("t"))
            .expect_err("collision");
        assert!(matches!(error, DataError::DuplicateKey { .. }), "{error}");
    }

    #[test]
    fn yaml_document_and_node_limits_are_enforced() {
        let mut text = String::new();
        for index in 0..=MAX_YAML_DOCUMENTS {
            text.push_str(&format!("---\na{index}: 1\n"));
        }
        let error = parse_documents(DataFormat::Yaml, &text, Path::new("t")).expect_err("docs");
        assert!(error.to_string().contains("document count"));

        let mut text = String::new();
        for index in 0..=MAX_YAML_NODES {
            text.push_str(&format!("- {index}\n"));
        }
        let error = parse_documents(DataFormat::Yaml, &text, Path::new("t")).expect_err("nodes");
        assert!(error.to_string().contains("node count"));
    }

    #[test]
    fn yaml_alias_count_limit_is_enforced() {
        let mut text = String::from("anchor: &a 1\n");
        for index in 0..=MAX_YAML_ALIASES {
            text.push_str(&format!("x{index}: *a\n"));
        }
        let error = parse_documents(DataFormat::Yaml, &text, Path::new("t")).expect_err("aliases");
        let message = error.to_string();
        assert!(
            message.contains("alias") || message.contains("billion-laughs"),
            "{message}"
        );
    }

    #[test]
    fn cyclic_aliases_are_rejected() {
        let error =
            parse_documents(DataFormat::Yaml, "a: &a [*a]\n", Path::new("t")).expect_err("cycle");
        let message = error.to_string();
        assert!(
            message.contains("cyclic") || message.contains("alias"),
            "{message}"
        );
    }
}
