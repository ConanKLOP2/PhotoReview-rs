use std::sync::Mutex;
use std::sync::PoisonError;

use pr_core::abstractions::Clock;
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};

/// A clock that only moves when told to.
#[derive(Debug)]
pub struct FakeClock {
    now: Mutex<DateTimeUtc>,
}

impl FakeClock {
    /// Starts at 2026-01-01T00:00:00Z.
    pub fn new() -> Self {
        Self::at(
            DateTimeUtc::parse_json("2026-01-01T00:00:00Z")
                .expect("valid literal")
                .0,
        )
    }

    pub fn at(start: DateTimeUtc) -> Self {
        Self { now: Mutex::new(start) }
    }

    pub fn set(&self, value: DateTimeUtc) {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner) = value;
    }

    pub fn advance_ticks(&self, ticks: i64) {
        let mut now = self.now.lock().unwrap_or_else(PoisonError::into_inner);
        *now = now
            .checked_add_ticks(ticks)
            .expect("fake clock left the DateTime range");
    }

    pub fn advance_seconds(&self, seconds: i64) {
        self.advance_ticks(seconds * TICKS_PER_SECOND);
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    fn utc_now(&self) -> DateTimeUtc {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
