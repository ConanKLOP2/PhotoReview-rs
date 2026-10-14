//! Port of `IClock.cs` + `SystemClock.cs`.

use crate::time::DateTimeUtc;

/// Wall-clock time, injectable for tests.
pub trait Clock: Send + Sync {
    /// Current time in UTC (C#: `UtcNow`).
    fn utc_now(&self) -> DateTimeUtc;
}

/// The OS clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn utc_now(&self) -> DateTimeUtc {
        DateTimeUtc::now()
    }
}
