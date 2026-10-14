//! Port of `PhotoReview.Core/Instance/SecondInstanceHandoff.cs` (C# baseline d92d6aae, 31 lines): the second launch
//! asks the running instance to take over its request and, when it did (or very likely did), simply exits.
//!
//! Unit W2-19. Stub: bodies are `todo!()`; data definitions are complete. Contract: `contracts/W2-19.md`.

// Stub-only (docs/MULTI-AGENT-PLAN D11): removed by the porter in the commit that removes the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::time::Duration;

use crate::abstractions::Log;
use crate::instance::forwarding::InstanceForwardClient;
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
    todo!()
}
