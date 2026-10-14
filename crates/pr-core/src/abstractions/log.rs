//! Port of `ILog.cs` + `Diagnostics/NullLog.cs`. INV-10: logging is off by default and creates nothing when off.

use std::error::Error;

pub trait Log: Send + Sync {
    fn enabled(&self) -> bool;
    fn info(&self, message: &str);
    fn warn(&self, message: &str);
    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>);
}

/// Discards everything.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullLog;

impl Log for NullLog {
    fn enabled(&self) -> bool {
        false
    }
    fn info(&self, _message: &str) {}
    fn warn(&self, _message: &str) {}
    fn error(&self, _message: &str, _error: Option<&(dyn Error + 'static)>) {}
}
