//! Port of the data part of `PhotoReview.Shell.Win32/Overlay/OverlayModel.cs` (WP-23): an immutable snapshot of what the
//! overlay needs from the view models. Panels never see a view model; tests build this record directly.
//!
//! Baseline `d92d6aae`. Owned by W8-15a. The C# `static From(MainViewModel)` is NOT ported here: it reads `MainViewModel`,
//! `InfoOverlayViewModel` and `CompareViewModel`, which are later units. It moves to the presenter that owns those view models
//! (open question O2 in the contract; the lead decides the owner).
//!
//! Traps (contract section "Traps"): T1 the record equality is NaN-aware on the two font sizes (manual `PartialEq`);
//! T8 the defaults are not zeros: `font_size = 12`, `exif_font_size = 11`, and every flag is hidden (`false`).

use crate::input::primitives::f64_same;

/// C# `OverlayModel` (`internal sealed record`, `init` properties). Strings are never null in Rust; the C# `?? string.Empty`
/// mapping is the caller's job (and is `""` in the defaults).
#[derive(Debug, Clone)]
pub struct OverlayModel {
    /// C# `HasImages`: a folder with images is open (forces toolbar and overlay visible when `false`).
    pub has_images: bool,
    /// C# `StatusNeedsAttention`: a message the user must read is shown (keeps the overlay visible).
    pub status_needs_attention: bool,
    /// C# `CompareVisible` (`Compare.IsVisible`).
    pub compare_visible: bool,
    /// C# `StatusPanelVisible` (`IsStatusPanelVisible`).
    pub status_panel_visible: bool,
    /// C# `FileInfoVisible` (`InfoOverlay.IsFileInfoVisible`, the status line).
    pub file_info_visible: bool,
    /// C# `StatusText`.
    pub status_text: String,
    /// C# `FontSize` (`InfoOverlay.FontSize`), default `12`.
    pub font_size: f64,
    /// C# `ExifFontSize` (`InfoOverlay.ExifFontSize`, FontSize - 1), default `11`.
    pub exif_font_size: f64,
    /// C# `ExifVisible` (`IsExifLineVisible`).
    pub exif_visible: bool,
    /// C# `ExifText`.
    pub exif_text: String,
    /// C# `HasSkippedEntries`.
    pub has_skipped_entries: bool,
    /// C# `SkippedWarningText`.
    pub skipped_warning_text: String,
    /// C# `FolderInfoVisible` (`InfoOverlay.IsFolderInfoVisible`).
    pub folder_info_visible: bool,
    /// C# `FolderInfoText`.
    pub folder_info_text: String,
    /// C# `ZoomIndicatorVisible` (`InfoOverlay.IsZoomIndicatorVisible`).
    pub zoom_indicator_visible: bool,
    /// C# `ZoomIndicatorText`.
    pub zoom_indicator_text: String,
    /// C# `CapturePairVisible` (the view model has a capture pair for the current image).
    pub capture_pair_visible: bool,
    /// C# `CapturePairText` (`""` when there is no pair).
    pub capture_pair_text: String,
    /// C# `CapturePairToolTip` (`""` when there is no pair).
    pub capture_pair_tool_tip: String,
}

impl Default for OverlayModel {
    /// C# property initialisers: `FontSize = 12`, `ExifFontSize = 11`, every flag `false`, every string `""`.
    fn default() -> Self {
        Self {
            has_images: false,
            status_needs_attention: false,
            compare_visible: false,
            status_panel_visible: false,
            file_info_visible: false,
            status_text: String::new(),
            font_size: 12.0,
            exif_font_size: 11.0,
            exif_visible: false,
            exif_text: String::new(),
            has_skipped_entries: false,
            skipped_warning_text: String::new(),
            folder_info_visible: false,
            folder_info_text: String::new(),
            zoom_indicator_visible: false,
            zoom_indicator_text: String::new(),
            capture_pair_visible: false,
            capture_pair_text: String::new(),
            capture_pair_tool_tip: String::new(),
        }
    }
}

impl PartialEq for OverlayModel {
    /// C# record equality: the two font sizes compare with `double.Equals` (NaN equals NaN, `0.0 == -0.0`, see
    /// `f64_same`); every other field compares normally.
    fn eq(&self, other: &Self) -> bool {
        self.has_images == other.has_images
            && self.status_needs_attention == other.status_needs_attention
            && self.compare_visible == other.compare_visible
            && self.status_panel_visible == other.status_panel_visible
            && self.file_info_visible == other.file_info_visible
            && self.status_text == other.status_text
            && f64_same(self.font_size, other.font_size)
            && f64_same(self.exif_font_size, other.exif_font_size)
            && self.exif_visible == other.exif_visible
            && self.exif_text == other.exif_text
            && self.has_skipped_entries == other.has_skipped_entries
            && self.skipped_warning_text == other.skipped_warning_text
            && self.folder_info_visible == other.folder_info_visible
            && self.folder_info_text == other.folder_info_text
            && self.zoom_indicator_visible == other.zoom_indicator_visible
            && self.zoom_indicator_text == other.zoom_indicator_text
            && self.capture_pair_visible == other.capture_pair_visible
            && self.capture_pair_text == other.capture_pair_text
            && self.capture_pair_tool_tip == other.capture_pair_tool_tip
    }
}
