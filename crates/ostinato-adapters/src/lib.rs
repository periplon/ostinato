//! Infrastructure adapters for Ostinato.
//!
//! * [`NativeLoader`] is the default Rust workflow-CUE evaluator.
//! * [`CueCliLoader`] exposes official CUE as the conformance oracle
//!   (`--evaluator cue`).
//! * [`SqliteStore`] is the durable, transactional journal.
//! * [`ProcessExecutor`] and [`CommandAgent`] are the deterministic and
//!   agentic step runners.

pub mod clock;
pub mod loaders;
pub mod process;
pub mod store;
pub mod telemetry;
pub mod workspace;

pub use clock::{SystemClock, UuidGen};
pub use loaders::{
    CueCliLoader, NativeLoader, ToolCommandInfo, official_loader, run_tool_command_paths,
    tool_command_metadata,
};
pub use process::{CommandAgent, ProcessExecutor};
pub use store::SqliteStore;
pub use telemetry::{TracingTelemetry, init_tracing};
pub use workspace::{LocalWorkspace, is_safe_relative, write_artifact};
