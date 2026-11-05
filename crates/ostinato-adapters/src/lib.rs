//! Infrastructure adapters for Ostinato.
//!
//! * [`NativeLoader`] is the default Rust workflow-CUE evaluator.
//! * [`CueCliLoader`] exposes official CUE as the conformance oracle
//!   (`--evaluator cue`).
//! * [`SqliteStore`] is the durable, transactional journal.
//! * [`ProcessExecutor`] and [`CommandAgent`] are the deterministic and
//!   agentic step runners.

pub mod clock;
pub mod codec;
pub mod cue_import;
pub mod data;
pub mod loaders;
pub mod output;
pub mod process;
pub mod store;
pub mod telemetry;
pub mod workspace;

pub use clock::{SystemClock, UuidGen};
pub use codec::JsonYamlCodec;
pub use cue_import::{
    ImportError, ImportPlan, PlannedFile, import_files, plan_per_input, plan_single,
    render_combined, render_documents,
};
pub use data::{
    DataError, DataFile, DataFormat, MAX_ALIAS_EXPANSION_NODES, MAX_DEPTH, MAX_EXPANDED_BYTES,
    MAX_INPUT_BYTES, MAX_JSON_NODES, MAX_YAML_ALIASES, MAX_YAML_DOCUMENTS, MAX_YAML_NODES, encode,
    parse_documents, read_documents, read_object_document, select_json_path, to_json, to_yaml,
    unify_objects, unify_values,
};
pub use loaders::{
    CueCliLoader, NativeLoader, ToolCommandInfo, official_loader, run_tool_command_paths,
    tool_command_metadata,
};
pub use output::{FileOutputSink, write_all_atomic};
pub use process::{CommandAgent, ProcessExecutor};
pub use store::SqliteStore;
pub use telemetry::{TracingTelemetry, init_tracing};
pub use workspace::{LocalWorkspace, is_safe_relative, write_artifact};
