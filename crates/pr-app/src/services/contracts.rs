//! Port of `Services/IClipboardService.cs` (8 lines), `Services/IPresentationObserver.cs` (19), `Services/IZoomPromptService.cs`
//! (10) and `Services/UnavailableClipboardService.cs` (12). Contract: `contracts/W8-04c.md`, rules R-CT-*.
//!
//! The C# interfaces are seams for view-model tests. The traits have no `Send`/`Sync` bound: every implementation runs on the
//! UI thread (the Win32 clipboard and the zoom prompt belong to the owner window's thread). Only `pr_core::abstractions::
//! KeyNameValidator` and the preview-service seam are shared across threads.

/// C# `IClipboardService` (8 lines): writes text to the system clipboard.
pub trait ClipboardService {
    /// `true` when the text is on the clipboard; `false` when the clipboard stayed unavailable. Never panics for that (R-CT-1).
    fn try_set_text(&self, text: &str) -> bool;
}

/// C# `UnavailableClipboardService` (12 lines): the clipboard of a view model built without an injected service (tests, tools).
/// Every write reports "unavailable", so the caller shows its status message (R-CT-2). C# `internal sealed` singleton `Instance`
/// becomes a unit value; there is no shared instance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnavailableClipboardService;

impl ClipboardService for UnavailableClipboardService {
    fn try_set_text(&self, _text: &str) -> bool {
        false
    }
}

/// C# `IPresentationObserver` (AR02a): production seam replacing `MainWindowTestHooks.OnPresented`. The composition root wires it
/// into the presentation sink; tests and benchmarks observe presentation completions through it.
pub trait PresentationObserver {
    /// Called once per completed presentation with the presented file path.
    fn on_presented(&self, path: &str);
}

/// C# `NullPresentationObserver` (default production observer: does nothing; C# `Instance` singleton becomes a unit value).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NullPresentationObserver;

impl PresentationObserver for NullPresentationObserver {
    fn on_presented(&self, _path: &str) {}
}

/// C# `IZoomPromptService` (C-13): asks for a custom click-zoom percentage; replaces `new ClickZoomCustomDialog` in the window.
/// The implementation is platform code (`pr-platform-win::dialogs`, W8-18/W8-19).
pub trait ZoomPromptService {
    /// The chosen percentage, or `None` when the user cancelled (C# `int?` null).
    fn prompt_custom_zoom(&self, current_percent: i32) -> Option<i32>;
}
