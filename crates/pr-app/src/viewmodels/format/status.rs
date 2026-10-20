//! Port of `PhotoReview.App.Shared/ViewModels/StatusFormatter.cs` (C# baseline d92d6aae, 117 lines): the status line and the
//! file-size text. Each C# method is one function; the text comes from `pr_l10n` (`ctx.texts`) with the keys named in the
//! doc comment (`pr_l10n::keys::*`). Indexes are 0-based in the API and shown 1-based, as in C#.

use pr_l10n::template::{LocArg, LocValue};

use super::context::{FormatContext, format_custom_fixed, format_grouped_integer};
use super::exif::render;

/// C# `StatusFormatter.FormatFileSize(long bytes)`: "0 byte", "1.5 KB", "2.38 MB" ... Units are `unit.size.*` (`{value}`).
/// Negative input counts as 0. Divide by 1024 while the value is >= 1024 and the unit is below GB (unit 3 is the last).
/// Number format: "0" for bytes (unit 0), "0.##" otherwise, with `ctx.numbers`. Trap T16.
pub fn format_file_size(bytes: i64, ctx: &FormatContext<'_>) -> String {
    let mut value = bytes.max(0) as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < 3 {
        value /= 1024.0;
        unit += 1;
    }
    let max_decimals = if unit == 0 { 0 } else { 2 };
    let number = LocValue::Str(format_custom_fixed(value, max_decimals, ctx.numbers));
    let key = match unit {
        0 => "unit.size.byte",
        1 => "unit.size.kilobyte",
        2 => "unit.size.megabyte",
        _ => "unit.size.gigabyte",
    };
    render(ctx, key, &[LocArg::new("value", number)])
}

/// `StatusFormatter.NoSupportedImages()`: key `status.noSupportedImages`.
pub fn no_supported_images(ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.noSupportedImages", &[])
}

/// `StatusFormatter.NoSupportedImagesButSubfolders(count)`: plural group `status.noSupportedImagesButSubfolders` (`{count}`,
/// `.one` / `.other`). Use `format_plural_with` with the base key; see contract Semantics 8.
pub fn no_supported_images_but_subfolders(count: i32, ctx: &FormatContext<'_>) -> String {
    let args = [LocArg::new("count", LocValue::Int(i64::from(count)))];
    ctx.texts.format_plural_with(
        "status.noSupportedImagesButSubfolders",
        i64::from(count),
        &args,
        ctx.numbers,
    )
}

/// `StatusFormatter.FolderOpenFailed(message)`: key `status.folderOpenFailed` (`{message}`).
pub fn folder_open_failed(message: &str, ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.folderOpenFailed", &[LocArg::new("message", message)])
}

/// `StatusFormatter.IndexOnly(index, count)`: key `status.position` ("{position}/{count}", position = index + 1).
pub fn index_only(index: i32, count: i32, ctx: &FormatContext<'_>) -> String {
    position_text(index, count, ctx)
}

/// `StatusFormatter.Loading(index, count, initialSize)`: key `status.loading` (`{position}`, `{count}`, `{size}`).
pub fn loading(index: i32, count: i32, initial_size: i64, ctx: &FormatContext<'_>) -> String {
    let size = format_file_size(initial_size, ctx);
    render(
        ctx,
        "status.loading",
        &[
            LocArg::new("position", position(index)),
            LocArg::new("count", count),
            LocArg::new("size", size),
        ],
    )
}

/// `StatusFormatter.LoadingFullRes(index, count, initialSize)`: key `status.loadingFullRes`.
pub fn loading_full_res(index: i32, count: i32, initial_size: i64, ctx: &FormatContext<'_>) -> String {
    let size = format_file_size(initial_size, ctx);
    render(
        ctx,
        "status.loadingFullRes",
        &[
            LocArg::new("position", position(index)),
            LocArg::new("count", count),
            LocArg::new("size", size),
        ],
    )
}

/// `StatusFormatter.DecodingRaw()`: key `status.decodingRaw`.
pub fn decoding_raw(ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.decodingRaw", &[])
}

/// `StatusFormatter.Ready(index, count, size, fileName)`: key `status.ready` (`{position}`, `{count}`, `{size}`, `{fileName}`).
pub fn ready(index: i32, count: i32, size: i64, file_name: &str, ctx: &FormatContext<'_>) -> String {
    let size = format_file_size(size, ctx);
    render(
        ctx,
        "status.ready",
        &[
            LocArg::new("position", position(index)),
            LocArg::new("count", count),
            LocArg::new("size", size),
            LocArg::new("fileName", file_name),
        ],
    )
}

