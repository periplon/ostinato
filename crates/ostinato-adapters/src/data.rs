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
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use bigdecimal::BigDecimal;
use num_bigint::BigInt;
use saphyr::{Mapping, Scalar, ScalarStyle, Yaml, YamlEmitter, YamlLoader};
use saphyr_parser::{BufferedInput, Event, Parser, Span, SpannedEventReceiver};
use serde::Deserializer;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
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

/// Maximum expanded JSON/YAML value size (aliases and merge keys included).
pub const MAX_EXPANDED_BYTES: usize = 32 * 1024 * 1024;

/// Maximum number of JSON values/nodes accepted in one document.
pub const MAX_JSON_NODES: usize = 1_000_000;

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
    #[error("{path}: input is not a regular file")]
    NotRegularFile { path: String },
    #[error("{path}: expanded value exceeds the {max}-byte budget")]
    ExpandedTooLarge { path: String, max: usize },
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

#[cfg(unix)]
fn open_regular_input(path: &Path) -> Result<(std::fs::File, u64), DataError> {
    use rustix::fs::{Mode, OFlags};
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| DataError::io(path, &std::io::Error::from(error)))?;
    let file = std::fs::File::from(fd);
    let metadata = file
        .metadata()
        .map_err(|error| DataError::io(path, &error))?;
    if !metadata.is_file() {
        return Err(DataError::NotRegularFile {
            path: path.display().to_string(),
        });
    }
    let flags = rustix::fs::fcntl_getfl(&file)
        .map_err(|error| DataError::io(path, &std::io::Error::from(error)))?;
    rustix::fs::fcntl_setfl(&file, flags & !OFlags::NONBLOCK)
        .map_err(|error| DataError::io(path, &std::io::Error::from(error)))?;
    Ok((file, metadata.len()))
}

#[cfg(not(unix))]
fn open_regular_input(path: &Path) -> Result<(std::fs::File, u64), DataError> {
    let file = std::fs::File::open(path).map_err(|error| DataError::io(path, &error))?;
    let metadata = file
        .metadata()
        .map_err(|error| DataError::io(path, &error))?;
    if !metadata.is_file() {
        return Err(DataError::NotRegularFile {
            path: path.display().to_string(),
        });
    }
    Ok((file, metadata.len()))
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
    let (file, length) = open_regular_input(path)?;
    if length > MAX_INPUT_BYTES as u64 {
        return Err(DataError::TooLarge {
            path: path.display().to_string(),
            bytes: length,
            max: MAX_INPUT_BYTES,
        });
    }
    let mut bytes = Vec::new();
    file.take((MAX_INPUT_BYTES as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| DataError::io(path, &error))?;
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
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match format {
        DataFormat::Json => {
            if text.trim().is_empty() {
                return Ok(vec![Json::Null]);
            }
            reject_reserved_json_key(text, origin)?;
            let counter = Rc::new(Cell::new(0));
            let mut deserializer = serde_json::Deserializer::from_str(text);
            let value = StrictSeed {
                counter: counter.clone(),
            }
            .deserialize(&mut deserializer)
            .map_err(|error| DataError::InvalidJson {
                path: origin.display().to_string(),
                message: error.to_string(),
            })?;
            deserializer.end().map_err(|error| DataError::InvalidJson {
                path: origin.display().to_string(),
                message: error.to_string(),
            })?;
            check_json_depth(&value.0, 0, origin)?;
            Ok(vec![value.0])
        }
        DataFormat::Yaml => {
            scan_yaml(text, origin)?;
            parse_yaml_documents(text, origin)
        }
    }
}

/// Read a single object document from a file.
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

/// Unify two JSON values using CUE-like rules.
///
/// Objects merge, equal-length lists unify element-wise, and equal scalars
/// unify. Different concrete values conflict.
///
/// # Errors
/// Returns [`DataError::Conflict`] for incompatible values or list lengths.
pub fn unify_values(left: &Json, right: &Json, origin: &Path) -> Result<Json, DataError> {
    merge_json(left, right, "$", origin)
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
    let normalized = normalize_number_signs(value);
    let yaml = json_to_yaml(&normalized);
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
    let normalized = normalize_number_signs(value);
    let mut out = String::new();
    write_pretty_json(&normalized, 0, &mut out)?;
    out.push('\n');
    Ok(out)
}

/// Format a number token the way CUE's `apd.Decimal.String` does.  serde_json
/// normalizes exponent markers to lowercase when it parses a number, so this
/// restores the uppercase `E` used by CUE's JSON export.
fn cue_number_text(number: &Number) -> String {
    let text = number.to_string();
    if text.contains('e') {
        text.replace('e', "E")
    } else {
        text
    }
}

fn write_json_string(value: &str, out: &mut String) -> Result<(), DataError> {
    let encoded = serde_json::to_string(value).map_err(|error| DataError::Encode {
        path: "<output>".to_owned(),
        message: error.to_string(),
    })?;
    out.push_str(&encoded);
    Ok(())
}

fn write_pretty_json(value: &Json, depth: usize, out: &mut String) -> Result<(), DataError> {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Json::Number(number) => out.push_str(&cue_number_text(number)),
        Json::String(value) => write_json_string(value, out)?,
        Json::Array(values) => {
            if values.is_empty() {
                out.push_str("[]");
                return Ok(());
            }
            out.push_str("[\n");
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push_str(",\n");
                }
                push_indent(out, depth + 1);
                write_pretty_json(value, depth + 1, out)?;
            }
            out.push('\n');
            push_indent(out, depth);
            out.push(']');
        }
        Json::Object(fields) => {
            if fields.is_empty() {
                out.push_str("{}");
                return Ok(());
            }
            out.push_str("{\n");
            for (index, (key, value)) in fields.iter().enumerate() {
                if index > 0 {
                    out.push_str(",\n");
                }
                push_indent(out, depth + 1);
                write_json_string(key, out)?;
                out.push_str(": ");
                write_pretty_json(value, depth + 1, out)?;
            }
            out.push('\n');
            push_indent(out, depth);
            out.push('}');
        }
    }
    Ok(())
}

fn push_indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
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
        let mut budget = MAX_EXPANDED_BYTES;
        out.push(yaml_to_json(document, 0, "$", origin, &mut budget)?);
    }
    Ok(out)
}

