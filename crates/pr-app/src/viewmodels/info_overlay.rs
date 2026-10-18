//! Port of `PhotoReview.App.Shared/ViewModels/InfoOverlayViewModel.cs` (196 lines): state of the on-image info
//! overlays (bottom-left file block, bottom-right folder block with the sibling image folders, zoom HUD).
//!
//! Async is replaced by an explicit handshake (D-W8-2): the C# background sibling search becomes a
//! [`SiblingSearch`] request returned from [`InfoOverlayViewModel::refresh`] / `set_folder`; the caller runs the
//! blocking finder on the worker pool and calls [`InfoOverlayViewModel::complete_search`] on the UI thread.
//! A request number (C# `_request`) drops a result for a folder the user already left.
//! Notifications (C# `PropertyChanged`) become the [`InfoOverlayChanges`] bit set (D-W8-1).
//! Stub: every body is `todo!()` (contract W8-06b).

#![allow(unused_variables, dead_code)] // D11: removed in the same commit that removes the last todo!()

use pr_core::settings::app_settings::AppSettings;

/// C# `PartSeparator`: five spaces between the parts of the folder line (layout, not text).
pub const PART_SEPARATOR: &str = "     ";

/// C# `readonly record struct SiblingImageFolders(string? Previous, string? Next)`, declared in
/// `Coordinators/SiblingFolderNavigator.cs` (unit W8-06f). Defined HERE for this unit (open question O1 in the
/// contract): W8-06f must import it from this module and must not define a second copy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SiblingImageFolders {
    /// Folder the PreviousFolder key opens, if any.
    pub previous: Option<String>,
    /// Folder the NextFolder key opens, if any.
    pub next: Option<String>,
}

/// Inputs the C# view model polls through its `Func<>` seams (`hasImage`, `getZoomPercent`). Passed by the caller
/// on every call instead of being stored, so the state stays a pure function of the arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewInputs {
    /// C# `hasImage()`: an image is currently shown.
    pub has_image: bool,
    /// C# `getZoomPercent()`: whole percent, e.g. `100`.
    pub zoom_percent: i32,
}

impl Default for ViewInputs {
    /// C# constructor defaults: `hasImage` always `true`, `getZoomPercent` always `100`.
    fn default() -> Self {
        todo!()
    }
}

/// The properties the C# view model raises `PropertyChanged` for, in C# raise order (`Refresh` order, with
/// `FolderInfoText` last because it is set after the six notifications). The derived `Ord` is this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InfoOverlayProperty {
    IsFileInfoVisible,
    IsFolderInfoVisible,
    FontSize,
    ExifFontSize,
    IsZoomIndicatorVisible,
    ZoomIndicatorText,
    /// Raised only when the text really changes (C# `SetProperty`).
    FolderInfoText,
}

/// Bit set of [`InfoOverlayProperty`] values (D-W8-1 `ChangeSet` shape). `iter` yields them in enum order.
/// Open question O2: the shared `ChangeSet` type is decided by the 08a contract; this local bit set is replaced
/// by it at merge without changing the semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InfoOverlayChanges(u8);

impl InfoOverlayChanges {
    /// No change.
    pub fn none() -> Self {
        todo!()
    }

    /// Adds `property` to the set.
    pub fn insert(&mut self, property: InfoOverlayProperty) {
        todo!()
    }

    /// `true` when `property` is in the set.
    pub fn contains(self, property: InfoOverlayProperty) -> bool {
        todo!()
    }

    /// `true` when nothing is in the set.
    pub fn is_empty(self) -> bool {
        todo!()
    }

    /// The members, in [`InfoOverlayProperty`] order (a concrete `Vec` iterator: the stub's `todo!()` needs it).
    pub fn iter(self) -> std::vec::IntoIter<InfoOverlayProperty> {
        todo!()
    }
}

/// A background sibling search the caller must run (C# `Start(folder)` / `SearchAsync`). The caller passes the
/// same value back to [`InfoOverlayViewModel::complete_search`]. `request` is the C# `_request` counter value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiblingSearch {
    /// Folder to search (exact text as given to `set_folder`, after the whitespace rule).
    pub folder: String,
    /// Request number this search belongs to; a result with an older number is dropped.
    pub request: u64,
}

/// Outcome of the finder call as the C# `SearchAsync` sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchOutcome {
    /// The finder returned these siblings.
    Found(SiblingImageFolders),
    /// The finder threw (any exception, `IOException`, `UnauthorizedAccessException`, ...): the folder is shown
    /// without siblings, never a stuck placeholder. Logging is the caller's job (C# `AppLog.Error` is not here).
    Failed,
    /// The search was cancelled (C# `OperationCanceledException`): nothing changes.
    Cancelled,
}

/// Result of an entry point: which notifications fired (in order) and an optional search to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoOverlayRefresh {
    /// Notifications raised by this call.
    pub changes: InfoOverlayChanges,
    /// Search the caller must run (then `complete_search`), if the folder block needed one.
    pub search: Option<SiblingSearch>,
}