/// `StatusFormatter.WithDimensions(index, count, size, width, height, fileName)`: key `status.withDimensions`.
pub fn with_dimensions(
    index: i32,
    count: i32,
    size: i64,
    width: i32,
    height: i32,
    file_name: &str,
    ctx: &FormatContext<'_>,
) -> String {
    let size = format_file_size(size, ctx);
    render(
        ctx,
        "status.withDimensions",
        &[
            LocArg::new("position", position(index)),
            LocArg::new("count", count),
            LocArg::new("size", size),
            LocArg::new("width", width),
            LocArg::new("height", height),
            LocArg::new("fileName", file_name),
        ],
    )
}

/// The seven arguments of `StatusFormatter.Compare`. `left_size` and `right_size` come from [`compare_size_suffix`]
/// (or are empty); `hash_text` comes from [`compare_hash_text`] / [`compare_hash_unknown`] and includes its own leading separator.
#[derive(Debug, Clone, Copy)]
pub struct CompareStatus<'a> {
    pub index: i32,
    pub count: i32,
    pub left_name: &'a str,
    pub left_size: &'a str,
    pub right_name: &'a str,
    pub right_size: &'a str,
    pub hash_text: &'a str,
}

/// `StatusFormatter.Compare(...)`: key `status.compare` (`{position}`, `{count}`, `{leftName}`, `{leftSize}`, `{rightName}`,
/// `{rightSize}`, `{hashText}`). The fragments are inserted verbatim.
pub fn compare(status: &CompareStatus<'_>, ctx: &FormatContext<'_>) -> String {
    render(
        ctx,
        "status.compare",
        &[
            LocArg::new("position", position(status.index)),
            LocArg::new("count", status.count),
            LocArg::new("leftName", status.left_name),
            LocArg::new("leftSize", status.left_size),
            LocArg::new("rightName", status.right_name),
            LocArg::new("rightSize", status.right_size),
            LocArg::new("hashText", status.hash_text),
        ],
    )
}

/// `StatusFormatter.CompareSizeSuffix(bytes)`: key `compare.sizeSuffix` (`{bytes}`), where `{bytes}` is the N0 text of the size
/// (`ctx.group_separator`, see [`super::context::format_grouped_integer`]). Invariant culture: " (1,048,576 B)".
pub fn compare_size_suffix(bytes: i64, ctx: &FormatContext<'_>) -> String {
    let grouped = format_grouped_integer(bytes, ctx.numbers, ctx.group_separator);
    render(ctx, "compare.sizeSuffix", &[LocArg::new("bytes", grouped)])
}

/// `StatusFormatter.CompareHashText(match)`: `None` = hash comparison off (`compare.hash.off`), `Some(true)` = `compare.hash.same`,
/// `Some(false)` = `compare.hash.different`.
pub fn compare_hash_text(same: Option<bool>, ctx: &FormatContext<'_>) -> String {
    let key = match same {
        None => "compare.hash.off",
        Some(true) => "compare.hash.same",
        Some(false) => "compare.hash.different",
    };
    render(ctx, key, &[])
}

/// `StatusFormatter.CompareHashUnknown()`: key `compare.hash.unknown`.
pub fn compare_hash_unknown(ctx: &FormatContext<'_>) -> String {
    render(ctx, "compare.hash.unknown", &[])
}

/// `StatusFormatter.ImageError(fileName, message)`: key `status.imageError` (`{fileName}`, `{message}`).
pub fn image_error(file_name: &str, message: &str, ctx: &FormatContext<'_>) -> String {
    render(
        ctx,
        "status.imageError",
        &[LocArg::new("fileName", file_name), LocArg::new("message", message)],
    )
}

/// `StatusFormatter.NoImagesRemaining()`: key `status.noImagesRemaining`.
pub fn no_images_remaining(ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.noImagesRemaining", &[])
}

/// `StatusFormatter.AllImagesProcessed()`: key `status.allImagesProcessed`.
pub fn all_images_processed(ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.allImagesProcessed", &[])
}

/// `StatusFormatter.CacheCleared()`: key `status.cacheCleared`.
pub fn cache_cleared(ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.cacheCleared", &[])
}

/// `StatusFormatter.DuplicateCheckFailed(message)`: key `status.duplicateCheckFailed` (`{message}`).
pub fn duplicate_check_failed(message: &str, ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.duplicateCheckFailed", &[LocArg::new("message", message)])
}

