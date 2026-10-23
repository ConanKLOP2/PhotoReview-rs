//! Port of the `InstanceScope` creation of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 329-335 and 339-340 (baseline `7eaee2a2`): the real [`InstanceGate`]. Unit W8-17d.
//! Contract: `contracts/W8-17d.md`.
//!
//! `pr_platform_win::instance_scope::InstanceScope` (W2-19, merged) owns the mutex and the forward pipe; `pr_core::instance::handoff::try_forward` (W2-19) does the hand-off. This file only
//! creates the scope with the start-up's options and adapts both to the pure [`InstanceGate`] of `pr_app::startup::instance`.

use std::sync::Arc;

use pr_app::startup::instance::{InstanceGate, InstanceStartError};
use pr_core::abstractions::Log;
use pr_core::instance::coalescer::ForwardedOpenCoalescer;
use pr_core::model::InstanceMode;
use pr_platform_win::instance_scope::InstanceScope;

/// C# `new InstanceScope(mode, _coalescer.Submit, log, prefix, allowServerForeground: true)`: `InstanceScope::with_options(mode, move |paths| coalescer.submit(paths), Some(log), prefix, None,
/// true)`. `Err(InstanceScopeError)` (an empty or whitespace prefix, a native failure) becomes `InstanceStartError { message }`. The callback runs on the pipe server's thread: `submit` only
/// queues, the coalescer's worker posts to the UI thread (`forwarded_open::ui_open_callback`).
pub fn create_scope(
    mode: InstanceMode,
    coalescer: Arc<ForwardedOpenCoalescer>,
    log: Arc<dyn Log>,
    prefix: &str,
) -> Result<Arc<InstanceScope>, InstanceStartError> {
    InstanceScope::with_options(
        mode,
        move |paths| {
            coalescer.submit(paths);
        },
        Some(log),
        prefix,
        None,
        true,
    )
    .map(Arc::new)
    .map_err(|error| InstanceStartError {
        message: error.to_string(),
    })
}

/// [`InstanceGate`] over a scope. `Send + Sync` (the hand-off runs on a worker).
pub struct ScopeGate {
    scope: Arc<InstanceScope>,
    log: Arc<dyn Log>,
}

impl std::fmt::Debug for ScopeGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopeGate").finish_non_exhaustive()
    }
}

impl ScopeGate {
    /// A gate over `scope`; `log` goes to `try_forward`.
    pub fn new(scope: Arc<InstanceScope>, log: Arc<dyn Log>) -> Self {
        Self { scope, log }
    }
}

impl InstanceGate for ScopeGate {
    /// `scope.mode()`.
    fn mode(&self) -> InstanceMode {
        self.scope.mode()
    }

    /// `scope.try_acquire(folder)`; `Err(InstanceScopeError)` -> `InstanceStartError`.
    fn try_acquire(&self, folder: Option<&str>) -> Result<bool, InstanceStartError> {
        self.scope.try_acquire(folder).map_err(|error| InstanceStartError {
            message: error.to_string(),
        })
    }

    /// `client = scope.create_client(folder)?` (`Err` -> `InstanceStartError`); `try_forward(&*client, existing_paths, handoff::DEFAULT_TIMEOUT, Some(&*log), &CancelToken::none())`;
    /// `Ok(delivered)` -> `Ok(delivered)`; `Err(Canceled)` (impossible with a token that never cancels) -> `Ok(false)`.
    fn forward_to_owner(&self, folder: Option<&str>, existing_paths: &[String]) -> Result<bool, InstanceStartError> {
        let client = self.scope.create_client(folder).map_err(|error| InstanceStartError {
            message: error.to_string(),
        })?;
        Ok(pr_core::instance::handoff::try_forward(
            &*client,
            existing_paths,
            pr_core::instance::handoff::DEFAULT_TIMEOUT,
            Some(&*self.log),
            &pr_core::sync::CancelToken::none(),
        )
        .unwrap_or(false))
    }
}
