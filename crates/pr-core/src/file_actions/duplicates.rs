//! Port of `PhotoReview.Core/FileActions/DuplicateFinder.cs` (C# baseline d92d6aae): which files "Remove numbered
//! duplicates" / "Remove original duplicates" would recycle (PARITY C-06). It only DECIDES; recycling is done by the
//! caller. Size groups first (one stat per file), then content hash inside each size group, then the naming rule.
//!
//! C# `FindAsync` is `async` with a `Func<string, CancellationToken, Task<string>>` hash; per
//! docs/adr/R0003-concurrency.md the Rust function is blocking and the hash is a plain closure the caller runs on the
//! right lane. The C# `fileSystem == null` branch (`File.Exists`/`FileInfo`) is not ported: pass the real
//! `PhysicalFileSystem`.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;

use crate::abstractions::FileSystem;
use crate::catalog::natural_cmp::is_dotnet_digit;
use crate::sync::{CancelToken, Canceled};
use crate::text::dotnet_path::get_file_name_without_extension;
use crate::text::ignore_case_key;

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
    cancel.check()?;
    if files.is_empty() {
        return Ok(Vec::new());
    }

    // Step 1: one stat per distinct path (first spelling wins), grouped by size in order of first appearance.
    let mut seen: HashSet<String> = HashSet::new();
    let mut size_index: HashMap<u64, usize> = HashMap::new();
    let mut size_groups: Vec<(u64, Vec<&str>)> = Vec::new();
    for path in files {
        if !seen.insert(ignore_case_key(path)) {
            continue;
        }
        cancel.check()?;
        let Some(stat) = file_system.get_file_stat(Path::new(path)) else {
            continue;
        };
        let idx = *size_index.entry(stat.length).or_insert_with(|| {
            size_groups.push((stat.length, Vec::new()));
            size_groups.len() - 1
        });
        size_groups[idx].1.push(path);
    }
    size_groups.retain(|(_, paths)| {
        paths.len() > 1
            && paths
                .iter()
                .any(|p| has_numbered_suffix(get_file_name_without_extension(p)) == remove_numbered)
    });

    // Step 2: hash each file of the kept groups; content groups keyed by size AND hash text (ignoring case).
    let mut content_index: HashMap<String, usize> = HashMap::new();
    let mut content_groups: Vec<Vec<&str>> = Vec::new();
    for (size, paths) in &size_groups {
        for &path in paths {
            cancel.check()?;
            match hash(path, cancel) {
                Ok(h) => {
                    let key = ignore_case_key(&format!("{size}|{h}"));
                    let idx = *content_index.entry(key).or_insert_with(|| {
                        content_groups.push(Vec::new());
                        content_groups.len() - 1
                    });
                    content_groups[idx].push(path);
                }
                Err(HashFailure::Canceled) => return Err(Canceled),
                Err(HashFailure::Unreadable(_)) => {}
            }
        }
    }

    // Step 3: naming rule; never remove a whole content group.
    let mut remove = Vec::new();
    for group in content_groups.iter().filter(|g| g.len() > 1) {
        let mut matching: Vec<&str> = group
            .iter()
            .copied()
            .filter(|p| has_numbered_suffix(get_file_name_without_extension(p)) == remove_numbered)
            .collect();
        if matching.len() == group.len() {
            matching.remove(0);
        }
        remove.extend(matching.into_iter().map(str::to_owned));
    }
    Ok(remove)
}

/// `NumberedPattern` = ` \(\d+\)$` (unanchored at the start) applied to a file stem: the stem ends with a space, `(`, one or
/// more digits, `)`. Same .NET regex rules as `compare::numbered_stem_base`: `\d` = [`is_dotnet_digit`](crate::catalog::natural_cmp::is_dotnet_digit),
/// `$` also matches before one final `'\n'`. Unlike `numbered_stem_base` a `'\n'` earlier in the stem does not matter:
/// `"a\nb (1)"` => true. `"img (1)"` => true, `"img(1)"` => false, `"img (1) "` => false, `" (1)"` => true, `"img (１)"` => true.
pub fn has_numbered_suffix(stem: &str) -> bool {
    let t = stem.strip_suffix('\n').unwrap_or(stem);
    let Some(t) = t.strip_suffix(')') else {
        return false;
    };
    let trimmed = t.trim_end_matches(is_dotnet_digit);
    if trimmed.len() == t.len() {
        return false;
    }
    trimmed.ends_with(" (")
}
