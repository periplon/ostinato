//! Import policy for JSON/YAML → CUE conversion.
//!
//! The pure value rendering lives in [`ostinato_lang::render_cue_data`]. This
//! module adds multi-document, multi-input and package-validation policy.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value as Json;
use thiserror::Error;

use crate::data;

/// Errors raised while planning an import.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ImportError {
    #[error("multiple documents require --list or --files when writing one CUE file")]
    MultiDocument,
    #[error(
        "cannot merge multiple inputs at `{at}`: top-level value is {found}, expected an object \
         (pass --list or --files instead)"
    )]
    NotObject { at: String, found: &'static str },
    #[error("conflicting values while merging inputs at `{at}`")]
    Conflict { at: String },
    #[error("invalid CUE package name `{name}`")]
    InvalidPackage { name: String },
    #[error("two inputs map to the same output `{output}`")]
    OutputCollision { output: String },
    #[error("{0}")]
    Data(String),
}

impl From<data::DataError> for ImportError {
    fn from(error: data::DataError) -> Self {
        Self::Data(error.to_string())
    }
}

/// One planned output file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    pub path: PathBuf,
    pub content: String,
}

/// Result of planning an import, before any destination is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPlan {
    pub files: Vec<PlannedFile>,
    /// Inputs that could not be written because a single `-o` was reused.
    pub skipped_inputs: Vec<PathBuf>,
}

/// Render the documents of one input file.
///
/// # Errors
/// Rejects multiple documents without `--list` and invalid package names.
pub fn render_documents(
    documents: &[Json],
    package: Option<&str>,
    list: bool,
) -> Result<String, ImportError> {
    validate_package(package)?;
    let value = if list {
        Json::Array(documents.to_vec())
    } else {
        match documents {
            [] => Json::Null,
            [document] => document.clone(),
            _ => return Err(ImportError::MultiDocument),
        }
    };
    Ok(ostinato_lang::render_cue_data(&value, package))
}

/// Render several input files into a single CUE file by unifying objects.
///
/// # Errors
/// Rejects non-object roots, concrete conflicts and invalid package names.
pub fn render_combined(
    documents: &[Json],
    package: Option<&str>,
    list: bool,
) -> Result<String, ImportError> {
    validate_package(package)?;
    let value = if list {
        Json::Array(documents.to_vec())
    } else {
        merge_documents(documents)?
    };
    Ok(ostinato_lang::render_cue_data(&value, package))
}

