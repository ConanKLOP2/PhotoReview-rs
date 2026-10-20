//! Port of `PhotoReview.App.Shared/ViewModels/ExifFormatter.cs` (C# baseline d92d6aae, 113 lines): the photo information line
//! (`IMG_1234.jpg · Taken: 05/01/2024 14:03 · 6000×4000 · Canon EOS R5 · RF24-70mm F2.8 · ISO 400 · 50 mm · f/2.8 · 1/250 s`).
//! Pure: the EXIF values, the file name, the local wall clock and the display culture come in as arguments.

use pr_core::model::ExifInfoFields;
use pr_core::time::DateTimeUtc;
use pr_imaging::exif_summary::{ExifRational, ExifSummary};
use pr_l10n::template::{LocArg, LocValue};

use super::context::{FormatContext, format_custom_fixed, is_blank, starts_with_ignore_case};

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
    let exif = line.exif;
    let mut parts: Vec<String> = Vec::new();

    if fields.contains(ExifInfoFields::FILE_NAME)
        && let Some(name) = line.file_name
        && !is_blank(name)
    {
        parts.push(name.to_owned());
    }
    if fields.contains(ExifInfoFields::DATE_TAKEN)
        && let Some(date) = exif.and_then(|e| e.date_taken)
    {
        parts.push(date_taken_text(date, ctx));
    }
    if fields.contains(ExifInfoFields::MODIFIED_DATE)
        && let Some(modified) = line.modified_local
    {
        parts.push(modified_text(modified, ctx));
    }
    if fields.contains(ExifInfoFields::DIMENSIONS) && line.width > 0 && line.height > 0 {
        parts.push(dimensions_text(line.width, line.height, ctx));
    }
    if fields.contains(ExifInfoFields::DIMENSIONS) && line.raw_preview_width > 0 && line.raw_preview_height > 0 {
        parts.push(raw_preview_text(line.raw_preview_width, line.raw_preview_height, ctx));
    }
    if fields.contains(ExifInfoFields::CAMERA)
        && let Some(camera) = camera_text(
            exif.and_then(|e| e.camera_make.as_deref()),
            exif.and_then(|e| e.camera_model.as_deref()),
        )
    {
        parts.push(camera);
    }
    if fields.contains(ExifInfoFields::LENS)
        && let Some(lens) = exif.and_then(|e| e.lens_model.as_deref())
    {
        parts.push(lens.to_owned());
    }
    if fields.contains(ExifInfoFields::ISO)
        && let Some(iso) = exif.and_then(|e| e.iso).filter(|v| *v > 0)
    {
        parts.push(iso_text(iso, ctx));
    }
    if fields.contains(ExifInfoFields::FOCAL_LENGTH)
        && let Some(focal) = exif.and_then(|e| e.focal_length).filter(|r| r.value() > 0.0)
    {
        parts.push(focal_length_text(focal.value(), ctx));
    }
    if fields.contains(ExifInfoFields::APERTURE)
        && let Some(aperture) = exif.and_then(|e| e.f_number).filter(|r| r.value() > 0.0)
    {
        parts.push(aperture_text(aperture.value(), ctx));
    }
    if fields.contains(ExifInfoFields::SHUTTER_SPEED)
        && let Some(shutter) = shutter_text(exif.and_then(|e| e.exposure_time), ctx)
    {
        parts.push(shutter);
    }

    parts.join(SEPARATOR)
}

/// C# `ExifFormatter.FormatDateTime(date, provider)` (internal, also used by the title bar): `DateText::general`, and when that
/// gives `None` the invariant text `DateText::invariant_general`. One bad date never blanks the line.
pub fn format_date_time(date: DateTimeUtc, ctx: &FormatContext<'_>) -> String {
    ctx.dates
        .general(date)
        .unwrap_or_else(|| ctx.dates.invariant_general(date))
}

/// C# `ExifFormatter.CameraText(make, model)` (internal): the camera line without repeating the make.
/// Blank parts are absent (`IsNullOrWhiteSpace`); a blank model returns the make unchanged (not trimmed), and so on.
/// See contract Semantics 6 for the exact rule and trap T8.
pub fn camera_text(make: Option<&str>, model: Option<&str>) -> Option<String> {
    let make = make.filter(|text| !is_blank(text));
    let Some(model) = model.filter(|text| !is_blank(text)) else {
        return make.map(str::to_owned);
    };
    let Some(make) = make else {
        return Some(model.to_owned());
    };
    if starts_with_ignore_case(model, make) {
        return Some(model.to_owned());
    }
    let make_word = match make.find(' ') {
        Some(space) if space > 0 => &make[..space],
        _ => make,
    };
    if starts_with_ignore_case(model, make_word) {
        Some(model.to_owned())
    } else {
        Some(format!("{make} {model}"))
    }
}