/// `StatusFormatter.DuplicateCheckCanceledFolderChanged()`: key `status.duplicateCheckCanceledFolderChanged`.
pub fn duplicate_check_canceled_folder_changed(ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.duplicateCheckCanceledFolderChanged", &[])
}

/// `StatusFormatter.DuplicateCheckRunning()`: key `status.duplicateCheckRunning`.
pub fn duplicate_check_running(ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.duplicateCheckRunning", &[])
}

/// `StatusFormatter.DuplicateCheckCanceled()`: key `status.duplicateCheckCanceled`.
pub fn duplicate_check_canceled(ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.duplicateCheckCanceled", &[])
}

/// `StatusFormatter.NoDuplicatesFound()`: key `status.noDuplicatesFound`.
pub fn no_duplicates_found(ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.noDuplicatesFound", &[])
}

/// `StatusFormatter.BatchCanceled()`: key `status.batchCanceled`.
pub fn batch_canceled(ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.batchCanceled", &[])
}

/// `StatusFormatter.BatchDone(succeeded, failures)`: key `status.batchDone` (`{succeeded}`, `{failures}`).
pub fn batch_done(succeeded: i32, failures: i32, ctx: &FormatContext<'_>) -> String {
    render(
        ctx,
        "status.batchDone",
        &[LocArg::new("succeeded", succeeded), LocArg::new("failures", failures)],
    )
}

/// `StatusFormatter.SiblingFolderBoundary(direction)`: `direction > 0` gives `status.siblingFolder.last`, otherwise `.first`.
pub fn sibling_folder_boundary(direction: i32, ctx: &FormatContext<'_>) -> String {
    let key = if direction > 0 {
        "status.siblingFolder.last"
    } else {
        "status.siblingFolder.first"
    };
    render(ctx, key, &[])
}

/// `StatusFormatter.ActionCompleted(actionName)`: key `status.actionCompleted` (`{actionName}`).
pub fn action_completed(action_name: &str, ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.actionCompleted", &[LocArg::new("actionName", action_name)])
}

/// `StatusFormatter.ActionInvalidOperation(actionName)`: key `status.actionInvalidOperation`.
pub fn action_invalid_operation(action_name: &str, ctx: &FormatContext<'_>) -> String {
    render(
        ctx,
        "status.actionInvalidOperation",
        &[LocArg::new("actionName", action_name)],
    )
}

/// `StatusFormatter.ActionNoDestination(actionName)`: key `status.actionNoDestination`.
pub fn action_no_destination(action_name: &str, ctx: &FormatContext<'_>) -> String {
    render(
        ctx,
        "status.actionNoDestination",
        &[LocArg::new("actionName", action_name)],
    )
}

/// `StatusFormatter.ActionFailed(actionName, message)`: key `status.actionFailed` (`{actionName}`, `{message}`). A C# `null`
/// message renders as "" (`LocValue::Null`, contract Semantics 10).
pub fn action_failed(action_name: &str, message: Option<&str>, ctx: &FormatContext<'_>) -> String {
    render(
        ctx,
        "status.actionFailed",
        &[
            LocArg::new("actionName", action_name),
            LocArg::new("message", nullable(message)),
        ],
    )
}

/// `StatusFormatter.CopiedTo(fileName)`: key `status.copiedTo` (`{fileName}`). A `None` name renders as "" (as `action_failed`).
pub fn copied_to(file_name: Option<&str>, ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.copiedTo", &[LocArg::new("fileName", nullable(file_name))])
}

/// `StatusFormatter.NothingToUndo()`: key `status.nothingToUndo`.
pub fn nothing_to_undo(ctx: &FormatContext<'_>) -> String {
    render(ctx, "status.nothingToUndo", &[])
}

/// The 1-based position of a 0-based index. `wrapping_add` keeps C# `int` arithmetic: `i32::MAX + 1` is `i32::MIN` (trap T21).
fn position(index: i32) -> i32 {
    index.wrapping_add(1)
}

/// `status.position` ("{position}/{count}") for a 0-based index and a count.
pub(super) fn position_text(index: i32, count: i32, ctx: &FormatContext<'_>) -> String {
    render(
        ctx,
        "status.position",
        &[LocArg::new("position", position(index)), LocArg::new("count", count)],
    )
}

/// A C# `string?` argument: `None` renders as "" (`LocValue::Null`), `Some` verbatim (`Some("")` is also "").
fn nullable(text: Option<&str>) -> LocValue {
    text.map_or(LocValue::Null, LocValue::from)
}
