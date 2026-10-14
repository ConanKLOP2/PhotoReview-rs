//! Port of `PhotoReview.Core/FileActions/DuplicateFinder.cs` (C# baseline d92d6aae): which files "Remove numbered
//! duplicates" / "Remove original duplicates" would recycle (PARITY C-06). It only DECIDES; recycling is done by the
//! caller. Size groups first (one stat per file), then content hash inside each size group, then the naming rule.
//!
//! C# `FindAsync` is `async` with a `Func<string, CancellationToken, Task<string>>` hash; per
//! docs/adr/R0003-concurrency.md the Rust function is blocking and the hash is a plain closure the caller runs on the
//! right lane. The C# `fileSystem == null` branch (`File.Exists`/`FileInfo`) is not ported: pass the real
//! `PhysicalFileSystem`.
// Temporary (D11): bodies are `todo!()`; the porter removes this line with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::io;

use crate::abstractions::FileSystem;
use crate::sync::{CancelToken, Canceled};

/// Why the caller's hash function failed for one file.
#[derive(Debug)]
pub enum HashFailure {
    /// The operation was canceled (C# `OperationCanceledException`, even one thrown without the token being canceled,
    /// e.g. a linked source). [`find`] stops and returns `Err(Canceled)`.
    Canceled,
    /// The file cannot be hashed and is skipped: C# `IOException`, `UnauthorizedAccessException`, `ArgumentException`
    /// or `NotSupportedException` (a closure maps all four to this). The other files are still compared.
    Unreadable(io::Error),
}

/// `DuplicateFinder.FindAsync(files, removeNumbered, hash, fileSystem, cancellationToken)`.
///
/// Returns the files to remove, in this order: content groups in the order their first member was hashed, members of
/// a group in hash order (= input order within a size group). Algorithm:
/// 0. `cancel.check()?` first (even for empty input). `files.is_empty()` => `Ok(vec![])`.
/// 1. De-duplicate `files` OrdinalIgnoreCase keeping the FIRST spelling (the same file listed twice or recased is not its
///    own duplicate). For each path, in order, `cancel.check()?` and then
///    `file_system.get_file_stat(Path::new(path))`: `None` (missing, directory or any stat failure) => skipped; `Some` =>
///    size `stat.length`. Group by exact size in the order sizes first appear; keep only groups with 2 or more files AND
///    at least one member for which `has_numbered_suffix(stem) == remove_numbered` (a group where nobody matches the
///    rule can never yield a removal, so none of its files is hashed).
/// 2. For every file of every kept group (size groups in order, files in order): `cancel.check()?`, then
///    `hash(path, cancel)`: `Ok(h)` => file joins the content group keyed `"{size}|{h}"` (size in invariant decimal; the key
///    is compared OrdinalIgnoreCase, so hash text `ABC` and `abc` are the same content); `Err(HashFailure::Canceled)` =>
///    return `Err(Canceled)`; `Err(HashFailure::Unreadable(_))` => skip this file.
/// 3. For each content group with 2 or more members (in creation order): `matching` = the members whose
///    `has_numbered_suffix(get_file_name_without_extension(path)) == remove_numbered`. When EVERY member matches, drop
///    the first one of `matching` (the first in input order survives, so a group is never removed completely). Append
///    `matching` to the result.
pub fn find(
    files: &[String],
    remove_numbered: bool,
    hash: &mut dyn FnMut(&str, &CancelToken) -> Result<String, HashFailure>,
    file_system: &dyn FileSystem,
    cancel: &CancelToken,
) -> Result<Vec<String>, Canceled> {
    todo!()
}

/// `NumberedPattern` = ` \(\d+\)$` (unanchored at the start) applied to a file stem: the stem ends with a space, `(`, one or
/// more digits, `)`. Same .NET regex rules as `compare::numbered_stem_base`: `\d` = [`is_dotnet_digit`](crate::catalog::natural_cmp::is_dotnet_digit),
/// `$` also matches before one final `'\n'`. Unlike `numbered_stem_base` a `'\n'` earlier in the stem does not matter:
/// `"a\nb (1)"` => true. `"img (1)"` => true, `"img(1)"` => false, `"img (1) "` => false, `" (1)"` => true, `"img (１)"` => true.
pub fn has_numbered_suffix(stem: &str) -> bool {
    todo!()
}
