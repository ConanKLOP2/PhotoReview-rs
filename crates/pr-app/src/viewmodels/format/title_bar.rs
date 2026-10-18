//! Port of `PhotoReview.App.Shared/ViewModels/TitleBarFormatter.cs` (C# baseline d92d6aae, 72 lines): the content of the main
//! window title bar, for example `Vacation 2024 · 12/340 · IMG_1234.jpg`. The result replaces the folder placeholder of
//! `main.title.withFolder` (the "Photo Review" wrapper is built elsewhere, W8-08d). Shares the value formatting of [`super::exif`]
//! and [`super::status`] so the same value renders identically.
#![allow(unused_variables, dead_code)] // D11: todo!() bodies only; the porter removes this with the last todo!()

use pr_core::model::TitleBarFields;
use pr_core::time::DateTimeUtc;
use pr_imaging::exif_summary::ExifSummary;

use super::context::FormatContext;

/// The C# `TitleBarFormatter.Format` parameters other than the field set and the context.
/// `index` is 0-based (shown 1-based); `count` 0 or `None` means none. `length` is the catalog entry's size in bytes.
#[derive(Debug, Clone, Copy)]
pub struct TitleBarLine<'a> {
    /// Full path of the open folder. The caller has checked it is not blank; blank input still gives no folder parts.
    pub folder_path: &'a str,
    /// 0-based index of the presented photo; `None` = none presented.
    pub index: Option<i32>,
    /// Number of photos in the catalog; `None` or 0 = none.
    pub count: Option<i32>,
    /// Presented photo's file name; `None` = none presented.
    pub file_name: Option<&'a str>,
    /// Presented photo's size in bytes (catalog entry, no disk read); `None` = unknown.
    pub length: Option<i64>,
    /// Original (post-orientation) pixel size; 0 = unknown.
    pub width: i32,
    pub height: i32,
    /// Presented photo's last-write time, already converted to local wall-clock time by the caller (see `ExifLine::modified_local`).
    pub modified_local: Option<DateTimeUtc>,
    /// EXIF read during the decode; `None` = none.
    pub exif: Option<&'a ExifSummary>,
}

/// C# `TitleBarFormatter.Format(fields, folderPath, index, count, fileName, length, width, height, modifiedUtc, exif, provider)`.
/// Parts in the fixed order FolderName, FolderPath, IndexCount, FileName, FileSize, Dimensions, ModifiedDate, DateTaken, Camera,
/// Lens, Iso, FocalLength, Aperture, ShutterSpeed. Missing values are skipped; joined with `super::exif::SEPARATOR`.
/// Returns "" when nothing is shown (the caller falls back to the folder name). Traps T13 to T15.
pub fn format_title_bar(fields: TitleBarFields, line: &TitleBarLine<'_>, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: format_title_bar, see contract Semantics 11 and traps T13 to T15")
}
