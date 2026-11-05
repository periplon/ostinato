//! CUE-compatible front-end for Ostinato.
//!
//! The **default** evaluator used by the CLI is the Rust implementation in this
//! crate (see `docs/SCOPE.md`). Official CUE is the conformance oracle used by
//! `--evaluator cue`. This crate provides:
//!
//! * a lexer/parser/evaluator for the workflow-oriented CUE surface;
//! * the typed workflow compiler shared by both evaluators;
//! * runtime condition and template evaluation.
//!
//! Official CUE is the normative implementation. The native evaluator is
//! intentionally not advertised as fully compliant.

pub mod ast;
pub mod attributes;
pub mod compile;
pub mod condition;
pub mod cue_data;
pub mod error;
pub mod eval;
mod gotime;
pub mod lexer;
pub mod parser;
pub mod scope;
pub mod template;

pub use attributes::{TagInfo, TagSegment, TagType, assign_field_ids, parameter_tags};
pub use compile::{
    CompiledWorkflow, ParamSpec, compile, compile_json, compile_value, compile_with_params,
};
pub use condition::{eval_condition, validate_condition};
pub use cue_data::{is_valid_package, render_cue_data};
pub use error::LangError;
pub use eval::{
    Builtin, TypeKind, Value, builtin_module, eval_expr_with_json, eval_file,
    eval_file_with_imports, eval_file_with_imports_mode,
    eval_file_with_imports_mode_allow_incomplete, eval_file_with_imports_mode_allow_outputs,
    eval_file_with_overrides, select_path, strip_hidden, unify, value_from_json,
};
pub use parser::{parse, parse_expr_fragment};
pub use scope::rename_file_locals;
pub use template::{lookup_path, render};
