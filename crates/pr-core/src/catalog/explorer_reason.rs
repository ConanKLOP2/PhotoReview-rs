//! Port of `PhotoReview.Core/Catalog/ExplorerReason.cs` (C# baseline e441d67a).
//!
//! Stable, language-neutral reason codes for an unavailable/invalid Explorer snapshot
//! ([`ExplorerViewSnapshot::reason`](super::explorer_types::ExplorerViewSnapshot)). A reason is `code` or
//! `code|detail|detail`; the UI maps the code to catalog text and inserts the technical details (exception type,
//! HRESULT, counts) verbatim, so the library never carries user-visible prose.
//!
//! The separator cannot be escaped: a `'|'` inside the code or a detail is replaced by `'/'` by [`format`], which
//! keeps [`parse`] returning exactly the details that were given.
// STUB (contract writer): the bodies are `todo!()`; the porter removes this allow together with the last `todo!()`.
#![allow(unused_variables, dead_code)]

pub const TIMEOUT: &str = "timeout";
pub const CANCELED: &str = "canceled";
/// Detail: exception type name.
pub const QUERY_FAILED: &str = "query-failed";
pub const SHELL_UNAVAILABLE: &str = "shell-unavailable";
/// Details: exception type name, HRESULT.
pub const NATIVE_VIEW_FAILED: &str = "native-view-failed";
/// Detail: number of Explorer windows inspected.
pub const NO_MATCHING_WINDOW: &str = "no-matching-window";
/// Details: COM call name, HRESULT.
pub const COM_CALL_FAILED: &str = "com-call-failed";
/// Details: HRESULT, item count.
pub const EMPTY_VIEW: &str = "empty-view";
pub const EMPTY_PATH: &str = "empty-path";
pub const OUTSIDE_FOLDER: &str = "outside-folder";
pub const DUPLICATE_ITEM: &str = "duplicate-item";
pub const INCOMPLETE_SNAPSHOT: &str = "incomplete-snapshot";

/// Every code above (C# tests find them by reflection; Rust tests iterate this list). Each matches
/// `^[a-z]+(-[a-z]+)*$`.
pub const ALL_CODES: [&str; 12] = [
    TIMEOUT,
    CANCELED,
    QUERY_FAILED,
    SHELL_UNAVAILABLE,
    NATIVE_VIEW_FAILED,
    NO_MATCHING_WINDOW,
    COM_CALL_FAILED,
    EMPTY_VIEW,
    EMPTY_PATH,
    OUTSIDE_FOLDER,
    DUPLICATE_ITEM,
    INCOMPLETE_SNAPSHOT,
];

/// The separator between the code and each detail.
pub const SEPARATOR: char = '|';

/// `ExplorerReason.Format`: joins a code and its details with `'|'`. Every part is cleaned first by replacing each
/// `'|'` with `'/'`. With no details the result is the cleaned code alone (no trailing separator). An empty detail is
/// kept (`format("x", &[""])` is `"x|"`).
pub fn format(code: &str, details: &[&str]) -> String {
    todo!()
}

/// `ExplorerReason.Parse`: splits on every `'|'`; the first piece is the code, the rest are the details (possibly
/// empty strings). A text without `'|'` is returned as its own code with no details; `""` gives code `""` and no
/// details; `"a||b"` gives code `"a"` and details `["", "b"]`.
pub fn parse(reason: &str) -> (String, Vec<String>) {
    todo!()
}