/// C# `ExifFormatter.ShutterText(exposure, provider)` (internal): "1/250 s" for short exposures, decimal seconds otherwise.
/// Returns `None` unless numerator and denominator are both > 0. See contract Semantics 7 and traps T2, T5.
pub fn shutter_text(exposure: Option<ExifRational>, ctx: &FormatContext<'_>) -> Option<String> {
    let time = exposure.filter(|t| t.numerator > 0 && t.denominator > 0)?;
    if time.numerator == 1 && time.denominator > 1 {
        return Some(render(
            ctx,
            "exif.shutterFraction",
            &[LocArg::new("denominator", LocValue::UInt(u64::from(time.denominator)))],
        ));
    }
    let seconds = time.value();
    if seconds < 0.5 {
        let reciprocal = 1.0 / seconds;
        let denominator = reciprocal.round();
        if seconds >= 0.1 && (reciprocal - denominator).abs() > denominator * 0.01 {
            return Some(seconds_text(seconds, 2, ctx));
        }
        return Some(render(
            ctx,
            "exif.shutterFraction",
            &[LocArg::new(
                "denominator",
                LocValue::Str(format_custom_fixed(denominator, 0, ctx.numbers)),
            )],
        ));
    }
    Some(seconds_text(seconds, 1, ctx))
}

/// `exif.shutterSeconds` with the value in the custom format of `max_decimals` decimals.
fn seconds_text(seconds: f64, max_decimals: u8, ctx: &FormatContext<'_>) -> String {
    render(
        ctx,
        "exif.shutterSeconds",
        &[LocArg::new(
            "value",
            LocValue::Str(format_custom_fixed(seconds, max_decimals, ctx.numbers)),
        )],
    )
}

/// `exif.dateTaken` ("Taken: {value}") with the date text of [`format_date_time`]. Shared with the title bar.
pub(super) fn date_taken_text(date: DateTimeUtc, ctx: &FormatContext<'_>) -> String {
    render(
        ctx,
        "exif.dateTaken",
        &[LocArg::new("value", LocValue::Str(format_date_time(date, ctx)))],
    )
}

/// `exif.modified` ("Modified: {value}") with the local wall clock of the file. Shared with the title bar.
pub(super) fn modified_text(date: DateTimeUtc, ctx: &FormatContext<'_>) -> String {
    render(
        ctx,
        "exif.modified",
        &[LocArg::new("value", LocValue::Str(format_date_time(date, ctx)))],
    )
}

/// `exif.dimensions` ("{width}×{height}"). Shared with the title bar.
pub(super) fn dimensions_text(width: i32, height: i32, ctx: &FormatContext<'_>) -> String {
    render(
        ctx,
        "exif.dimensions",
        &[
            LocArg::new("width", LocValue::Int(i64::from(width))),
            LocArg::new("height", LocValue::Int(i64::from(height))),
        ],
    )
}

/// `overlay.rawPreview` ("RAW preview {width}×{height}"), right after the dimensions part.
pub(super) fn raw_preview_text(width: i32, height: i32, ctx: &FormatContext<'_>) -> String {
    render(
        ctx,
        "overlay.rawPreview",
        &[
            LocArg::new("width", LocValue::Int(i64::from(width))),
            LocArg::new("height", LocValue::Int(i64::from(height))),
        ],
    )
}

/// `exif.iso` ("ISO {value}"). Shared with the title bar.
pub(super) fn iso_text(iso: i32, ctx: &FormatContext<'_>) -> String {
    render(ctx, "exif.iso", &[LocArg::new("value", LocValue::Int(i64::from(iso)))])
}

/// `exif.focalLength` ("{value} mm") with the value in the "0.#" format. Shared with the title bar.
pub(super) fn focal_length_text(value: f64, ctx: &FormatContext<'_>) -> String {
    render(
        ctx,
        "exif.focalLength",
        &[LocArg::new(
            "value",
            LocValue::Str(format_custom_fixed(value, 1, ctx.numbers)),
        )],
    )
}

/// `exif.aperture` ("f/{value}") with the value in the "0.#" format. Shared with the title bar.
pub(super) fn aperture_text(value: f64, ctx: &FormatContext<'_>) -> String {
    render(
        ctx,
        "exif.aperture",
        &[LocArg::new(
            "value",
            LocValue::Str(format_custom_fixed(value, 1, ctx.numbers)),
        )],
    )
}

/// One catalog text rendered with the display culture (`Localizer::format_with`).
pub(super) fn render(ctx: &FormatContext<'_>, key: &str, args: &[LocArg]) -> String {
    ctx.texts.format_with(key, args, ctx.numbers)
}