/// Localised text the folder block needs (D-W8-4: text from `pr_l10n` only). Production implementation is
/// backed by the `main.folderInfo.*` catalog keys; tests and the golden use the English fallbacks.
pub trait FolderInfoLabels {
    /// `main.folderInfo.current`, for example `"📁 trip"`.
    fn current(&self, folder_name: &str) -> String;
    /// `main.folderInfo.previous`, for example `"◀ PageUp: trip"`. `key` is the configured key name, untranslated.
    fn previous(&self, key: &str, folder_name: &str) -> String;
    /// `main.folderInfo.next`, for example `"PageDown: trip ▶"`.
    fn next(&self, key: &str, folder_name: &str) -> String;
    /// `main.folderInfo.pending`, the placeholder while a search runs (`"…"`).
    fn pending(&self) -> String;
}

/// C# `InfoOverlayViewModel`. Generic over the label source; holds no settings or image state (passed per call).
#[derive(Debug)]
pub struct InfoOverlayViewModel<L: FolderInfoLabels> {
    labels: L,
    /// Folder currently open; `None` = no folder (C# `_folder`).
    folder: Option<String>,
    /// Folder the siblings belong to; `None` = not computed (C# `_siblingsFolder`).
    siblings_folder: Option<String>,
    siblings: SiblingImageFolders,
    /// Folder a search is running for (C# `_pendingFolder`).
    pending_folder: Option<String>,
    /// Request counter (C# `_request`), bumped by every cancel and every start.
    request: u64,
    /// Current folder line (C# `_folderInfoText`).
    folder_info_text: String,
}

impl<L: FolderInfoLabels> InfoOverlayViewModel<L> {
    /// Fresh view model: no folder, empty text, no search.
    pub fn new(labels: L) -> Self {
        todo!()
    }

    /// C# `IsFileInfoVisible`: `ShowInfoOverlay && ShowFileInfo`.
    pub fn is_file_info_visible(settings: &AppSettings) -> bool {
        todo!()
    }

    /// C# `FontSize`: `settings.InfoOverlayFontSize` as stored (NOT clamped here; the settings layer clamps).
    pub fn font_size(settings: &AppSettings) -> f64 {
        todo!()
    }

    /// C# `ExifFontSize`: `FontSize - 1` (one point smaller).
    pub fn exif_font_size(settings: &AppSettings) -> f64 {
        todo!()
    }

    /// C# `IsZoomIndicatorVisible`: `settings.ShowZoomIndicator && inputs.has_image`.
    pub fn is_zoom_indicator_visible(settings: &AppSettings, inputs: ViewInputs) -> bool {
        todo!()
    }

    /// C# `ZoomIndicatorText`: the integer percent in invariant form followed by `%` (`"100%"`, `"-5%"`).
    pub fn zoom_indicator_text(inputs: ViewInputs) -> String {
        todo!()
    }

    /// C# `IsFolderInfoVisible`: `ShowInfoOverlay && ShowFolderInfo && folder is not null`.
    pub fn is_folder_info_visible(&self, settings: &AppSettings) -> bool {
        todo!()
    }

    /// C# `FolderInfoText`: the current folder line (empty while hidden or with no folder).
    pub fn folder_info_text(&self) -> &str {
        todo!()
    }

    /// C# `SetFolder(string? folder)`: cancels any search (request bump), stores the folder (`None` when it is
    /// null, empty or whitespace only: C# `IsNullOrWhiteSpace`), drops the siblings, then calls `refresh`.
    pub fn set_folder(
        &mut self,
        settings: &AppSettings,
        inputs: ViewInputs,
        folder: Option<&str>,
    ) -> InfoOverlayRefresh {
        todo!()
    }

    /// C# `Refresh()`. Always raises the six notifications; then, in order:
    /// - folder `None` or folder block disabled: cancel the search, text becomes `""`;
    /// - siblings already known for this folder (`siblings_folder == folder`, ordinal): text = `format`;
    /// - otherwise text = placeholder `current(name) + PART_SEPARATOR + pending()` and a search is started
    ///   for the folder unless one is already pending for it (then no new search, no request bump).
    ///
    /// The placeholder is written BEFORE a search is returned (C# comment: the continuation must not be
    /// overwritten by a late placeholder). `FolderInfoText` is added to the changes only when the text differs.
    pub fn refresh(&mut self, settings: &AppSettings, inputs: ViewInputs) -> InfoOverlayRefresh {
        todo!()
    }

    /// C# `SearchAsync` continuation, run on the UI thread after the finder returned.
    /// - `Cancelled`: no change, no search.
    /// - `search.request != self.request`: stale, dropped, no change.
    /// - otherwise: pending cleared, siblings = `Found` value or the default for `Failed`, `siblings_folder` set,
    ///   then `refresh`.
    pub fn complete_search(
        &mut self,
        settings: &AppSettings,
        inputs: ViewInputs,
        search: &SiblingSearch,
        outcome: SearchOutcome,
    ) -> InfoOverlayRefresh {
        todo!()
    }
}

/// C# `DisplayName(string folder)`: `Path.TrimEndingDirectorySeparator` (removes ONE trailing `\` or `/`, never from
/// a root such as `C:\`), then `Path.GetFileName` (the text after the last separator), and if that is empty the
/// whole INPUT is returned (C# `string.IsNullOrEmpty(name) ? folder : name`).
/// Examples: `"C:\photos\trip"` gives `"trip"`; `"C:\photos\trip\"` gives `"trip"`;
/// `"C:\photos\\"` (two trailing separators) gives `"C:\photos\\"`; `"C:\"` gives `"C:\"`; `"/home/user/pics/"` gives `"pics"`.
pub fn display_name(folder: &str) -> String {
    todo!()
}
