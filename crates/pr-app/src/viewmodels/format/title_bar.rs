//! Port of `PhotoReview.App.Shared/ViewModels/TitleBarFormatter.cs` (C# baseline d92d6aae, 72 lines): the content of the main
//! window title bar, for example `Vacation 2024 · 12/340 · IMG_1234.jpg`. The result replaces the folder placeholder of
//! `main.title.withFolder` (the "Photo Review" wrapper is built elsewhere, W8-08d). Shares the value formatting of [`super::exif`]
//! and [`super::status`] so the same value renders identically.

use pr_core::model::TitleBarFields;
use pr_core::text::dotnet_path::{get_file_name, trim_ending_directory_separator};
use pr_core::time::DateTimeUtc;
use pr_imaging::exif_summary::ExifSummary;

use super::context::{FormatContext, is_blank};
use super::exif::{
    SEPARATOR, aperture_text, camera_text, date_taken_text, dimensions_text, focal_length_text, iso_text,
    modified_text, shutter_text,
};
use super::status::{format_file_size, position_text};

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
    let exif = line.exif;
    let folder_shown = !is_blank(line.folder_path);
    let mut parts: Vec<String> = Vec::new();

    if fields.contains(TitleBarFields::FOLDER_NAME) && folder_shown {
        parts.push(folder_name(line.folder_path).to_owned());
    }
    if fields.contains(TitleBarFields::FOLDER_PATH) && folder_shown {
        parts.push(line.folder_path.to_owned());
    }
    if fields.contains(TitleBarFields::INDEX_COUNT)
        && let Some(index) = line.index
        && let Some(count) = line.count.filter(|c| *c > 0)
    {
        parts.push(position_text(index, count, ctx));
    }
    if fields.contains(TitleBarFields::FILE_NAME)
        && let Some(name) = line.file_name
        && !is_blank(name)
    {
        parts.push(name.to_owned());
    }
    if fields.contains(TitleBarFields::FILE_SIZE)
        && let Some(length) = line.length
    {
        parts.push(format_file_size(length, ctx));
    }
    if fields.contains(TitleBarFields::DIMENSIONS) && line.width > 0 && line.height > 0 {
        parts.push(dimensions_text(line.width, line.height, ctx));
    }
    if fields.contains(TitleBarFields::MODIFIED_DATE)
        && let Some(modified) = line.modified_local
    {
        parts.push(modified_text(modified, ctx));
    }
    if fields.contains(TitleBarFields::DATE_TAKEN)
        && let Some(date) = exif.and_then(|e| e.date_taken)
    {
        parts.push(date_taken_text(date, ctx));
    }
    if fields.contains(TitleBarFields::CAMERA)
        && let Some(camera) = camera_text(
            exif.and_then(|e| e.camera_make.as_deref()),
            exif.and_then(|e| e.camera_model.as_deref()),
        )
    {
        parts.push(camera);
    }
    if fields.contains(TitleBarFields::LENS)
        && let Some(lens) = exif.and_then(|e| e.lens_model.as_deref())
    {
        parts.push(lens.to_owned());
    }
    if fields.contains(TitleBarFields::ISO)
        && let Some(iso) = exif.and_then(|e| e.iso).filter(|v| *v > 0)
    {
        parts.push(iso_text(iso, ctx));
    }
    if fields.contains(TitleBarFields::FOCAL_LENGTH)
        && let Some(focal) = exif.and_then(|e| e.focal_length).filter(|r| r.value() > 0.0)
    {
        parts.push(focal_length_text(focal.value(), ctx));
    }
    if fields.contains(TitleBarFields::APERTURE)
        && let Some(aperture) = exif.and_then(|e| e.f_number).filter(|r| r.value() > 0.0)
    {
        parts.push(aperture_text(aperture.value(), ctx));
    }
    if fields.contains(TitleBarFields::SHUTTER_SPEED)
        && let Some(shutter) = shutter_text(exif.and_then(|e| e.exposure_time), ctx)
    {
        parts.push(shutter);
    }

    parts.join(SEPARATOR)
}

/// The last name of the folder path (trailing separator ignored). A drive root such as `D:\` has no name of its own,
/// so the folder path itself is shown instead of an empty text (trap T13).
fn folder_name(folder_path: &str) -> &str {
    let name = get_file_name(trim_ending_directory_separator(folder_path));
    if name.is_empty() { folder_path } else { name }
}
