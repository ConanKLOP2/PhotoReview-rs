//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 329-359 (baseline `d92d6aae`): the single-instance negotiation of the start-up (Q-R18, Q-R10).
//! Unit W8-17d. Contract: `contracts/W8-17d.md`.
//!
//! Order (C# comments kept): the instance MODE comes from the settings just read, so BEFORE any lock; `--smoke` takes no lock at all. A refused lock is not an error: the
//! request is HANDED to the running instance (`SecondInstanceHandoff.TryForwardAsync`, `pr_core::instance::handoff::try_forward`, up to 1.5 s); only when nobody answers does the
//! second launch say so and exit 0. The seam [`InstanceGate`] is implemented over `pr_platform_win::instance_scope::InstanceScope` by `pr_shell::startup::instance`; this
//! module is pure.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::fmt;

use pr_core::model::InstanceMode;
use pr_l10n::localizer::Localizer;

/// The lock or the hand-off could not even be attempted (C# `InstanceScope` / `CreateClient` throwing): the start-up fails (box + exit code 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceStartError {
    /// The message of the underlying error.
    pub message: String,
}

impl fmt::Display for InstanceStartError {
    /// The message.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl std::error::Error for InstanceStartError {}

/// What the start-up needs of the instance scope (C# `InstanceScope`).
pub trait InstanceGate: Send + Sync {
    /// C# `Mode` (the mode the scope was created with).
    fn mode(&self) -> InstanceMode;
    /// C# `TryAcquire(folder)`: `Ok(false)` when ANOTHER process holds the lock for this folder (in `SingleWindow` mode: for the whole app).
    fn try_acquire(&self, folder: Option<&str>) -> Result<bool, InstanceStartError>;
    /// C# `SecondInstanceHandoff.TryForwardAsync(CreateClient(folder), existingPaths, DefaultTimeout, log)`: BLOCKS up to the hand-off timeout, so the caller runs it
    /// on a worker thread. `Ok(true)` = the running instance took the request over (or very likely did, Q-R12).
    fn forward_to_owner(&self, folder: Option<&str>, existing_paths: &[String]) -> Result<bool, InstanceStartError>;
}

/// The two boxes of a second launch that nobody answered (W-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceMessageKind {
    /// `PerFolder` mode: `Tr.FolderAlreadyOpenInOtherInstance` (key `folder.alreadyOpenInOtherInstance`).
    FolderAlreadyOpen,
    /// `SingleWindow` mode: `Tr.AppAlreadyRunningNoResponse` (key `app.alreadyRunningNoResponse`).
    AppAlreadyRunningNoResponse,
}

/// What the second launch does after the hand-off (C# lines 339-352).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffDecision {
    /// Delivered: exit 0 without a box.
    ExitSilently,
    /// Not delivered: show this box (information, title `app.title`), then exit 0.
    ExitWithMessage(InstanceMessageKind),
}

/// C# lines 341-348: `forwarded` -> `ExitSilently`; else `PerFolder` -> `FolderAlreadyOpen`, any other mode -> `AppAlreadyRunningNoResponse`.
pub fn handoff_decision(forwarded: bool, mode: InstanceMode) -> HandoffDecision {
    todo!()
}

/// The localized text of the box: `localizer.get("folder.alreadyOpenInOtherInstance")` or `localizer.get("app.alreadyRunningNoResponse")`.
pub fn instance_message_text(kind: InstanceMessageKind, localizer: &Localizer) -> String {
    todo!()
}

/// The title of every start-up box: `localizer.get("app.title")` (C# `Tr.AppTitle`).
pub fn startup_box_title(localizer: &Localizer) -> String {
    todo!()
}

/// C# lines 331-335: `try_acquire(launch_folder)` of a non-smoke start-up. `Ok(true)` = this process holds the lock and goes on; `Ok(false)` = the caller must run the hand-off
/// ([`InstanceGate::forward_to_owner`], on a worker) and then [`handoff_decision`]. `Err` = start-up failure.
pub fn try_take_lock(gate: &dyn InstanceGate, launch_folder: Option<&str>) -> Result<bool, InstanceStartError> {
    todo!()
}
