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

pub use clock::{SystemClock, UuidGen};
pub use loaders::{CueCliLoader, NativeLoader, official_loader, run_tool_command};
pub use process::{CommandAgent, ProcessExecutor};
pub use store::SqliteStore;
pub use telemetry::{TracingTelemetry, init_tracing};