/// Plan a default one-output-per-input import.
///
/// # Errors
/// Rejects multiple documents without `--list`, duplicate output basenames and
/// invalid package names.
pub fn plan_per_input(
    inputs: &[data::DataFile],
    package: Option<&str>,
    list: bool,
    files: bool,
) -> Result<ImportPlan, ImportError> {
    validate_package(package)?;
    let mut planned = Vec::new();
    for input in inputs {
        if files {
            for (index, document) in input.documents.iter().enumerate() {
                let path = split_output(&input.path, index);
                let content = render_documents(std::slice::from_ref(document), package, list)?;
                planned.push(PlannedFile { path, content });
            }
        } else {
            let content = render_documents(&input.documents, package, list)?;
            planned.push(PlannedFile {
                path: input.path.with_extension("cue"),
                content,
            });
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for file in &planned {
        if !seen.insert(normalized_path(&file.path)) {
            return Err(ImportError::OutputCollision {
                output: file.path.display().to_string(),
            });
        }
    }
    Ok(ImportPlan {
        files: planned,
        skipped_inputs: Vec::new(),
    })
}

/// Plan a single-output import.
///
/// With `merge`, all documents are unified. Without it, the first input is
/// written to `output` and the remaining inputs are reported as skipped, which
/// mirrors `cue import a.json b.yaml -o out.cue`.
///
/// # Errors
/// Rejects multiple documents without `--list`/`--files`, merge conflicts,
/// invalid package names and duplicate split outputs.
pub fn plan_single(
    inputs: &[data::DataFile],
    output: &Path,
    package: Option<&str>,
    list: bool,
    files: bool,
    merge: bool,
    force: bool,
) -> Result<ImportPlan, ImportError> {
    validate_package(package)?;
    if merge && files {
        return Err(ImportError::MultiDocument);
    }
    if files {
        if output == Path::new("-") {
            let mut content = String::new();
            for input in inputs {
                for document in &input.documents {
                    content.push_str(&render_documents(
                        std::slice::from_ref(document),
                        package,
                        list,
                    )?);
                }
            }
            return Ok(ImportPlan {
                files: vec![PlannedFile {
                    path: output.to_path_buf(),
                    content,
                }],
                skipped_inputs: Vec::new(),
            });
        }
        let first = inputs.first().ok_or(ImportError::MultiDocument)?;
        let content = match first.documents.first() {
            Some(document) => render_documents(std::slice::from_ref(document), package, list)?,
            None => render_documents(&[], package, list)?,
        };
        let mut skipped_inputs = Vec::new();
        if first.documents.len() > 1 {
            skipped_inputs.push(first.path.clone());
        }
        skipped_inputs.extend(inputs.iter().skip(1).map(|input| input.path.clone()));
        return Ok(ImportPlan {
            files: vec![PlannedFile {
                path: output.to_path_buf(),
                content,
            }],
            skipped_inputs,
        });
    }
    if merge {
        let documents: Vec<Json> = inputs
            .iter()
            .flat_map(|input| input.documents.iter().cloned())
            .collect();
        let content = render_combined(&documents, package, list)?;
        return Ok(ImportPlan {
            files: vec![PlannedFile {
                path: output.to_path_buf(),
                content,
            }],
            skipped_inputs: Vec::new(),
        });
    }
    let selected =
        if force { inputs.last() } else { inputs.first() }.ok_or(ImportError::MultiDocument)?;
    let content = render_documents(&selected.documents, package, list)?;
    let skipped_inputs = if force {
        inputs
            .iter()
            .take(inputs.len().saturating_sub(1))
            .map(|input| input.path.clone())
            .collect()
    } else {
        inputs
            .iter()
            .skip(1)
            .map(|input| input.path.clone())
            .collect()
    };
    Ok(ImportPlan {
        files: vec![PlannedFile {
            path: output.to_path_buf(),
            content,
        }],
        skipped_inputs,
    })
}

/// Read and plan one import operation.
///
/// # Errors
/// Propagates decoding and planning errors. All inputs are decoded before any
/// output is planned, so parse failures never leave partial files.
pub fn import_files(
    inputs: &[PathBuf],
    output: Option<&Path>,
    package: Option<&str>,
    list: bool,
    files: bool,
    merge: bool,
    force: bool,
) -> Result<ImportPlan, ImportError> {
    let mut decoded = Vec::with_capacity(inputs.len());
    for path in inputs {
        decoded.push(data::DataFile::read(path)?);
    }
    match output {
        Some(output) => plan_single(&decoded, output, package, list, files, merge, force),
        None => plan_per_input(&decoded, package, list, files),
    }
}

fn normalized_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(path),
            Err(_) => path.to_path_buf(),
        }
    };
    let Some(parent) = absolute.parent() else {
        return absolute;
    };
    let parent = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    match absolute.file_name() {
        Some(name) => {
            let mut name = name.to_os_string();
            if is_case_insensitive(&parent)
                && let Some(text) = name.to_str()
            {
                name = text.to_ascii_lowercase().into();
            }
            parent.join(name)
        }
        None => parent,
    }
}

fn is_case_insensitive(directory: &Path) -> bool {
    for attempt in 0..4 {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let name = format!(".ostinato-case-{}-{nanos}-{attempt}", std::process::id());
        let path = directory.join(&name);
        let Ok(file) = OpenOptions::new().write(true).create_new(true).open(&path) else {
            continue;
        };
        let alternate = directory.join(name.to_ascii_uppercase());
        let insensitive = std::fs::metadata(&alternate).is_ok();
        drop(file);
        let _ = std::fs::remove_file(&path);
        return insensitive;
    }
    false
}

fn split_output(input: &Path, index: usize) -> PathBuf {
    if index == 0 {
        return input.with_extension("cue");
    }
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("import");
    let name = format!("{stem}-{index}.cue");
    input.with_file_name(name)
}

fn merge_documents(documents: &[Json]) -> Result<Json, ImportError> {
    let mut iter = documents.iter();
    let Some(first) = iter.next() else {
        return Ok(Json::Null);
    };
    if !first.is_object() {
        return Err(ImportError::NotObject {
            at: "$".to_owned(),
            found: json_type_name(first),
        });
    }
    let mut merged = first.clone();
    for document in iter {
        if !document.is_object() {
            return Err(ImportError::NotObject {
                at: "$".to_owned(),
                found: json_type_name(document),
            });
        }
        merged = data::unify_values(&merged, document, Path::new("$")).map_err(map_unify)?;
    }
    Ok(merged)
}

fn map_unify(error: data::DataError) -> ImportError {
    match error {
        data::DataError::Conflict { at, .. } => ImportError::Conflict { at },
        other => ImportError::Data(other.to_string()),
    }
}

