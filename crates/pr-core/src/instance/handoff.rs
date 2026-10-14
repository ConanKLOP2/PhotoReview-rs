//! Port of `PhotoReview.Core/Instance/SecondInstanceHandoff.cs` (C# baseline d92d6aae, 31 lines): the second launch
//! asks the running instance to take over its request and, when it did (or very likely did), simply exits.
//!
//! Unit W2-19. Contract: `contracts/W2-19.md`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use crate::abstractions::Log;
use crate::instance::forwarding::{ForwardError, ForwardOutcome, InstanceForwardClient};
use crate::sync::{CancelToken, Canceled};

/// The wait for the owner to answer before a second launch gives up (covers the owner still starting up).
/// C# `DefaultTimeout` = 1500 ms.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1500);

/// `TryForwardAsync`: true when the running instance took over (or, Q-R12, very likely took over) the request and this
/// process can simply exit. `log == None` -> nothing is logged (C# `NullLog`).
///
/// * `Delivered` and `Unknown` -> `Ok(true)`; `Rejected` and `NoInstance` -> `Ok(false)`.
/// * One `Info` line per call that got an outcome: `Forward to running instance: {outcome} ({n} path(s))`.
/// * A client failure (`ForwardError::Failed`, or a panic inside `send`) -> one `Warn` line
///   `Forwarding to the running instance failed: {kind}` and `Ok(false)`.
/// * `ForwardError::Canceled` -> `Err(Canceled)`.
pub fn try_forward(
    client: &dyn InstanceForwardClient,
    existing_paths: &[String],
    timeout: Duration,
    log: Option<&dyn Log>,
    cancel: &CancelToken,
) -> Result<bool, Canceled> {
    // A panic inside `send` is the C# "client throws" case.
    let result = catch_unwind(AssertUnwindSafe(|| client.send(existing_paths, timeout, cancel))).unwrap_or_else(|_| {
        Err(ForwardError::Failed {
            kind: "Panic".to_owned(),
        })
    });
    match result {
        Err(ForwardError::Canceled) => Err(Canceled),
        Err(ForwardError::Failed { kind }) => {
            if let Some(log) = log {
                log.warn(&format!("Forwarding to the running instance failed: {kind}"));
            }
            Ok(false)
        }
        Ok(outcome) => {
            if let Some(log) = log {
                log.info(&format!(
                    "Forward to running instance: {outcome} ({} path(s))",
                    existing_paths.len()
                ));
            }
            Ok(matches!(outcome, ForwardOutcome::Delivered | ForwardOutcome::Unknown))
        }
    }
}
