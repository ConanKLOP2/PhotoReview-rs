//! Port of `PhotoReview.Core/Instance/InstanceForwarding.cs` (C# baseline d92d6aae, 21 lines): the client half of the
//! Q-R10 forwarding channel. The named-pipe implementation (`PhotoReviewRs` pipe names, R0002) is Wave 6.
//!
//! Unit W2-19. Complete (data and trait only). Contract: `contracts/W2-19.md`.

use std::fmt;
use std::time::Duration;

use crate::sync::CancelToken;

/// Result of handing the command-line paths of a second launch to the instance that already owns the folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ForwardOutcome {
    /// The running instance accepted the request; the caller should exit.
    Delivered,
    /// An instance answered but refused the request (invalid or malformed paths).
    Rejected,
    /// Q-R12: the request was written to the owner but its reply did not arrive in time (owner busy). It most likely
    /// opens the path; the caller exits without an error dialog.
    Unknown,
    /// Nobody answered in time (stale mutex, or the owner is stuck): fall back to the old behaviour.
    NoInstance,
}

impl ForwardOutcome {
    /// The C# enum member name (`outcome.ToString()`), which the log line contains.
    pub fn name(self) -> &'static str {
        match self {
            ForwardOutcome::Delivered => "Delivered",
            ForwardOutcome::Rejected => "Rejected",
            ForwardOutcome::Unknown => "Unknown",
            ForwardOutcome::NoInstance => "NoInstance",
        }
    }
}

impl fmt::Display for ForwardOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why a [`InstanceForwardClient::send`] did not produce an outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardError {
    /// The caller's token was canceled (C# `OperationCanceledException`); propagated by the handoff.
    Canceled,
    /// Any other failure. `kind` is what the log line names (C# `ex.GetType().Name`): a short error-kind word such as
    /// `"BrokenPipe"` or `"InvalidOperation"`.
    Failed { kind: String },
}

/// Client half of the Q-R10 forwarding channel (implemented over a named pipe in `pr-platform-win`).
/// C# `IInstanceForwardClient`.
pub trait InstanceForwardClient: Send + Sync {
    /// Sends `paths` to the instance that owns the folder and waits at most `timeout` for its answer.
    fn send(&self, paths: &[String], timeout: Duration, cancel: &CancelToken) -> Result<ForwardOutcome, ForwardError>;
}
