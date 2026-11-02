//! CUE-compatible front-end for Ostinato.
//!
//! The **default** evaluator used by the CLI is the official CUE
//! implementation (see `docs/COMPLIANCE.md`). This crate provides:
//!
//! * a native lexer/parser/evaluator for a documented CUE subset, used by
//!   `--evaluator native` and by tests that must run without external tools;
//! * the typed workflow compiler shared by both evaluators;
//! * runtime condition and template evaluation.
//!
//! Official CUE is the normative implementation. The native evaluator is
//! intentionally not advertised as fully compliant.

pub mod ast;
pub mod compile;
pub mod condition;
pub mod error;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod template;

pub use compile::{CompiledWorkflow, ParamSpec, compile, compile_json, compile_value};
pub use condition::eval_condition;
pub use error::LangError;
pub use eval::{TypeKind, Value, eval_expr_with_json, eval_file, value_from_json};
pub use parser::{parse, parse_expr_fragment};
pub use template::{lookup_path, render};
