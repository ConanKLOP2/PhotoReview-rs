//! Port of `PhotoReview.App.Shared/ViewModels/ExifFormatter.cs` (C# baseline d92d6aae, 113 lines): the photo information line
//! (`IMG_1234.jpg · Taken: 05/01/2024 14:03 · 6000×4000 · Canon EOS R5 · RF24-70mm F2.8 · ISO 400 · 50 mm · f/2.8 · 1/250 s`).
//! Pure: the EXIF values, the file name, the local wall clock and the display culture come in as arguments.
#![allow(unused_variables, dead_code)] // D11: todo!() bodies only; the porter removes this with the last todo!()

use pr_core::model::ExifInfoFields;
use pr_core::time::DateTimeUtc;
use pr_imaging::exif_summary::{ExifRational, ExifSummary};

use super::context::FormatContext;

/// C# `ExifFormatter.Separator` (U+00B7 with one space each side). The title bar uses the same separator.
pub const SEPARATOR: &str = " · ";

/// The C# `Format` parameters other than the field set and the context.
/// Width and height are C# `int`: 0 or negative means unknown. A raw preview size counts only when both sides are > 0.
#[derive(Debug, Clone, Copy)]
pub struct ExifLine<'a> {
    /// Shown for `ExifInfoFields::FILE_NAME` when not blank (kept verbatim, not trimmed).
    pub file_name: Option<&'a str>,
    /// Original (post-orientation) pixel size.
    pub width: i32,
    pub height: i32,
    /// EXIF read during the decode; `None` = none (for example an older cache entry).
    pub exif: Option<&'a ExifSummary>,
    /// C# `modifiedUtc.ToLocalTime()`, already applied by the caller: this is the local wall clock. Golden tests pass it
    /// as a `DateTimeKind.Local` value in C#, where `ToLocalTime` is the identity (contract Semantics 9).
    pub modified_local: Option<DateTimeUtc>,
    /// Q-RAW-03: the embedded RAW preview's pixel size; 0 = not a RAW preview.
    pub raw_preview_width: i32,
    pub raw_preview_height: i32,
}

/// C# `ExifFormatter.Format(fields, fileName, width, height, exif, modifiedUtc, provider, rawPreviewWidth, rawPreviewHeight)`.
/// Parts are added in the fixed order FileName, DateTaken, ModifiedDate, Dimensions (+ raw preview), Camera, Lens, Iso,
/// FocalLength, Aperture, ShutterSpeed; missing or non-positive values are skipped (never a placeholder); joined with [`SEPARATOR`].
/// Returns "" when nothing is shown.
pub fn format_exif(fields: ExifInfoFields, line: &ExifLine<'_>, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: format_exif, see contract Semantics 1 to 4 and traps T3 to T7")
}

/// C# `ExifFormatter.FormatDateTime(date, provider)` (internal, also used by the title bar): `DateText::general`, and when that
/// gives `None` the invariant text `DateText::invariant_general`. One bad date never blanks the line.
pub fn format_date_time(date: DateTimeUtc, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: format_date_time, see contract Semantics 5")
}

/// C# `ExifFormatter.CameraText(make, model)` (internal): the camera line without repeating the make.
/// Blank parts are absent (`IsNullOrWhiteSpace`); a blank model returns the make unchanged (not trimmed), and so on.
/// See contract Semantics 6 for the exact rule and trap T8.
pub fn camera_text(make: Option<&str>, model: Option<&str>) -> Option<String> {
    todo!("W8-02b: camera_text, see contract Semantics 6")
}

/// C# `ExifFormatter.ShutterText(exposure, provider)` (internal): "1/250 s" for short exposures, decimal seconds otherwise.
/// Returns `None` unless numerator and denominator are both > 0. See contract Semantics 7 and traps T2, T5.
pub fn shutter_text(exposure: Option<ExifRational>, ctx: &FormatContext<'_>) -> Option<String> {
    todo!("W8-02b: shutter_text, see contract Semantics 7")
}
