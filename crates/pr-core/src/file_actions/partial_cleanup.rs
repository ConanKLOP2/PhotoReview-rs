//! Port of `PhotoReview.Core/FileActions/PartialDestinationCleanup.cs` (C# baseline d92d6aae; RV-C03 / error-handling
//! review, COPY-PARTIAL-01; PARITY F-21).
//!
//! Shared best-effort cleanup for a Copy that failed after possibly creating its destination. The CALLER must only
//! invoke it for a destination this operation created, proven by [`crate::abstractions::CopyCreationProof::destination_created`]
//! (a pre-check that found the path free proves nothing: a foreign file may appear later). `created` is the
//! [`FileStat`] observed right after the copy.

// TEMPORARY (D11): bodies are `todo!()`; the porter removes this line with the last `todo!()`.
#![allow(unused_variables)]

use std::path::Path;

use crate::abstractions::{FileStat, FileSystem};

/// `PartialDestinationCleanup.RemoveIfPartial(fileSystem, destination, sourceSize, created)`.
///
/// Deletes `destination` ONLY while all of these hold, evaluated on ONE `get_file_stat(destination)` result:
/// 1. the stat is `Some` (the file exists);
/// 2. it is provably incomplete: `stat.length < source_size` (STRICT; `source_size` is C# `long`, negative or zero never
///    qualifies; compare without wrapping: `source_size > 0 && stat.length < source_size as u64`);
/// 3. it is still the file this operation created: [`is_same_file`]`(&stat, created)`.
///
/// A complete (equal or longer) file is left for Recovery to judge. Never panics and never reports: a file that cannot
/// be inspected (`get_file_stat` returns `None`) or deleted (`delete` returns `Err`) stays; the caller reports its own
/// original error. Exactly one `get_file_stat` and at most one `delete` call; no other file-system call.
pub fn remove_if_partial(file_system: &dyn FileSystem, destination: &Path, source_size: i64, created: &FileStat) {
    todo!("W3-08")
}

/// `PartialDestinationCleanup.IsSameFile(now, created)`: `now.length == created.length && now.last_write_utc ==
/// created.last_write_utc`. Exact compare (no rounding allowance: the file was created on its own volume and observed
/// through the same file system; a tolerance would only widen what a replacement can pass as). Also used by
/// `FileActionService` (W3-04, group copy rollback at C# lines 344 and 364), hence `pub`.
pub fn is_same_file(now: &FileStat, created: &FileStat) -> bool {
    todo!("W3-08")
}
