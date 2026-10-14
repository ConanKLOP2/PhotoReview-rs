//! Port of `PhotoReview.Core/Catalog/CaptureGroupBuilder.cs` (C# baseline d92d6aae): finds the unambiguous
//! same-folder, same-basename JPEG+RAW captures (with their optional XMP sidecar) and collapses each pair into one
//! catalog entry (PARITY I-17, `RawPairMode`). `CaptureGroup` and `CatalogEntry` themselves live in
//! [`super::types`] (W1-07).
// Temporary (D11): bodies are `todo!()`; the porter removes this line with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use crate::catalog::types::{CaptureGroup, CatalogEntry};
use crate::model::RawPairMode;

/// `CaptureGroupBuilder.Build(imagePaths, sidecarPaths)`.
///
/// Result order = the order in which each (folder, basename) bucket was FIRST seen in `image_paths`.
/// 1. Bucket every image path. Skip a path that is empty/whitespace-only (`trim().is_empty()`). Its extension
///    (`catalog::types::get_extension`) must be in [`JPEG_EXTENSIONS`](super::types::JPEG_EXTENSIONS) (JPEG side) or
///    [`RAW_EXTENSIONS`](super::types::RAW_EXTENSIONS) (RAW side), compared OrdinalIgnoreCase (`extension_in`); every
///    other path (png, tif, xmp, no extension...) is ignored. Bucket key = (`get_directory_name(path)` or `""` when
///    `None`, `get_file_name_without_extension(path)`), both compared OrdinalIgnoreCase (`ignore_case_key`). A bucket
///    remembers the FIRST jpeg path, the FIRST raw path and how many of each it saw.
/// 2. A bucket becomes a group only when it has EXACTLY one JPEG and EXACTLY one RAW (`a.jpg` + `a.jpeg` + `a.arw` is
///    ambiguous: no group at all; case variants `a.jpg`/`A.JPG` count as two JPEGs).
/// 3. Sidecar choice (`sidecar_paths` `None` or no accepted sidecar => `xmp = None`):
///    * Index sidecars first: only paths that are not blank and whose extension equals `.xmp` (OrdinalIgnoreCase);
///      key = (folder as above, `get_file_name_without_extension`), so `a.xmp` is under `("dir","a")` and `a.cr2.xmp`
///      under `("dir","a.cr2")`; each key keeps its paths in input order.
///    * If the group's own key (folder, basename) exists: that is the "base-named" sidecar. Exactly one path => it is
///      the sidecar; two or more (`a.xmp` + `A.XMP`) => NO sidecar and the file-extension sidecars are NOT consulted.
///    * Otherwise look up (folder of the JPEG, `get_file_name(jpeg)`) then (folder of the RAW, `get_file_name(raw)`) and
///      collect all their paths: exactly one candidate in total => it; zero or two or more => none.
///    * `CaptureGroup::new(jpeg, raw, xmp)` cannot fail here (all three paths were checked non-blank); never `unwrap()`
///      it: skip the bucket in the impossible `Err` case.
pub fn build(image_paths: &[String], sidecar_paths: Option<&[String]>) -> Vec<CaptureGroup> {
    todo!()
}

/// `CaptureGroupBuilder.GroupEntries(entries, mode, sidecarPaths)`: replaces each pair's members with ONE entry.
/// * `mode == RawPairMode::Separate` => a plain clone of `entries`, `build` is not called.
/// * `groups = build(paths of entries, sidecar_paths)`; empty => a plain clone of `entries`.
/// * Index entries by path (OrdinalIgnoreCase, FIRST entry of a path wins) and groups by each of their two member
///   paths (OrdinalIgnoreCase; a later group overwrites an earlier one on a shared member path).
/// * Walk `entries` in order: an entry that is not a group member is kept as is. The FIRST member seen of a group
///   emits the group's single entry at THAT position (not at the representative's position): the entry found for
///   `group.get_representative_path(mode)` cloned with `capture_group = Some(group)` (so it keeps the
///   representative's own length, date and size); later members of the same group emit nothing. If the representative's
///   path has no entry in `entries`, nothing is emitted for the group (the pair disappears). Identify "the same group" by
///   its index in `groups`, not by value.
pub fn group_entries(
    entries: &[CatalogEntry],
    mode: RawPairMode,
    sidecar_paths: Option<&[String]>,
) -> Vec<CatalogEntry> {
    todo!()
}
