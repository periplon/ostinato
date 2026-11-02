use std::time::Duration;

use ostinato_application::{Clock, IdGenerator};

/// Wall-clock adapter.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> String {
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// UUID v4 run id adapter.
#[derive(Debug, Default, Clone, Copy)]
pub struct UuidGen;

impl IdGenerator for UuidGen {
    fn new_run_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}
