use ostinato_application::{PortError, PortResult, Telemetry};
use ostinato_domain::AuditEvent;

/// Observability adapter: structured tracing events and metrics.
#[derive(Debug, Default, Clone, Copy)]
pub struct TracingTelemetry;

impl Telemetry for TracingTelemetry {
    fn event(&self, event: &AuditEvent) {
        tracing::info!(
            target: "ostinato.audit",
            run_id = %event.run_id,
            seq = event.seq,
            kind = %event.kind,
            step = event.step.as_deref().unwrap_or(""),
            hash = %event.hash,
            data = %event.data,
            "audit event"
        );
    }

    fn metric(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        tracing::debug!(
            target: "ostinato.metrics",
            metric = name,
            value,
            labels = ?labels,
            "metric"
        );
    }
}

/// Initialise JSON or human-readable tracing on stderr.
///
/// # Errors
/// Returns a configuration error if a global subscriber is already set.
pub fn init_tracing(verbose: u8, json: bool) -> PortResult<()> {
    let default_level = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true);
    if json {
        builder
            .json()
            .try_init()
            .map_err(|err| PortError::Config(format!("tracing init failed: {err}")))
    } else {
        builder
            .try_init()
            .map_err(|err| PortError::Config(format!("tracing init failed: {err}")))
    }
}
