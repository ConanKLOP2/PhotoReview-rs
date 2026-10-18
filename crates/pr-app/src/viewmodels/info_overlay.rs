//! Port of `PhotoReview.App.Shared/ViewModels/InfoOverlayViewModel.cs` (196 lines): state of the on-image info
//! overlays (bottom-left file block, bottom-right folder block with the sibling image folders, zoom HUD).
//!
//! Async is replaced by an explicit handshake (D-W8-2): the C# background sibling search becomes a
//! [`SiblingSearch`] request returned from [`InfoOverlayViewModel::refresh`] / `set_folder`; the caller runs the
//! blocking finder on the worker pool and calls [`InfoOverlayViewModel::complete_search`] on the UI thread.
//! A request number (C# `_request`) drops a result for a folder the user already left.
//! Notifications (C# `PropertyChanged`) become the [`InfoOverlayChanges`] bit set (D-W8-1).

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
        Self {
            has_image: true,
            zoom_percent: 100,
        }
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

/// Every [`InfoOverlayProperty`], in enum (C# raise) order.
const ALL_PROPERTIES: [InfoOverlayProperty; 7] = [
    InfoOverlayProperty::IsFileInfoVisible,
    InfoOverlayProperty::IsFolderInfoVisible,
    InfoOverlayProperty::FontSize,
    InfoOverlayProperty::ExifFontSize,
    InfoOverlayProperty::IsZoomIndicatorVisible,
    InfoOverlayProperty::ZoomIndicatorText,
    InfoOverlayProperty::FolderInfoText,
];

/// Bit set of [`InfoOverlayProperty`] values (D-W8-1 `ChangeSet` shape). `iter` yields them in enum order.
/// Open question O2: the shared `ChangeSet` type is decided by the 08a contract; this local bit set is replaced
/// by it at merge without changing the semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InfoOverlayChanges(u8);

impl InfoOverlayChanges {
    /// No change.
    pub fn none() -> Self {
        Self(0)
    }

    /// The bit of `property` (one bit per discriminant, 0 to 6).
    fn bit(property: InfoOverlayProperty) -> u8 {
        1 << (property as u8)
    }

    /// Adds `property` to the set.
    pub fn insert(&mut self, property: InfoOverlayProperty) {
        self.0 |= Self::bit(property);
    }

    /// `true` when `property` is in the set.
    pub fn contains(self, property: InfoOverlayProperty) -> bool {
        self.0 & Self::bit(property) != 0
    }

