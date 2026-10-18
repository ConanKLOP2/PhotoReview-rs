//! Port of `PhotoReview.App.Shared/TaskLogging.cs` (C# baseline d92d6aae): observing deliberately fire-and-forget work.
//!
//! C# `FireAndLog` attaches a continuation (`OnlyOnFaulted`) that writes the context and the base exception to the app
//! log, so a failure is not lost to `TaskScheduler.UnobservedTaskException` at GC time. Rust has no `Task` (R0003): a
//! job runs on a worker and reports one [`TaskEnd`] through a channel. This module logs that end the way the
//! continuation did. The `AsyncVoidJustification` constant is WPF-only (scoped VSTHRD100 suppressions) and is NOT
//! ported (see the contract).

// D11: temporary allow, the stub bodies are todo!(); removed when the porter fills them.
#![allow(unused_variables, dead_code)]

use std::error::Error;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use pr_core::abstractions::Log;

/// How a fire-and-forget job ended, reported exactly once through its channel (C# terminal state of a `Task`).
pub enum TaskEnd {
    /// C# `RanToCompletion`.
    Completed,
    /// C# `Canceled` (`OperationCanceledException`): never a fault, never logged.
    Canceled,
    /// C# `Faulted`: the job's error. Logged by [`observe_end`] as its innermost error.
    Faulted(Box<dyn Error + Send + Sync>),
}

/// A synchronous failure of the call that starts a job (C# exception thrown by the start delegate before a task exists).
pub enum StartFault {
    /// C# `OperationCanceledException` from the start delegate: silent.
    Canceled,
    /// Any other error from the start delegate: logged with the context.
    Failed(Box<dyn Error + Send + Sync>),
}

/// C# `Exception.GetBaseException()`: the innermost error of the `source()` chain (the error itself when it has no
/// source). `FireAndLog` logs this one, not the wrapper.
pub fn base_error<'a>(err: &'a (dyn Error + 'static)) -> &'a (dyn Error + 'static) {
    todo!()
}

/// C# `FireAndLog(this Task task, string context)` continuation, synchronous core: `Faulted` logs
/// `log.error(context, Some(base_error(e)))`; `Completed` and `Canceled` write nothing. Takes the end by reference, so
/// the caller keeps the outcome (C# `FireAndLog_FaultedTask_StillReportsTheFaultToAwaiters`).
pub fn observe_end(end: &TaskEnd, context: &str, log: &dyn Log) {
    todo!()
}

/// C# `FireAndLog(Func<Task?> start, string context)`: calls `start` inside `catch_unwind` (ADR R0003: a panic is a fault).
///
/// * `Ok(None)`: the absent collaborator (C# `null` task), no-op.
/// * `Ok(Some(handle))`: handed to [`observe_receiver`].
/// * `Err(StartFault::Canceled)`: silent.
/// * `Err(StartFault::Failed(e))` or a panic (payload text via `pr_core::abstractions::panic_message`): logged once with
///   `context` and the error, as the C# `catch (Exception ex) when (ex is not OutOfMemoryException)` arm does.
pub fn start_and_observe(
    start: impl FnOnce() -> Result<Option<Receiver<TaskEnd>>, StartFault> + std::panic::UnwindSafe,
    context: &str,
    log: Arc<dyn Log>,
) {
    todo!()
}

/// C# `FireAndLog(this Task task, string context)` continuation, asynchronous form: a background thread (named
/// `PhotoReview.TaskObserver`, background) waits for the handle's single [`TaskEnd`] and calls [`observe_end`].
/// A sender dropped without sending is silent (the job vanished; C# has no fault to report). Returns the spawn result;
/// the caller logs a spawn failure once with `context`.
pub fn observe_receiver(handle: Receiver<TaskEnd>, context: String, log: Arc<dyn Log>) -> std::io::Result<()> {
    todo!()
}

impl std::fmt::Debug for TaskEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Completed => f.write_str("Completed"),
            Self::Canceled => f.write_str("Canceled"),
            Self::Faulted(e) => f.debug_tuple("Faulted").field(&e.to_string()).finish(),
        }
    }
}

impl std::fmt::Debug for StartFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Canceled => f.write_str("Canceled"),
            Self::Failed(e) => f.debug_tuple("Failed").field(&e.to_string()).finish(),
        }
    }
}
