//! Infrastructure adapters for Ostinato.
//!
//! * [`CueCliLoader`] delegates language semantics to the official CUE CLI,
//!   which makes the default evaluator fully compliant with current CUE.
//! * [`NativeLoader`] provides an offline CUE subset (explicitly not fully
//!   compliant).
//! * [`SqliteStore`] is the durable, transactional journal.
//! * [`ProcessExecutor`] and [`CommandAgent`] are the deterministic and
//!   agentic step runners.

pub mod clock;
pub mod loaders;
pub mod process;
pub mod store;
pub mod telemetry;

pub use clock::{SystemClock, UuidGen};
pub use loaders::{CueCliLoader, NativeLoader, official_loader};
pub use process::{CommandAgent, ProcessExecutor};
pub use store::SqliteStore;
pub use telemetry::{TracingTelemetry, init_tracing};