fn validate_package(package: Option<&str>) -> Result<(), ImportError> {
    match package {
        Some(package) if !ostinato_lang::is_valid_package(package) => {
            Err(ImportError::InvalidPackage {
                name: package.to_owned(),
            })
        }
        _ => Ok(()),
    }
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn data_file(path: &str, documents: Vec<Json>) -> data::DataFile {
        data::DataFile {
            path: PathBuf::from(path),
            documents,
        }
    }

    #[test]
    fn multiple_documents_without_list_or_files_fail() {
        let error = render_documents(
            &[serde_json::json!({"a": 1}), serde_json::json!({"b": 2})],
            None,
            false,
        )
        .expect_err("multi");
        assert!(matches!(error, ImportError::MultiDocument));
    }

    #[test]
    fn per_input_plan_reports_collisions() {
        let inputs = vec![
            data_file("x.json", vec![serde_json::json!({"a": 1})]),
            data_file("x.yaml", vec![serde_json::json!({"b": 2})]),
        ];
        let error = plan_per_input(&inputs, None, false, false).expect_err("collision");
        assert!(matches!(error, ImportError::OutputCollision { .. }));
    }

    #[test]
    fn split_outputs_match_cue_names() {
        assert_eq!(
            split_output(Path::new("multi.yaml"), 0),
            PathBuf::from("multi.cue")
        );
        assert_eq!(
            split_output(Path::new("multi.yaml"), 1),
            PathBuf::from("multi-1.cue")
        );
    }

    #[test]
    fn single_output_without_merge_skips_extra_inputs() {
        let inputs = vec![
            data_file("a.json", vec![serde_json::json!({"a": 1})]),
            data_file("b.yaml", vec![serde_json::json!({"b": 2})]),
        ];
        let plan = plan_single(
            &inputs,
            Path::new("out.cue"),
            None,
            false,
            false,
            false,
            false,
        )
        .expect("plan");
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.skipped_inputs, vec![PathBuf::from("b.yaml")]);
    }

    #[test]
    fn single_output_with_force_uses_last_input() {
        let inputs = vec![
            data_file("a.json", vec![serde_json::json!({"a": 1})]),
            data_file("b.yaml", vec![serde_json::json!({"b": 2})]),
        ];
        let plan = plan_single(
            &inputs,
            Path::new("out.cue"),
            None,
            false,
            false,
            false,
            true,
        )
        .expect("plan");
        assert_eq!(plan.files[0].content, "b: 2\n");
        assert_eq!(plan.skipped_inputs, vec![PathBuf::from("a.json")]);
    }

    #[test]
    fn files_stdout_concatenates_all_documents() {
        let inputs = vec![
            data_file(
                "a.yaml",
                vec![serde_json::json!({"a": 1}), serde_json::json!({"a": 2})],
            ),
            data_file("b.yaml", vec![serde_json::json!({"b": 3})]),
        ];
        let plan =
            plan_single(&inputs, Path::new("-"), None, false, true, false, false).expect("plan");
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].content, "a: 1\na: 2\nb: 3\n");
        assert!(plan.skipped_inputs.is_empty());
    }

    #[test]
    fn output_collisions_are_detected_after_path_normalization() {
        let cwd = std::env::current_dir().expect("cwd");
        let inputs = vec![
            data_file(
                cwd.join("x.json").to_str().expect("absolute"),
                vec![serde_json::json!({"a": 1})],
            ),
            data_file("x.yaml", vec![serde_json::json!({"b": 2})]),
        ];
        let error = plan_per_input(&inputs, None, false, false).expect_err("collision");
        assert!(
            matches!(error, ImportError::OutputCollision { .. }),
            "{error}"
        );

        let inputs = vec![
            data_file("x.json", vec![serde_json::json!({"a": 1})]),
            data_file("./x.yaml", vec![serde_json::json!({"b": 2})]),
        ];
        let error = plan_per_input(&inputs, None, false, false).expect_err("collision");
        assert!(
            matches!(error, ImportError::OutputCollision { .. }),
            "{error}"
        );
    }

    #[test]
    fn merge_unifies_inputs() {
        let inputs = vec![
            data_file("a.json", vec![serde_json::json!({"a": 1})]),
            data_file("b.yaml", vec![serde_json::json!({"b": 2})]),
        ];
        let plan = plan_single(
            &inputs,
            Path::new("out.cue"),
            None,
            false,
            false,
            true,
            false,
        )
        .expect("plan");
        assert_eq!(plan.files[0].content, "a: 1\nb: 2\n");
        assert!(plan.skipped_inputs.is_empty());
    }
}
