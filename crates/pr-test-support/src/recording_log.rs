use std::error::Error;
use std::sync::{Mutex, PoisonError};

use pr_core::abstractions::Log;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// Keeps every message for assertions. `enabled` is configurable (INV-10 tests).
#[derive(Debug)]
pub struct RecordingLog {
    enabled: bool,
    entries: Mutex<Vec<(LogLevel, String)>>,
}

impl RecordingLog {
    pub fn new() -> Self {
        Self::with_enabled(true)
    }

    pub fn with_enabled(enabled: bool) -> Self {
        Self {
            enabled,
            entries: Mutex::new(Vec::new()),
        }
    }

    pub fn entries(&self) -> Vec<(LogLevel, String)> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn contains(&self, level: LogLevel, needle: &str) -> bool {
        self.entries().iter().any(|(l, m)| *l == level && m.contains(needle))
    }

    fn push(&self, level: LogLevel, message: String) {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((level, message));
    }
}

impl Default for RecordingLog {
    fn default() -> Self {
        Self::new()
    }
}

impl Log for RecordingLog {
    fn enabled(&self) -> bool {
        self.enabled
    }

    fn info(&self, message: &str) {
        self.push(LogLevel::Info, message.to_owned());
    }

    fn warn(&self, message: &str) {
        self.push(LogLevel::Warn, message.to_owned());
    }

    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        let text = match error {
            Some(e) => format!("{message}: {e}"),
            None => message.to_owned(),
        };
        self.push(LogLevel::Error, text);
    }
}
