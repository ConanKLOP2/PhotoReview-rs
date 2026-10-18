//! Port of `PhotoReview.App.Shared/ViewModels/StatusFormatter.cs` (C# baseline d92d6aae, 117 lines): the status line and the
//! file-size text. Each C# method is one function; the text comes from `pr_l10n` (`ctx.texts`) with the keys named in the
//! doc comment (`pr_l10n::keys::*`). Indexes are 0-based in the API and shown 1-based, as in C#.
#![allow(unused_variables, dead_code)] // D11: todo!() bodies only; the porter removes this with the last todo!()

use super::context::FormatContext;

/// C# `StatusFormatter.FormatFileSize(long bytes)`: "0 byte", "1.5 KB", "2.38 MB" ... Units are `unit.size.*` (`{value}`).
/// Negative input counts as 0. Divide by 1024 while the value is >= 1024 and the unit is below GB (unit 3 is the last).
/// Number format: "0" for bytes (unit 0), "0.##" otherwise, with `ctx.numbers`. Trap T16.
pub fn format_file_size(bytes: i64, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: format_file_size, keys unit.size.byte|kilobyte|megabyte|gigabyte")
}

/// `StatusFormatter.NoSupportedImages()`: key `status.noSupportedImages`.
pub fn no_supported_images(ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.noSupportedImages")
}

/// `StatusFormatter.NoSupportedImagesButSubfolders(count)`: plural group `status.noSupportedImagesButSubfolders` (`{count}`,
/// `.one` / `.other`). Use `format_plural_with` with the base key; see contract Semantics 8.
pub fn no_supported_images_but_subfolders(count: i32, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: plural status.noSupportedImagesButSubfolders")
}

/// `StatusFormatter.FolderOpenFailed(message)`: key `status.folderOpenFailed` (`{message}`).
pub fn folder_open_failed(message: &str, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.folderOpenFailed")
}

/// `StatusFormatter.IndexOnly(index, count)`: key `status.position` ("{position}/{count}", position = index + 1).
pub fn index_only(index: i32, count: i32, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.position with index + 1 (wrapping, trap T21)")
}

/// `StatusFormatter.Loading(index, count, initialSize)`: key `status.loading` (`{position}`, `{count}`, `{size}`).
pub fn loading(index: i32, count: i32, initial_size: i64, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.loading")
}

/// `StatusFormatter.LoadingFullRes(index, count, initialSize)`: key `status.loadingFullRes`.
pub fn loading_full_res(index: i32, count: i32, initial_size: i64, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.loadingFullRes")
}

/// `StatusFormatter.DecodingRaw()`: key `status.decodingRaw`.
pub fn decoding_raw(ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.decodingRaw")
}

/// `StatusFormatter.Ready(index, count, size, fileName)`: key `status.ready` (`{position}`, `{count}`, `{size}`, `{fileName}`).
pub fn ready(index: i32, count: i32, size: i64, file_name: &str, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.ready")
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
    todo!("W8-02b: status.withDimensions")
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
    todo!("W8-02b: status.compare")
}

/// `StatusFormatter.CompareSizeSuffix(bytes)`: key `compare.sizeSuffix` (`{bytes}`), where `{bytes}` is the N0 text of the size
/// (`ctx.group_separator`, see [`super::context::format_grouped_integer`]). Invariant culture: " (1,048,576 B)".
pub fn compare_size_suffix(bytes: i64, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: compare.sizeSuffix with N0 grouping (trap T22)")
}

/// `StatusFormatter.CompareHashText(match)`: `None` = hash comparison off (`compare.hash.off`), `Some(true)` = `compare.hash.same`,
/// `Some(false)` = `compare.hash.different`.
pub fn compare_hash_text(same: Option<bool>, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: compare.hash.off / same / different")
}

/// `StatusFormatter.CompareHashUnknown()`: key `compare.hash.unknown`.
pub fn compare_hash_unknown(ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: compare.hash.unknown")
}

/// `StatusFormatter.ImageError(fileName, message)`: key `status.imageError` (`{fileName}`, `{message}`).
pub fn image_error(file_name: &str, message: &str, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.imageError")
}

/// `StatusFormatter.NoImagesRemaining()`: key `status.noImagesRemaining`.
pub fn no_images_remaining(ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.noImagesRemaining")
}

/// `StatusFormatter.AllImagesProcessed()`: key `status.allImagesProcessed`.
pub fn all_images_processed(ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.allImagesProcessed")
}

/// `StatusFormatter.CacheCleared()`: key `status.cacheCleared`.
pub fn cache_cleared(ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.cacheCleared")
}

/// `StatusFormatter.DuplicateCheckFailed(message)`: key `status.duplicateCheckFailed` (`{message}`).
pub fn duplicate_check_failed(message: &str, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.duplicateCheckFailed")
}

/// `StatusFormatter.DuplicateCheckCanceledFolderChanged()`: key `status.duplicateCheckCanceledFolderChanged`.
pub fn duplicate_check_canceled_folder_changed(ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.duplicateCheckCanceledFolderChanged")
}

/// `StatusFormatter.DuplicateCheckRunning()`: key `status.duplicateCheckRunning`.
pub fn duplicate_check_running(ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.duplicateCheckRunning")
}

/// `StatusFormatter.DuplicateCheckCanceled()`: key `status.duplicateCheckCanceled`.
pub fn duplicate_check_canceled(ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.duplicateCheckCanceled")
}

/// `StatusFormatter.NoDuplicatesFound()`: key `status.noDuplicatesFound`.
pub fn no_duplicates_found(ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.noDuplicatesFound")
}

/// `StatusFormatter.BatchCanceled()`: key `status.batchCanceled`.
pub fn batch_canceled(ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.batchCanceled")
}

/// `StatusFormatter.BatchDone(succeeded, failures)`: key `status.batchDone` (`{succeeded}`, `{failures}`).
pub fn batch_done(succeeded: i32, failures: i32, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.batchDone")
}

/// `StatusFormatter.SiblingFolderBoundary(direction)`: `direction > 0` gives `status.siblingFolder.last`, otherwise `.first`.
pub fn sibling_folder_boundary(direction: i32, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.siblingFolder.first / last")
}

/// `StatusFormatter.ActionCompleted(actionName)`: key `status.actionCompleted` (`{actionName}`).
pub fn action_completed(action_name: &str, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.actionCompleted")
}

/// `StatusFormatter.ActionInvalidOperation(actionName)`: key `status.actionInvalidOperation`.
pub fn action_invalid_operation(action_name: &str, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.actionInvalidOperation")
}

/// `StatusFormatter.ActionNoDestination(actionName)`: key `status.actionNoDestination`.
pub fn action_no_destination(action_name: &str, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.actionNoDestination")
}

/// `StatusFormatter.ActionFailed(actionName, message)`: key `status.actionFailed` (`{actionName}`, `{message}`). A C# `null`
/// message renders as "" (`LocValue::Null`, contract Semantics 10).
pub fn action_failed(action_name: &str, message: Option<&str>, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.actionFailed, null message renders empty")
}

/// `StatusFormatter.CopiedTo(fileName)`: key `status.copiedTo` (`{fileName}`). A `None` name renders as "" (as `action_failed`).
pub fn copied_to(file_name: Option<&str>, ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.copiedTo, null name renders empty")
}

/// `StatusFormatter.NothingToUndo()`: key `status.nothingToUndo`.
pub fn nothing_to_undo(ctx: &FormatContext<'_>) -> String {
    todo!("W8-02b: status.nothingToUndo")
}