    /// `true` when nothing is in the set.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The members, in [`InfoOverlayProperty`] order (a concrete `Vec` iterator, so the return type is nameable).
    pub fn iter(self) -> std::vec::IntoIter<InfoOverlayProperty> {
        ALL_PROPERTIES
            .into_iter()
            .filter(|property| self.contains(*property))
            .collect::<Vec<_>>()
            .into_iter()
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

/// A refresh that changed nothing and starts no search.
fn nothing_changed() -> InfoOverlayRefresh {
    InfoOverlayRefresh {
        changes: InfoOverlayChanges::none(),
        search: None,
    }
}

impl<L: FolderInfoLabels> InfoOverlayViewModel<L> {
    /// Fresh view model: no folder, empty text, no search.
    pub fn new(labels: L) -> Self {
        Self {
            labels,
            folder: None,
            siblings_folder: None,
            siblings: SiblingImageFolders::default(),
            pending_folder: None,
            request: 0,
            folder_info_text: String::new(),
        }
    }

    /// C# `IsFileInfoVisible`: `ShowInfoOverlay && ShowFileInfo`.
    pub fn is_file_info_visible(settings: &AppSettings) -> bool {
        settings.show_info_overlay && settings.show_file_info
    }

    /// C# `FontSize`: `settings.InfoOverlayFontSize` as stored (NOT clamped here; the settings layer clamps).
    pub fn font_size(settings: &AppSettings) -> f64 {
        settings.info_overlay_font_size
    }

    /// C# `ExifFontSize`: `FontSize - 1` (one point smaller).
    pub fn exif_font_size(settings: &AppSettings) -> f64 {
        Self::font_size(settings) - 1.0
    }

    /// C# `IsZoomIndicatorVisible`: `settings.ShowZoomIndicator && inputs.has_image`.
    pub fn is_zoom_indicator_visible(settings: &AppSettings, inputs: ViewInputs) -> bool {
        settings.show_zoom_indicator && inputs.has_image
    }

    /// C# `ZoomIndicatorText`: the integer percent in invariant form followed by `%` (`"100%"`, `"-5%"`).
    pub fn zoom_indicator_text(inputs: ViewInputs) -> String {
        format!("{}%", inputs.zoom_percent)
    }

    /// C# `IsFolderInfoVisible`: `ShowInfoOverlay && ShowFolderInfo && folder is not null`.
    pub fn is_folder_info_visible(&self, settings: &AppSettings) -> bool {
        settings.show_info_overlay && settings.show_folder_info && self.folder.is_some()
    }

    /// C# `FolderInfoText`: the current folder line (empty while hidden or with no folder).
    pub fn folder_info_text(&self) -> &str {
        &self.folder_info_text
    }

    /// C# `SetFolder(string? folder)`: cancels any search (request bump), stores the folder (`None` when it is
    /// null, empty or whitespace only: C# `IsNullOrWhiteSpace`), drops the siblings, then calls `refresh`.
    pub fn set_folder(
        &mut self,
        settings: &AppSettings,
        inputs: ViewInputs,
        folder: Option<&str>,
    ) -> InfoOverlayRefresh {
        self.bump_request();
        self.pending_folder = None;
        self.folder = folder.filter(|text| !text.trim().is_empty()).map(str::to_owned);
        self.siblings_folder = None;
        self.siblings = SiblingImageFolders::default();
        self.refresh(settings, inputs)
    }

    /// C# `Refresh()`. Always raises the six notifications; then, in order:
    /// - folder `None` or folder block disabled: cancel the search, text becomes `""`;
    /// - siblings already known for this folder (`siblings_folder == folder`, ordinal): text = `format`;
    /// - otherwise text = placeholder `current(name) + PART_SEPARATOR + pending()` and a search is started
    ///   for the folder unless one is already pending for it (then no new search, no request bump).
    ///
    /// The placeholder is written BEFORE a search is returned (C# comment: the continuation must not be
    /// overwritten by a late placeholder). `FolderInfoText` is added to the changes only when the text differs.
    pub fn refresh(&mut self, settings: &AppSettings, _inputs: ViewInputs) -> InfoOverlayRefresh {
        let mut changes = InfoOverlayChanges::none();
        changes.insert(InfoOverlayProperty::IsFileInfoVisible);
        changes.insert(InfoOverlayProperty::IsFolderInfoVisible);
        changes.insert(InfoOverlayProperty::FontSize);
        changes.insert(InfoOverlayProperty::ExifFontSize);
        changes.insert(InfoOverlayProperty::IsZoomIndicatorVisible);
        changes.insert(InfoOverlayProperty::ZoomIndicatorText);

        let mut search = None;
        let folder = self.folder.clone();
        let new_text = match folder {
            Some(folder) if self.is_folder_info_visible(settings) => {
                if self.siblings_folder.as_deref() == Some(folder.as_str()) {
                    self.format_folder_line(settings, &folder)
                } else {
                    // Placeholder first, so a caller that completes the search on this thread cannot be
                    // overwritten by a late placeholder (contract trap T11).
                    let text = format!(
                        "{}{}{}",
                        self.labels.current(&display_name(&folder)),
                        PART_SEPARATOR,
                        self.labels.pending()
                    );
                    if self.pending_folder.as_deref() != Some(folder.as_str()) {
                        self.bump_request();
                        self.pending_folder = Some(folder.clone());
                        search = Some(SiblingSearch {
                            folder,
                            request: self.request,
                        });
                    }
                    text
                }
            }
            _ => {
                self.bump_request();
                self.pending_folder = None;
                String::new()
            }
        };

        if self.folder_info_text != new_text {
            changes.insert(InfoOverlayProperty::FolderInfoText);
        }
        self.folder_info_text = new_text;
        InfoOverlayRefresh { changes, search }
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
        let siblings = match outcome {
            SearchOutcome::Cancelled => return nothing_changed(),
            SearchOutcome::Found(found) => found,
            SearchOutcome::Failed => SiblingImageFolders::default(),
        };
        if search.request != self.request {
            return nothing_changed();
        }
        self.pending_folder = None;
        self.siblings = siblings;
        self.siblings_folder = Some(search.folder.clone());
        self.refresh(settings, inputs)
    }

    /// C# `Format(folder, siblings)`: previous part (if the sibling exists and its key is not blank), the current
    /// folder, next part (same rule), joined with [`PART_SEPARATOR`]. Keys are trimmed (contract semantic 8).
    fn format_folder_line(&self, settings: &AppSettings, folder: &str) -> String {
        let mut parts: Vec<String> = Vec::new();

        let previous_key = settings.shortcuts.previous_folder.trim();
        if let Some(previous) = self.siblings.previous.as_deref().filter(|_| !previous_key.is_empty()) {
            parts.push(self.labels.previous(previous_key, &display_name(previous)));
        }

        parts.push(self.labels.current(&display_name(folder)));

        let next_key = settings.shortcuts.next_folder.trim();
        if let Some(next) = self.siblings.next.as_deref().filter(|_| !next_key.is_empty()) {
            parts.push(self.labels.next(next_key, &display_name(next)));
        }

        parts.join(PART_SEPARATOR)
    }

    /// Bumps the request counter (C# `_request++`). Wrapping keeps the counter total; it cannot realistically wrap.
    fn bump_request(&mut self) {
        self.request = self.request.wrapping_add(1);
    }
}

/// `\` or `/`: the two directory separators Windows paths accept (C# `Path.DirectorySeparatorChar` and
/// `AltDirectorySeparatorChar`). Written out so the result does not depend on the host OS.
fn is_separator(character: char) -> bool {
    character == '\\' || character == '/'
}

/// C# `DisplayName(string folder)`: `Path.TrimEndingDirectorySeparator` (removes ONE trailing `\` or `/`, never from
/// a root such as `C:\`), then `Path.GetFileName` (the text after the last separator), and if that is empty the
/// whole INPUT is returned (C# `string.IsNullOrEmpty(name) ? folder : name`).
/// Examples: `"C:\photos\trip"` gives `"trip"`; `"C:\photos\trip\"` gives `"trip"`;
/// `"C:\photos\\"` (two trailing separators) gives `"C:\photos\\"`; `"C:\"` gives `"C:\"`; `"/home/user/pics/"` gives `"pics"`.
pub fn display_name(folder: &str) -> String {
    let chars: Vec<char> = folder.chars().collect();
    let trimmed = &chars[..trimmed_length(&chars)];
    let root = root_length(trimmed);
    // C# `Path.GetFileName`: scan back to the last separator, but never past the root ("C:foo" gives "foo",
    // a UNC share root gives "").
    let mut start = 0;
    for index in (0..trimmed.len()).rev() {
        if index < root || is_separator(trimmed[index]) {
            start = index + 1;
            break;
        }
    }
    if start >= trimmed.len() {
        folder.to_owned()
    } else {
        trimmed[start..].iter().collect()
    }
}

/// `Path.TrimEndingDirectorySeparator`: drops ONE trailing separator unless the whole path is a root.
fn trimmed_length(chars: &[char]) -> usize {
    match chars.last() {
        Some(&last) if is_separator(last) && root_length(chars) != chars.len() => chars.len() - 1,
        _ => chars.len(),
    }
}

/// .NET `PathInternal.IsDevice`: a `\\?\` prefix, or two separators + `.`/`?` + a separator.
fn is_device(chars: &[char]) -> bool {
    let extended = chars.len() >= 4
        && chars[0] == '\\'
        && (chars[1] == '\\' || chars[1] == '?')
        && chars[2] == '?'
        && chars[3] == '\\';
    extended
        || (chars.len() >= 4
            && is_separator(chars[0])
            && is_separator(chars[1])
            && (chars[2] == '.' || chars[2] == '?')
            && is_separator(chars[3]))
}

/// .NET `PathInternal.IsDeviceUNC`: a device prefix followed by `UNC` and a separator.
fn is_device_unc(chars: &[char]) -> bool {
    chars.len() >= 8
        && is_device(chars)
        && is_separator(chars[7])
        && chars[4].eq_ignore_ascii_case(&'U')
        && chars[5].eq_ignore_ascii_case(&'N')
        && chars[6].eq_ignore_ascii_case(&'C')
}

/// .NET `PathInternal.GetRootLength` (Windows): UNC server and share, device roots, `C:` / `C:\`, and a rooted `\`.
fn root_length(chars: &[char]) -> usize {
    let len = chars.len();
    let device = is_device(chars);
    let device_unc = device && is_device_unc(chars);
    let mut index = 0;
    if (!device || device_unc) && len > 0 && is_separator(chars[0]) {
        if device_unc || (len > 1 && is_separator(chars[1])) {
            index = if device_unc { 8 } else { 2 };
            let mut separators_left = 2;
            while index < len {
                if is_separator(chars[index]) {
                    separators_left -= 1;
                    if separators_left == 0 {
                        break;
                    }
                }
                index += 1;
            }
        } else {
            index = 1;
        }
    } else if device {
        index = 4;
        while index < len && !is_separator(chars[index]) {
            index += 1;
        }
        if index < len && index > 4 && is_separator(chars[index]) {
            index += 1;
        }
    } else if len >= 2 && chars[1] == ':' && chars[0].is_ascii_alphabetic() {
        index = 2;
        if len > 2 && is_separator(chars[2]) {
            index += 1;
        }
    }
    index
}