/// Walk the event stream iteratively before handing the text to the recursive
/// loader. This rejects excessive nesting and alias trees whose estimated clone
/// size exceeds the byte budget without ever recursing, so a deeply nested
/// document cannot overflow the stack and a large scalar referenced by many
/// aliases cannot allocate gigabytes of cloned nodes first.
fn scan_yaml(text: &str, origin: &Path) -> Result<(), DataError> {
    let mut parser = Parser::new(BufferedInput::new(text.chars()));
    let mut receiver = AliasSizeReceiver::default();
    let mut depth = 0usize;
    while let Some(event) = parser.next_event() {
        let (event, span) = event.map_err(|error| DataError::InvalidYaml {
            path: origin.display().to_string(),
            message: error.to_string(),
        })?;
        match &event {
            Event::StreamEnd => break,
            Event::SequenceStart(..) | Event::MappingStart(..) => {
                depth = depth.saturating_add(1);
                if depth > MAX_DEPTH {
                    return Err(DataError::TooDeep {
                        path: origin.display().to_string(),
                        max: MAX_DEPTH,
                    });
                }
            }
            Event::SequenceEnd | Event::MappingEnd => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        receiver.on_event(event, span);
        if receiver.violation.is_some() {
            return Err(DataError::ExpandedTooLarge {
                path: origin.display().to_string(),
                max: MAX_EXPANDED_BYTES,
            });
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
struct AliasSizeReceiver {
    stack: Vec<(Option<usize>, usize)>,
    anchors: BTreeMap<usize, usize>,
    total: usize,
    /// Running total of `total` plus every frame on the stack. Updated
    /// incrementally so every event stays O(1) instead of rescanning the stack.
    current_size: usize,
    violation: Option<String>,
}

impl AliasSizeReceiver {
    fn add(&mut self, bytes: usize) {
        self.current_size = self.current_size.saturating_add(bytes);
        if let Some((_, current)) = self.stack.last_mut() {
            *current = current.saturating_add(bytes);
        } else {
            self.total = self.total.saturating_add(bytes);
        }
        self.check_budget();
    }

    /// Account for a finished frame in its parent. The frame bytes were already
    /// counted when they were added, so `current_size` is unchanged.
    fn finish(&mut self, bytes: usize) {
        if let Some((_, current)) = self.stack.last_mut() {
            *current = current.saturating_add(bytes);
        } else {
            self.total = self.total.saturating_add(bytes);
        }
        self.check_budget();
    }

    fn push(&mut self, anchor: usize) {
        self.current_size = self.current_size.saturating_add(16);
        self.stack.push(((anchor > 0).then_some(anchor), 16));
        self.check_budget();
    }

    fn check_budget(&mut self) {
        if self.current_size > MAX_EXPANDED_BYTES && self.violation.is_none() {
            self.violation = Some(format!(
                "expanded YAML value exceeds the {MAX_EXPANDED_BYTES}-byte budget"
            ));
        }
    }

    fn on_event<'input>(&mut self, event: Event<'input>, _span: Span) {
        match &event {
            Event::Scalar(text, _, anchor, _) => {
                let size = text.len().saturating_add(16);
                self.add(size);
                if *anchor > 0 {
                    self.anchors.insert(*anchor, size);
                }
            }
            Event::SequenceStart(anchor, _) | Event::MappingStart(anchor, _) => {
                self.push(*anchor);
            }
            Event::SequenceEnd | Event::MappingEnd => {
                if let Some((anchor, bytes)) = self.stack.pop() {
                    self.finish(bytes);
                    if let Some(anchor) = anchor {
                        self.anchors.insert(anchor, bytes);
                    }
                }
            }
            Event::Alias(anchor) => {
                let size = self.anchors.get(anchor).copied().unwrap_or(0);
                self.add(size.saturating_add(16));
            }
            _ => {}
        }
    }
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

fn yaml_to_json(
    node: &Yaml<'_>,
    depth: usize,
    at: &str,
    origin: &Path,
    budget: &mut usize,
) -> Result<Json, DataError> {
    if depth > MAX_DEPTH {
        return Err(DataError::TooDeep {
            path: origin.display().to_string(),
            max: MAX_DEPTH,
        });
    }
    consume(budget, 16, origin)?;
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
        Yaml::Value(Scalar::String(value)) => {
            consume(budget, value.len().saturating_add(8), origin)?;
            Ok(Json::String(value.to_string()))
        }
        Yaml::Representation(value, style, tag) => {
            let resolved = resolve_yaml_scalar(value, *style, tag.as_deref(), origin)?;
            consume(budget, json_size(&resolved), origin)?;
            Ok(resolved)
        }
        Yaml::Sequence(sequence) => {
            let mut out = Vec::with_capacity(sequence.len());
            for (index, item) in sequence.iter().enumerate() {
                out.push(yaml_to_json(
                    item,
                    depth + 1,
                    &format!("{at}[{index}]"),
                    origin,
                    budget,
                )?);
            }
            Ok(Json::Array(out))
        }
        Yaml::Mapping(mapping) => yaml_mapping_to_json(mapping, depth, at, origin, budget),
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
    budget: &mut usize,
) -> Result<Json, DataError> {
    // Merge keys (`<<`) are resolved after explicit fields are known. Per the
    // YAML merge-key specification, explicit keys win and earlier merged maps
    // take precedence over later ones. Only a plain untagged `<<` (or the core
    // `!!merge` tag) is a merge key; a quoted `"<<"` stays a literal key.
    let mut explicit = Vec::with_capacity(mapping.len());
    let mut merged: Vec<(String, Json)> = Vec::new();
    let mut seen_explicit = BTreeSet::new();
    for (key, value) in mapping {
        let key = yaml_key(key, origin)?;
        if key.merge {
            merge_yaml_mapping(value, depth, at, origin, budget, &mut merged)?;
            continue;
        }
        consume(budget, key.text.len().saturating_add(8), origin)?;
        if !seen_explicit.insert(key.text.clone()) {
            return Err(DataError::DuplicateKey {
                path: origin.display().to_string(),
                key: key.text,
            });
        }
        explicit.push((key.text, value));
    }
    let mut fields = Map::new();
    for (key, value) in merged {
        consume(budget, json_size(&value), origin)?;
        fields.entry(key).or_insert(value);
    }
    for (key, value) in explicit {
        let child_at = format!("{at}.{key}");
        let converted = yaml_to_json(value, depth + 1, &child_at, origin, budget)?;
        fields.insert(key, converted);
    }
    Ok(Json::Object(fields))
}

fn merge_yaml_mapping(
    value: &Yaml<'_>,
    depth: usize,
    at: &str,
    origin: &Path,
    budget: &mut usize,
    merged: &mut Vec<(String, Json)>,
) -> Result<(), DataError> {
    match value {
        Yaml::Mapping(mapping) => {
            let converted = yaml_mapping_to_json(mapping, depth + 1, at, origin, budget)?;
            consume(budget, json_size(&converted), origin)?;
            if let Json::Object(object) = converted {
                merged.extend(object);
            }
            Ok(())
        }
        Yaml::Sequence(sequence) => {
            for item in sequence {
                merge_yaml_mapping(item, depth + 1, at, origin, budget, merged)?;
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
            // CUE's explicit `!!bool` only recognizes the canonical true
            // spellings; every other token resolves to false.
            "bool" => Ok(Json::Bool(matches!(text, "true" | "True" | "TRUE"))),
            "int" => {
                parse_yaml_integer(text, origin, true)?.ok_or_else(|| DataError::InvalidYaml {
                    path: origin.display().to_string(),
                    message: format!("invalid `!!int` value `{text}`"),
                })
            }
            "float" => parse_yaml_float(text, origin, true),
            "merge" => Ok(Json::String("<<".to_owned())),
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
    if looks_like_yaml_integer(text) {
        // CUE keeps base-prefixed integers that do not fit in u64 (and other
        // spellings that are not valid YAML integers) as strings rather than
        // failing the document.
        return Ok(parse_yaml_integer(text, origin, false)?
            .unwrap_or_else(|| Json::String(text.to_owned())));
    }
    if looks_like_yaml_float(text) {
        return parse_yaml_float(text, origin, false);
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

fn looks_like_yaml_integer(text: &str) -> bool {
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    if unsigned.is_empty() {
        return false;
    }
    if let Some(rest) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        return !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '_');
    }
    if let Some(rest) = unsigned
        .strip_prefix("0o")
        .or_else(|| unsigned.strip_prefix("0O"))
        .or_else(|| unsigned.strip_prefix("0b"))
        .or_else(|| unsigned.strip_prefix("0B"))
    {
        return !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit() || ch == '_');
    }
    unsigned
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit())
        && unsigned.chars().all(|ch| ch.is_ascii_digit() || ch == '_')
}

fn parse_yaml_integer(
    text: &str,
    origin: &Path,
    explicit: bool,
) -> Result<Option<Json>, DataError> {
    let signed = matches!(text.as_bytes().first(), Some(b'-' | b'+'));
    let negative = text.starts_with('-');
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    if unsigned.is_empty() {
        return Ok(None);
    }
    let (mut radix, body) = if let Some(rest) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        (16, rest)
    } else if let Some(rest) = unsigned.strip_prefix("0o") {
        (8, rest)
    } else if let Some(rest) = unsigned.strip_prefix("0b") {
        (2, rest)
    } else if unsigned.starts_with("0B") || unsigned.starts_with("0O") {
        // CUE rejects uppercase binary/octal prefixes, including as tagged
        // values, rather than treating them as strings.
        return Err(illegal_number(text, origin));
    } else {
        (10, unsigned)
    };
    // YAML 1.1 legacy octal: an unsigned leading-zero decimal with no
    // separators is octal when every digit is 0-7. Invalid octal is a string
    // for plain scalars and an error for `!!int`; signed octal is always an
    // error.
    if radix == 10 && body.len() > 1 && body.starts_with('0') && !body.contains('_') {
        if body.chars().all(|ch| matches!(ch, '0'..='7')) {
            if signed {
                return Err(illegal_number(text, origin));
            }
            radix = 8;
        } else if explicit {
            return Err(illegal_number(text, origin));
        } else {
            return Ok(None);
        }
    }
    let Some(digits) = clean_integer_digits(body, radix, text, origin)? else {
        return Ok(None);
    };
    if explicit {
        let Some(magnitude) = BigInt::parse_bytes(digits.as_bytes(), radix) else {
            return Ok(None);
        };
        let value = if negative { -magnitude } else { magnitude };
        return Ok(value.to_string().parse::<Number>().ok().map(Json::Number));
    }
    if radix == 10 {
        // Keep arbitrary-precision decimal integers, matching the data-file
        // policy elsewhere in this adapter.
        let Some(magnitude) = BigInt::parse_bytes(digits.as_bytes(), 10) else {
            return Ok(None);
        };
        let value = if negative { -magnitude } else { magnitude };
        return Ok(value.to_string().parse::<Number>().ok().map(Json::Number));
    }
    // Base-prefixed plain scalars stay strings when they do not fit in u64
    // (positive) or i64 (negative), matching CUE's YAML decoder.
    let Some(magnitude) = u128::from_str_radix(&digits, radix).ok() else {
        return Ok(None);
    };
    let signed_value = if negative {
        if magnitude > (1_u128 << 63) {
            return Ok(None);
        }
        format!("-{magnitude}")
    } else {
        if magnitude > u128::from(u64::MAX) {
            return Ok(None);
        }
        magnitude.to_string()
    };
    Ok(signed_value.parse::<Number>().ok().map(Json::Number))
}

/// Render an unsigned legacy-octal key the way CUE does: `010` -> `0o10`,
/// `000` -> `0o00`. Signed octal never reaches key rendering (it is an error).
fn legacy_octal_key_text(value: &str) -> Option<String> {
    if value.starts_with(['-', '+']) {
        return None;
    }
    if value.len() > 1
        && value.starts_with('0')
        && !value.contains('_')
        && value.chars().all(|ch| matches!(ch, '0'..='7'))
    {
        Some(format!("0o{}", &value[1..]))
    } else {
        None
    }
}

fn clean_integer_digits(
    body: &str,
    radix: u32,
    text: &str,
    origin: &Path,
) -> Result<Option<String>, DataError> {
    let digit = |ch: char| -> bool {
        match radix {
            16 => ch.is_ascii_hexdigit(),
            8 => matches!(ch, '0'..='7'),
            2 => matches!(ch, '0' | '1'),
            _ => ch.is_ascii_digit(),
        }
    };
    let mut digits = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '_' {
            let next_is_digit = chars.peek().copied().is_some_and(digit);
            if next_is_digit {
                continue;
            }
            // `0x_`/`0b_` are not integers but are valid strings for plain
            // scalars. Once a digit has been seen, a dangling underscore is an
            // illegal number.
            if digits.is_empty() && radix != 10 {
                return Ok(None);
            }
            return Err(illegal_number(text, origin));
        }
        if digit(ch) {
            digits.push(ch);
        } else {
            return Ok(None);
        }
    }
    if digits.is_empty() {
        return Ok(None);
    }
    if radix == 10 && body.len() > 1 && body.starts_with('0') && body.contains('_') {
        return Err(illegal_number(text, origin));
    }
    Ok(Some(digits))
}

fn looks_like_yaml_float(text: &str) -> bool {
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
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    !unsigned.is_empty()
        && unsigned.contains(['.', 'e', 'E'])
        && unsigned
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit() || ch == '.')
}

fn yaml_float_base_prefixed(unsigned: &str) -> bool {
    ["0x", "0X", "0b", "0B", "0o", "0O"]
        .iter()
        .any(|prefix| unsigned.starts_with(prefix))
}

fn float_significand_has_digit(unsigned: &str) -> bool {
    let end = unsigned.find(['e', 'E']).unwrap_or(unsigned.len());
    unsigned[..end].chars().any(|ch| ch.is_ascii_digit())
}

fn number_token_is_zero(text: &str) -> bool {
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    let mantissa = unsigned.split(['e', 'E']).next().unwrap_or(unsigned);
    mantissa.chars().all(|ch| ch == '0' || ch == '.') && mantissa.chars().any(|ch| ch == '0')
}

fn canonical_zero_number(number: &Number) -> Option<Number> {
    let text = number.to_string();
    if !number_token_is_zero(&text) {
        return None;
    }
    if text.contains(['.', 'e', 'E']) {
        Number::from_f64(0.0)
    } else {
        Some(Number::from(0))
    }
}

fn parse_yaml_float(text: &str, origin: &Path, explicit: bool) -> Result<Json, DataError> {
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
    let signed = matches!(text.as_bytes().first(), Some(b'-' | b'+'));
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    if yaml_float_base_prefixed(unsigned) {
        return if explicit {
            Err(illegal_number(text, origin))
        } else {
            Ok(Json::String(text.to_owned()))
        };
    }
    // A bare dot has no significand digit and stays a string.
    if !float_significand_has_digit(unsigned) {
        return if explicit {
            Err(illegal_number(text, origin))
        } else {
            Ok(Json::String(text.to_owned()))
        };
    }
    // Unsigned `._5` is a string; signed `-._5`/`+._5` resolve to +/-0.5
    // through the ordinary underscore stripping path.
    if !signed && unsigned.starts_with("._") && !explicit {
        return Ok(Json::String(text.to_owned()));
    }
    // Only an unsigned raw `0` followed by a decimal digit is an illegal
    // leading-zero float. Signed forms normalize (`-01.5` -> -1.5) and
    // `0_0.1` is a valid separated digit.
    if !signed
        && unsigned.as_bytes().first() == Some(&b'0')
        && unsigned
            .as_bytes()
            .get(1)
            .is_some_and(|byte| byte.is_ascii_digit())
    {
        return Err(DataError::InvalidYaml {
            path: origin.display().to_string(),
            message: format!("cannot decode `{text}` as a number: illegal number"),
        });
    }
    let cleaned = match strip_float_underscores(text, origin) {
        Ok(cleaned) => cleaned,
        Err(error) => {
            if explicit {
                return Err(error);
            }
            // An illegal underscore that leaves no number after removal is a
            // string (`1e_`); otherwise it stays a hard error (`1_`, `1_.5`).
            let fallback: String = text.chars().filter(|ch| *ch != '_').collect();
            if normalized_float(&fallback).is_none() {
                return Ok(Json::String(text.to_owned()));
            }
            return Err(error);
        }
    };
    let Some(mut normalized) = normalized_float(&cleaned) else {
        return if explicit {
            Err(illegal_number(text, origin))
        } else {
            Ok(Json::String(text.to_owned()))
        };
    };
    if !explicit {
        // CUE rejects plain exponents that overflow f64 lexical parsing and
        // keeps them as strings (`1e400`, `1e309`). Explicit `!!float` values
        // stay numbers and preserve arbitrary precision.
        match normalized.parse::<f64>() {
            Ok(value) if value.is_finite() => {}
            _ => return Ok(Json::String(text.to_owned())),
        }
    }
    if unsigned.ends_with('.') && normalized.ends_with(".0") {
        normalized.truncate(normalized.len() - 2);
    }
    // CUE canonicalizes negative zero to zero while preserving scale.
    if normalized.starts_with('-') && number_token_is_zero(&normalized) {
        normalized.remove(0);
    }
    normalized
        .parse::<Number>()
        .map(Json::Number)
        .map_err(|_| illegal_number(text, origin))
}

fn strip_float_underscores(text: &str, origin: &Path) -> Result<String, DataError> {
    let chars: Vec<char> = text.chars().collect();
    let digit = |ch: char| ch.is_ascii_digit();
    let mut out = String::with_capacity(text.len());
    for (index, ch) in chars.iter().enumerate() {
        if *ch != '_' {
            out.push(*ch);
            continue;
        }
        let previous = index
            .checked_sub(1)
            .and_then(|index| chars.get(index))
            .copied();
        let next = chars.get(index + 1).copied();
        match (previous, next) {
            (Some(previous), Some(next)) if digit(previous) && digit(next) => {}
            (Some('.'), Some(next)) if digit(next) => {}
            (Some('e' | 'E'), Some(next)) if digit(next) => {}
            (Some('+' | '-'), Some(next))
                if digit(next)
                    && chars
                        .get(index.wrapping_sub(2))
                        .is_some_and(|ch| matches!(ch, 'e' | 'E')) => {}
            _ => return Err(illegal_number(text, origin)),
        }
    }
    Ok(out)
}

fn illegal_number(text: &str, origin: &Path) -> DataError {
    DataError::InvalidYaml {
        path: origin.display().to_string(),
        message: format!("cannot decode `{text}` as a number: illegal '_' in number"),
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct YamlKey {
    text: String,
    merge: bool,
}

fn yaml_key(node: &Yaml<'_>, origin: &Path) -> Result<YamlKey, DataError> {
    match node {
        Yaml::Value(Scalar::String(value)) => Ok(YamlKey {
            text: value.to_string(),
            merge: false,
        }),
        Yaml::Value(Scalar::Boolean(value)) => Ok(YamlKey {
            text: value.to_string(),
            merge: false,
        }),
        Yaml::Value(Scalar::Integer(value)) => Ok(YamlKey {
            text: value.to_string(),
            merge: false,
        }),
        Yaml::Value(Scalar::FloatingPoint(value)) => Ok(YamlKey {
            text: value.into_inner().to_string(),
            merge: false,
        }),
        Yaml::Value(Scalar::Null) => Ok(YamlKey {
            text: "null".to_owned(),
            merge: false,
        }),
        Yaml::Representation(value, style, tag) => {
            yaml_representation_key(value, *style, tag.as_deref(), origin)
        }
        Yaml::Tagged(tag, _) => Err(DataError::UnsupportedTag {
            path: origin.display().to_string(),
            tag: tag.to_string(),
        }),
        _ => Err(DataError::ComplexKey {
            path: origin.display().to_string(),
        }),
    }
}

fn yaml_representation_key(
    value: &str,
    style: ScalarStyle,
    tag: Option<&saphyr_parser::Tag>,
    origin: &Path,
) -> Result<YamlKey, DataError> {
    if let Some(tag) = tag {
        if !tag.is_yaml_core_schema() {
            return Err(DataError::UnsupportedTag {
                path: origin.display().to_string(),
                tag: tag.to_string(),
            });
        }
        return match tag.suffix.as_str() {
            "str" => Ok(YamlKey {
                text: value.to_owned(),
                merge: false,
            }),
            // CUE keeps the raw spelling of explicitly tagged numeric keys
            // (`!!int 0x10` → key `"0x10"`, `!!float 1e2` → key `"1e2"`),
            // except legacy octal integers, which render as `0oNN`.
            "int" => {
                if parse_yaml_integer(value, origin, true)?.is_none() {
                    return Err(DataError::InvalidYaml {
                        path: origin.display().to_string(),
                        message: format!("invalid `!!int` value `{value}`"),
                    });
                }
                Ok(YamlKey {
                    text: legacy_octal_key_text(value).unwrap_or_else(|| value.to_owned()),
                    merge: false,
                })
            }
            "float" => {
                // Validate the tag, but CUE preserves the raw float spelling in
                // keys.
                let _ = parse_yaml_float(value, origin, true)?;
                Ok(YamlKey {
                    text: value.to_owned(),
                    merge: false,
                })
            }
            "bool" => Ok(YamlKey {
                text: matches!(value, "true" | "True" | "TRUE").to_string(),
                merge: false,
            }),
            "null" => Ok(YamlKey {
                text: "null".to_owned(),
                merge: false,
            }),
            "merge" => Ok(YamlKey {
                text: "<<".to_owned(),
                merge: true,
            }),
            _ => Err(DataError::UnsupportedTag {
                path: origin.display().to_string(),
                tag: tag.to_string(),
            }),
        };
    }
    if style != ScalarStyle::Plain {
        return Ok(YamlKey {
            text: value.to_owned(),
            merge: false,
        });
    }
    if value.is_empty() || matches!(value, "~" | "null" | "Null" | "NULL") {
        return Ok(YamlKey {
            text: "null".to_owned(),
            merge: false,
        });
    }
    if let Some(value) = parse_yaml_bool(value) {
        return Ok(YamlKey {
            text: value.to_string(),
            merge: false,
        });
    }
    if looks_like_yaml_integer(value) {
        let parsed = parse_yaml_integer(value, origin, false)?;
        if parsed.is_some()
            && let Some(text) = legacy_octal_key_text(value)
        {
            return Ok(YamlKey { text, merge: false });
        }
        return Ok(YamlKey {
            text: value.to_owned(),
            merge: false,
        });
    }
    if looks_like_yaml_float(value) {
        // Resolve exactly like a scalar so invalid numbers still fail, but
        // CUE preserves the raw spelling in keys.
        let _ = parse_yaml_float(value, origin, false)?;
    }
    Ok(YamlKey {
        text: value.to_owned(),
        merge: value == "<<",
    })
}

/// Drop the sign from zero number tokens before encoding. CUE prints
/// `-0.0` as `0.0` while preserving the scale/exponent.
fn normalize_number_signs(value: &Json) -> Json {
    match value {
        Json::Number(number) => {
            let text = number.to_string();
            if text.starts_with('-') && number_token_is_zero(&text) {
                return text[1..]
                    .parse::<Number>()
                    .map_or_else(|_| Json::Number(number.clone()), Json::Number);
            }
            Json::Number(number.clone())
        }
        Json::Array(values) => Json::Array(values.iter().map(normalize_number_signs).collect()),
        Json::Object(fields) => Json::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), normalize_number_signs(value)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn json_to_yaml(value: &Json) -> Yaml<'static> {
    match value {
        Json::Null => Yaml::Representation(Cow::Owned("null".to_owned()), ScalarStyle::Plain, None),
        Json::Bool(value) => Yaml::Value(Scalar::Boolean(*value)),
        Json::Number(number) => Yaml::Representation(
            Cow::Owned(cue_number_text(number)),
            ScalarStyle::Plain,
            None,
        ),
        Json::String(value) => yaml_quoted(value),
        Json::Array(values) => Yaml::Sequence(values.iter().map(json_to_yaml).collect()),
        Json::Object(fields) => {
            let mut mapping = Mapping::default();
            for (key, value) in fields {
                mapping.insert(yaml_quoted(key), json_to_yaml(value));
            }
            Yaml::Mapping(mapping)
        }
    }
}

fn yaml_quoted(value: &str) -> Yaml<'static> {
    Yaml::Representation(
        Cow::Owned(escape_yaml_string(value)),
        ScalarStyle::DoubleQuoted,
        None,
    )
}

fn escape_yaml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\u{00}'..='\u{1f}' | '\u{7f}' => out.push_str(&format!("\\u{:04X}", u32::from(ch))),
            '\u{80}'..='\u{9f}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{feff}'
            | '\u{fffe}'
            | '\u{ffff}'
            | '\u{fdd0}'..='\u{fdef}' => {
                out.push_str(&format!("\\u{:04X}", u32::from(ch)));
            }
            _ if (u32::from(ch) & 0xffff == 0xfffe) || (u32::from(ch) & 0xffff == 0xffff) => {
                out.push_str(&format!("\\U{:08X}", u32::from(ch)));
            }
            _ => out.push(ch),
        }
    }
    out
}

