//! Port of `PhotoReview.Shell.Win32/Dialogs/Win32ZoomPromptService.cs` (31 lines, baseline `d92d6aae`). Unit W8-18b. Contract: `contracts/W8-18b.md`.
//! The WPF `WpfZoomPromptService` (16 lines) is retired.
//!
//! `pr_app::services::contracts::ZoomPromptService` (C-13) for the Win32 shell: the custom click-zoom prompt is run by the lazily created `SecondaryWindowPort`
//! (W8-18o; the dialog itself is `zoom_prompt`, W8-18p). A missing port is `None`, like the user cancelling (no zoom change).
#![allow(unused_variables, dead_code)] // stub: removed by the porter with the last `todo!()`

use std::rc::Rc;

use pr_app::services::contracts::ZoomPromptService;

use super::dialog_service::LazySecondaryPort;
use crate::win32::structs::Hwnd;

/// C# `Win32ZoomPromptService`.
pub struct Win32ZoomPromptService {
    owner: Box<dyn Fn() -> Hwnd>,
    secondary: Rc<LazySecondaryPort>,
}

impl Win32ZoomPromptService {
    /// C# ctor `(Func<nint> ownerProvider, Lazy<ISecondaryWindowPort?> secondary)`; the C# `ArgumentNullException` checks cannot be written in Rust.
    pub fn new(owner: Box<dyn Fn() -> Hwnd>, secondary: Rc<LazySecondaryPort>) -> Self {
        Self { owner, secondary }
    }
}

impl ZoomPromptService for Win32ZoomPromptService {
    fn prompt_custom_zoom(&self, current_percent: i32) -> Option<i32> {
        todo!("W8-18b: secondary.get()?.prompt_custom_zoom(owner(), current_percent)")
    }
}

impl std::fmt::Debug for Win32ZoomPromptService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Win32ZoomPromptService").finish_non_exhaustive()
    }
}
