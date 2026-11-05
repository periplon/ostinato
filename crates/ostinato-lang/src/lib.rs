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

/// Machine-readable description of the float64 model used by `math` builtins.
///
/// Ostinato deliberately pins its Go-port arithmetic to the Go/arm64 FMA
/// behaviour: every multiply-add in the ported Cephes code uses Rust's
/// `f64::mul_add`, which is fused on all targets (falling back to a correctly
/// rounded software `fma` where the hardware lacks FMA). This keeps the
/// evaluator's own results platform-independent. It does **not** make the CUE
/// oracle identical on every platform: Go only fuses on arm64, so a Go/amd64
/// oracle can differ in the last bit. Rare 1-ulp `Asinh`/`Acosh`/`Atanh`
/// residuals remain documented in `docs/SCOPE.md`.
pub const GO_F64_MODEL: &str = "arm64-go-fma";

pub mod ast;
pub mod attributes;
pub mod compile;
pub mod condition;
pub mod cue_data;
pub mod error;
pub mod eval;
mod gomath;
mod gotime;
pub mod lexer;
pub mod numeric;
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
    Builtin, JsonExportError, TypeKind, Value, builtin_module, eval_expr_with_json, eval_file,
    eval_file_with_imports, eval_file_with_imports_mode,
    eval_file_with_imports_mode_allow_incomplete, eval_file_with_imports_mode_allow_outputs,
    eval_file_with_overrides, select_path, strip_hidden, unify, value_from_json,
};
pub use parser::{parse, parse_expr_fragment};
pub use scope::rename_file_locals;
pub use template::{lookup_path, render};