fn consume(budget: &mut usize, bytes: usize, origin: &Path) -> Result<(), DataError> {
    if bytes > *budget {
        return Err(DataError::ExpandedTooLarge {
            path: origin.display().to_string(),
            max: MAX_EXPANDED_BYTES,
        });
    }
    *budget -= bytes;
    Ok(())
}

fn json_size(value: &Json) -> usize {
    match value {
        Json::Null | Json::Bool(_) => 8,
        Json::Number(_) => 16,
        Json::String(value) => value.len() + 8,
        Json::Array(values) => 16 + values.iter().map(json_size).sum::<usize>(),
        Json::Object(fields) => {
            16 + fields
                .iter()
                .map(|(key, value)| key.len() + json_size(value))
                .sum::<usize>()
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
        (Json::Number(left), Json::Number(right)) => {
            let conflict = || DataError::Conflict {
                path: origin.display().to_string(),
                at: at.to_owned(),
            };
            let left_text = left.to_string();
            let right_text = right.to_string();
            // CUE does not unify an int token with a float token, even when the
            // numeric values are equal.
            let left_float = left_text.contains(['.', 'e', 'E']);
            let right_float = right_text.contains(['.', 'e', 'E']);
            if left_float != right_float {
                return Err(conflict());
            }
            // Both sides are zero: canonicalize the sign away so `0.0` vs
            // `-0.0` is order-independent.
            if let Some(left_zero) = canonical_zero_number(left)
                && canonical_zero_number(right).is_some()
            {
                return Ok(Json::Number(left_zero));
            }
            // Identical lexical numbers always unify, including exponents that
            // BigDecimal cannot represent.
            if left_text == right_text {
                return Ok(Json::Number(left.clone()));
            }
            let (Ok(left_value), Ok(right_value)) = (
                left_text.parse::<BigDecimal>(),
                right_text.parse::<BigDecimal>(),
            ) else {
                return Err(conflict());
            };
            if left_value == right_value {
                Ok(Json::Number(
                    canonical_zero_number(left).unwrap_or_else(|| left.clone()),
                ))
            } else {
                Err(conflict())
            }
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

struct StrictSeed {
    counter: Rc<Cell<usize>>,
}

impl<'de> DeserializeSeed<'de> for StrictSeed {
    type Value = StrictJson;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictVisitor {
            counter: self.counter,
        })
    }
}

struct StrictVisitor {
    counter: Rc<Cell<usize>>,
}

/// serde_json's arbitrary-precision number protocol sends numbers through a
/// single-entry map with this key. The workspace always enables the feature;
/// the constant is harmless when it is disabled because the key never appears.
const NUMBER_TOKEN: &str = "$serde_json::private::Number";

impl StrictVisitor {
    fn bump<E: de::Error>(&self) -> Result<(), E> {
        let next = self.counter.get().saturating_add(1);
        if next > MAX_JSON_NODES {
            return Err(E::custom(format!(
                "JSON node count exceeds the maximum of {MAX_JSON_NODES}"
            )));
        }
        self.counter.set(next);
        Ok(())
    }
}

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = StrictJson;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.bump()?;
        Ok(StrictJson(Json::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.bump()?;
        Ok(StrictJson(Json::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.bump()?;
        Ok(StrictJson(Json::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.bump()?;
        let Some(number) = Number::from_f64(value) else {
            return Err(E::custom("non-finite JSON number"));
        };
        Ok(StrictJson(Json::Number(number)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.bump()?;
        Ok(StrictJson(Json::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.bump()?;
        Ok(StrictJson(Json::String(value)))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.bump()?;
        Ok(StrictJson(Json::Null))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.bump()?;
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
        self.bump()?;
        let mut out = Vec::new();
        while let Some(item) = sequence.next_element_seed(StrictSeed {
            counter: self.counter.clone(),
        })? {
            out.push(item.0);
        }
        Ok(StrictJson(Json::Array(out)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        self.bump()?;
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
        let first_value: StrictJson = map.next_value_seed(StrictSeed {
            counter: self.counter.clone(),
        })?;
        let mut out = Map::new();
        out.insert(first_key.clone(), first_value.0);
        while let Some(key) = map.next_key::<String>()? {
            if out.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate key `{key}`")));
            }
            let value: StrictJson = map.next_value_seed(StrictSeed {
                counter: self.counter.clone(),
            })?;
            out.insert(key, value.0);
        }
        Ok(StrictJson(Json::Object(out)))
    }
}

/// Reject the internal arbitrary-precision number key before deserialization.
///
/// serde_json uses `{"$serde_json::private::Number": "..."}` internally for
/// number tokens. A user object with that key would otherwise be reinterpreted
/// as a number. Scanning raw JSON distinguishes real number tokens (not
/// objects) from user objects and gives a clear error instead of corruption.
fn reject_reserved_json_key(text: &str, origin: &Path) -> Result<(), DataError> {
    #[derive(Clone, Copy)]
    enum Context {
        Object { expect_key: bool },
        Array,
    }
    let chars: Vec<char> = text.chars().collect();
    let mut stack: Vec<Context> = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '{' => {
                stack.push(Context::Object { expect_key: true });
                index += 1;
            }
            '[' => {
                stack.push(Context::Array);
                index += 1;
            }
            '}' | ']' => {
                stack.pop();
                index += 1;
            }
            ',' => {
                if let Some(Context::Object { expect_key }) = stack.last_mut() {
                    *expect_key = true;
                }
                index += 1;
            }
            '"' => {
                let start = index;
                index += 1;
                let mut escaped = false;
                while index < chars.len() {
                    let ch = chars[index];
                    index += 1;
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == '"' {
                        break;
                    }
                }
                let literal: String = chars[start..index].iter().collect();
                let decoded: String = serde_json::from_str(&literal).unwrap_or(literal);
                if let Some(Context::Object { expect_key }) = stack.last_mut()
                    && *expect_key
                {
                    if decoded == NUMBER_TOKEN {
                        return Err(DataError::InvalidJson {
                            path: origin.display().to_string(),
                            message: format!(
                                "reserved key `{NUMBER_TOKEN}` is not supported in user JSON"
                            ),
                        });
                    }
                    *expect_key = false;
                }
            }
            ch if ch.is_whitespace() || ch == ':' => index += 1,
            _ => {
                index += 1;
            }
        }
    }
    Ok(())
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
        let message = error.to_string();
        assert!(
            message.contains("billion-laughs")
                || message.contains("expanded")
                || message.contains("budget"),
            "{message}"
        );
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
    fn deeply_nested_yaml_is_rejected_in_linear_time() {
        for depth in [MAX_DEPTH + 1, 5_000, 20_000, 200_000, 250_000] {
            let text = format!("a:\n  {}1", "- ".repeat(depth));
            let start = std::time::Instant::now();
            let error =
                parse_documents(DataFormat::Yaml, &text, Path::new("t")).expect_err("deep nesting");
            assert!(
                matches!(error, DataError::TooDeep { .. }),
                "{depth}: {error}"
            );
            assert!(
                start.elapsed() < std::time::Duration::from_secs(2),
                "depth {depth} took {:?}",
                start.elapsed()
            );
        }
    }

    #[test]
    fn zero_with_huge_exponent_does_not_materialize_zeros() {
        for exponent in ["100000000", "9223372036854775807"] {
            let text = format!("a: 0e-{exponent}\n---\na: 0e-{exponent}\n");
            let documents = parse(DataFormat::Yaml, &text);
            let merged = unify_objects(&documents, Path::new("t")).expect("merge");
            let json = to_json(&merged).expect("json");
            assert!(
                json.len() < 256,
                "exponent {exponent} produced {} bytes",
                json.len()
            );
            assert!(json.contains("\"a\": 0"), "{json}");
        }
    }

    #[test]
    fn number_merge_never_falls_back_to_zero() {
        let huge: Json = serde_json::from_str("1e-999999999999999999999").expect("number");
        let merged = unify_values(
            &serde_json::json!({"a": huge.clone()}),
            &serde_json::json!({"a": huge.clone()}),
            Path::new("t"),
        )
        .expect("identical tokens");
        assert_eq!(merged["a"], huge);
        let different: Json = serde_json::from_str("2e-999999999999999999999").expect("number");
        let error = unify_values(
            &serde_json::json!({"a": huge.clone()}),
            &serde_json::json!({"a": different}),
            Path::new("t"),
        )
        .expect_err("unrepresentable conflict");
        assert!(matches!(error, DataError::Conflict { .. }), "{error}");
        let error = unify_values(
            &serde_json::json!({"a": 1}),
            &serde_json::json!({"a": 1.0}),
            Path::new("t"),
        )
        .expect_err("int/float conflict");
        assert!(matches!(error, DataError::Conflict { .. }), "{error}");
    }

    #[test]
    fn negative_zero_merge_and_encoding_are_canonical() {
        for (left, right) in [
            ("0.0", "-0.0"),
            ("-0.0", "0.0"),
            ("-0.0", "-0.0"),
            ("0", "-0"),
            ("-0", "0"),
        ] {
            let left: Json = serde_json::from_str(left).expect("number");
            let right: Json = serde_json::from_str(right).expect("number");
            let merged = unify_values(
                &serde_json::json!({"a": left}),
                &serde_json::json!({"a": right}),
                Path::new("t"),
            )
            .expect("zero merge");
            let text = merged["a"].to_string();
            assert!(!text.starts_with('-'), "{text}");
            assert!(text == "0" || text == "0.0", "{text}");
        }
        let value: Json = serde_json::from_str(r#"{"a":-0.0,"b":[-0.0]}"#).expect("json");
        let json = to_json(&value).expect("json");
        assert!(!json.contains("-0"), "{json}");
        let yaml = to_yaml(&value).expect("yaml");
        assert!(!yaml.contains("-0"), "{yaml}");
    }

    #[test]
    fn explicit_numeric_and_bool_tags_match_cue() {
        let docs = parse(
            DataFormat::Yaml,
            "f: !!float 1e400\nu: !!float ._5\nb: !!int 0x10000000000000000\n\
             h: !!int 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF\n",
        );
        assert!(docs[0]["f"].to_string().contains("400"), "{}", docs[0]["f"]);
        assert_eq!(docs[0]["u"].to_string(), "0.5");
        assert_eq!(docs[0]["b"].to_string(), "18446744073709551616");
        assert_eq!(
            docs[0]["h"].to_string(),
            "5444517870735015415413993718908291383295"
        );
        for bad in [
            "!!int 1e2",
            "!!int 1.0",
            "!!int 0b12",
            "!!int 0b_2",
            "!!int 0x_",
        ] {
            let error = parse_documents(DataFormat::Yaml, &format!("a: {bad}\n"), Path::new("t"))
                .expect_err(bad);
            assert!(error.to_string().contains("!!int"), "{bad}: {error}");
        }
        let docs = parse(
            DataFormat::Yaml,
            "a: !!bool yes\nb: !!bool 1\nc: !!bool true\nd: !!bool TRUE\n",
        );
        assert_eq!(docs[0]["a"], Json::Bool(false));
        assert_eq!(docs[0]["b"], Json::Bool(false));
        assert_eq!(docs[0]["c"], Json::Bool(true));
        assert_eq!(docs[0]["d"], Json::Bool(true));
    }

    #[test]
    fn plain_base_integers_and_leading_zeros_match_cue() {
        let text = "a: 0x10000000000000000\nb: 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF\n\
                    c: 0x_\nd: 0b_\ne: 0b12\nf: 0b_2\n";
        let docs = parse(DataFormat::Yaml, text);
        for key in ["a", "b", "c", "d", "e", "f"] {
            assert!(docs[0][key].is_string(), "{key}: {}", docs[0][key]);
        }
        assert_eq!(docs[0]["c"], Json::String("0x_".to_owned()));
        assert_eq!(docs[0]["e"], Json::String("0b12".to_owned()));
        for bad in ["-00", "+00", "-001"] {
            let error = parse_documents(DataFormat::Yaml, &format!("a: {bad}\n"), Path::new("t"))
                .expect_err(bad);
            assert!(error.to_string().contains("illegal"), "{bad}: {error}");
        }
        for good in ["-0", "00"] {
            let docs = parse(DataFormat::Yaml, &format!("a: {good}\n"));
            assert_eq!(docs[0]["a"].as_i64(), Some(0), "{good}");
        }
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
    fn yaml_numeric_underscores_match_cue() {
        for text in ["_", "__", "+_", "_1", "-_"] {
            let docs = parse(DataFormat::Yaml, &format!("v: {text}\n"));
            assert_eq!(docs[0]["v"], Json::String(text.to_owned()), "{text}");
        }
        let cases = [
            ("1_000.5", "1000.5"),
            ("1_0e5", "1000000"),
            ("1e1_0", "1e10"),
            ("1._5", "1.5"),
            ("1._50", "1.50"),
            ("01", "1"),
            ("1_000", "1000"),
            ("0x1", "1"),
            ("0o17", "15"),
            ("0b101", "5"),
        ];
        for (text, expected) in cases {
            let docs = parse(DataFormat::Yaml, &format!("v: {text}\n"));
            let actual = docs[0]["v"].to_string();
            assert!(
                actual == expected || actual.parse::<f64>().ok() == expected.parse::<f64>().ok(),
                "{text}: {actual} != {expected}"
            );
        }
        for text in [
            "1_", "1__0", "0_1", "1_.5", "1.5_", "1_e5", "1e5_", "01.5", "00.5",
        ] {
            let error = parse_documents(DataFormat::Yaml, &format!("v: {text}\n"), Path::new("t"))
                .expect_err(text);
            assert!(
                error.to_string().contains("illegal '_'")
                    || error.to_string().contains("illegal number"),
                "{text}: {error}"
            );
        }
        for text in ["1..5", "1.5.6", "._5", "1e400"] {
            let docs = parse(DataFormat::Yaml, &format!("v: {text}\n"));
            assert_eq!(docs[0]["v"], Json::String(text.to_owned()), "{text}");
        }
        for text in ["0x_1", "0b_1", "0o_1"] {
            let docs = parse(DataFormat::Yaml, &format!("v: {text}\n"));
            assert_eq!(docs[0]["v"], Json::Number(Number::from(1)), "{text}");
        }
    }

    #[test]
    fn yaml_legacy_octal_and_float_edges_match_cue() {
        for (text, expected) in [
            ("010", "8"),
            ("0o10", "8"),
            ("00", "0"),
            ("000", "0"),
            ("-0", "0"),
        ] {
            let docs = parse(DataFormat::Yaml, &format!("v: {text}\n"));
            assert_eq!(docs[0]["v"].to_string(), expected, "{text}");
        }
        for text in [
            "08", "09", "019", "0009", "-08", "+08", "-09", "+09", "-019", "+019", "-0009", "+0009",
        ] {
            let docs = parse(DataFormat::Yaml, &format!("v: {text}\n"));
            assert_eq!(docs[0]["v"], Json::String(text.to_owned()), "{text}");
        }
        for text in ["-010", "+010", "-00", "+00"] {
            parse_documents(DataFormat::Yaml, &format!("v: {text}\n"), Path::new("t"))
                .expect_err(text);
        }
        let docs = parse(DataFormat::Yaml, "v: !!int 010\n");
        assert_eq!(docs[0]["v"].to_string(), "8");
        for text in ["!!int 08", "!!int 09", "!!int 019", "!!int 0009"] {
            parse_documents(DataFormat::Yaml, &format!("v: {text}\n"), Path::new("t"))
                .expect_err(text);
        }

        for text in [".", "-.", "+."] {
            let docs = parse(DataFormat::Yaml, &format!("v: {text}\n"));
            assert_eq!(docs[0]["v"], Json::String(text.to_owned()), "{text}");
        }
        for (text, expected) in [
            ("-01.5", "-1.5"),
            ("+01.5", "1.5"),
            ("0_0.1", "0.1"),
            ("-._5", "-0.5"),
            ("+._5", "0.5"),
            ("!!float -._5", "-0.5"),
        ] {
            let docs = parse(DataFormat::Yaml, &format!("v: {text}\n"));
            let actual = docs[0]["v"].to_string();
            assert!(
                actual == expected || actual.parse::<f64>().ok() == expected.parse::<f64>().ok(),
                "{text}: {actual} != {expected}"
            );
        }
        for text in ["1e_", "0x1.0", "0b1e2", "0o1e2", "0x1e2.0"] {
            let docs = parse(DataFormat::Yaml, &format!("v: {text}\n"));
            assert_eq!(docs[0]["v"], Json::String(text.to_owned()), "{text}");
        }
        for text in ["1_", "1_.5", "1.5_", "1_e5", "1e5_"] {
            parse_documents(DataFormat::Yaml, &format!("v: {text}\n"), Path::new("t"))
                .expect_err(text);
        }
        for text in ["!!float 0x1.0", "!!float 0b1e2", "!!float 1e_"] {
            parse_documents(DataFormat::Yaml, &format!("v: {text}\n"), Path::new("t"))
                .expect_err(text);
        }

        let docs = parse(DataFormat::Yaml, "010: a\n08: b\n019: c\n-08: d\n");
        assert_eq!(docs[0]["0o10"], Json::String("a".to_owned()));
        assert_eq!(docs[0]["08"], Json::String("b".to_owned()));
        assert_eq!(docs[0]["019"], Json::String("c".to_owned()));
        assert_eq!(docs[0]["-08"], Json::String("d".to_owned()));
        let docs = parse(DataFormat::Yaml, "!!int 010: d\n");
        assert_eq!(docs[0]["0o10"], Json::String("d".to_owned()));
        parse_documents(DataFormat::Yaml, "!!int 09: x\n", Path::new("t")).expect_err("bad key");
    }

    #[test]
    fn tagged_keys_keep_raw_numeric_text_and_resolve_bool_like_cue() {
        let docs = parse(DataFormat::Yaml, "!!int 0x10: a\n!!float 1e2: b\n");
        assert_eq!(docs[0]["0x10"], Json::String("a".to_owned()));
        assert_eq!(docs[0]["1e2"], Json::String("b".to_owned()));
        let docs = parse(DataFormat::Yaml, "!!bool yes: a\n!!bool true: b\n");
        assert_eq!(docs[0]["false"], Json::String("a".to_owned()));
        assert_eq!(docs[0]["true"], Json::String("b".to_owned()));
    }

    #[test]
    fn quoted_merge_key_is_literal() {
        let docs = parse(
            DataFormat::Yaml,
            "\"<<\": {a: 1}\nbase: &b {x: 1}\nchild:\n  <<: *b\n  y: 2\n",
        );
        assert_eq!(docs[0]["<<"]["a"], 1);
        assert_eq!(docs[0]["child"]["x"], 1);
        assert_eq!(docs[0]["child"]["y"], 2);
        assert!(docs[0]["child"].get("<<").is_none());
    }

    #[test]
    fn yaml_keys_normalize_and_collide_like_cue() {
        let error = parse_documents(DataFormat::Yaml, "true: a\nTrue: b\n", Path::new("t"))
            .expect_err("bool keys");
        assert!(matches!(error, DataError::DuplicateKey { .. }), "{error}");
        let error = parse_documents(DataFormat::Yaml, "~: a\nnull: b\n", Path::new("t"))
            .expect_err("null keys");
        assert!(matches!(error, DataError::DuplicateKey { .. }), "{error}");
        let docs = parse(DataFormat::Yaml, "0x1: a\n1: b\n");
        assert_eq!(docs[0]["0x1"], Json::String("a".to_owned()));
        assert_eq!(docs[0]["1"], Json::String("b".to_owned()));
        let error = parse_documents(
            DataFormat::Yaml,
            "!!str 5: a\n!!int \"5\": b\n",
            Path::new("t"),
        )
        .expect_err("tagged keys");
        assert!(matches!(error, DataError::DuplicateKey { .. }), "{error}");
    }

    #[test]
    fn yaml_emitter_escapes_controls_and_quotes_keys() {
        let value = serde_json::json!({
            "_id": "a\u{85}b\u{2028}c\u{2029}d\u{feff}e\u{fffe}f\u{ffff}g",
            "0B101": 1,
            "0O17": 2,
            "<<": 3,
            "plain": "ok",
        });
        let yaml = to_yaml(&value).expect("yaml");
        assert!(yaml.contains("\\u0085"), "{yaml}");
        assert!(yaml.contains("\\u2028"), "{yaml}");
        assert!(yaml.contains("\\u2029"), "{yaml}");
        assert!(yaml.contains("\\uFEFF"), "{yaml}");
        assert!(yaml.contains("\\uFFFE"), "{yaml}");
        assert!(yaml.contains("\\uFFFF"), "{yaml}");
        assert!(yaml.contains("\"_id\""), "{yaml}");
        assert!(yaml.contains("\"0B101\""), "{yaml}");
        assert!(yaml.contains("\"0O17\""), "{yaml}");
        assert!(yaml.contains("\"<<\""), "{yaml}");
        assert_eq!(parse(DataFormat::Yaml, &yaml).remove(0), value);
    }

    #[test]
    fn reserved_serde_json_number_key_is_rejected() {
        let error = parse_documents(
            DataFormat::Json,
            r#"{"$serde_json::private::Number":"1"}"#,
            Path::new("t"),
        )
        .expect_err("reserved");
        assert!(error.to_string().contains("reserved key"), "{error}");
    }

    #[test]
    fn utf8_bom_is_stripped() {
        assert_eq!(
            parse(DataFormat::Json, "\u{feff}{\"a\":1}"),
            vec![serde_json::json!({"a": 1})]
        );
        assert_eq!(
            parse(DataFormat::Yaml, "\u{feff}a: 1\n"),
            vec![serde_json::json!({"a": 1})]
        );
    }

    #[test]
    fn json_node_limit_is_enforced() {
        let mut text = String::from("[");
        for _ in 0..=MAX_JSON_NODES {
            text.push_str("0,");
        }
        text.push('0');
        text.push(']');
        let error = parse_documents(DataFormat::Json, &text, Path::new("t")).expect_err("nodes");
        assert!(error.to_string().contains("node count"), "{error}");
    }

    #[test]
    fn nested_alias_byte_budget_is_enforced() {
        let scalar = "x".repeat(50 * 1024);
        let mut text = format!("a0: &a0 \"{scalar}\"\n");
        for level in 1..=4 {
            text.push_str(&format!("a{level}: &a{level} ["));
            for index in 0..10 {
                if index > 0 {
                    text.push_str(", ");
                }
                text.push_str(&format!("*a{}", level - 1));
            }
            text.push_str("]\n");
        }
        let error = parse_documents(DataFormat::Yaml, &text, Path::new("t")).expect_err("bytes");
        assert!(
            matches!(error, DataError::ExpandedTooLarge { .. }),
            "{error}"
        );
    }

    #[test]
    fn alias_byte_budget_is_enforced() {
        let scalar = "x".repeat(1024 * 1024);
        let mut text = format!("big: &b \"{scalar}\"\n");
        for index in 0..64 {
            text.push_str(&format!("a{index}: *b\n"));
        }
        let error = parse_documents(DataFormat::Yaml, &text, Path::new("t")).expect_err("bytes");
        let message = error.to_string();
        assert!(
            message.contains("expanded") || message.contains("budget"),
            "{message}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn devices_and_fifos_are_rejected_without_reading() {
        let directory = tempfile::tempdir().expect("tempdir");
        let link = directory.path().join("zero.json");
        std::os::unix::fs::symlink("/dev/zero", &link).expect("symlink");
        let error = read_documents(&link).expect_err("device");
        assert!(
            matches!(
                error,
                DataError::NotRegularFile { .. }
                    | DataError::UnsupportedFormat { .. }
                    | DataError::Io { .. }
            ),
            "{error}"
        );

        let fifo = directory.path().join("pipe.yaml");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo");
        if status.success() {
            let error = read_documents(&fifo).expect_err("fifo");
            assert!(matches!(error, DataError::NotRegularFile { .. }), "{error}");
        }
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
        assert!(yaml.find("\"z\": 1").expect("z") < yaml.find("\"a\": 2").expect("a"));
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
