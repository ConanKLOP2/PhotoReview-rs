//! Port of `PhotoReview.App.Shared/LaunchArguments.cs` (C# baseline d92d6aae): command-line folder arguments as
//! Explorer's "Browse with PhotoReview" (and drag onto the exe) hand them over.
//!
//! A folder argument is made canonical (absolute, no trailing separator except for a drive root) so the instance lock,
//! the Explorer prefetch and the folder load see one spelling. A quoted drive root such as `"D:\"` reaches the process as
//! `D:"` (the backslash escapes the closing quote) and is repaired. Anything that is not an existing folder is returned
//! unchanged.

use std::path::Path;

use pr_core::abstractions::FileSystem;
use pr_core::file_actions::errors::get_path_root;
use pr_core::text::dotnet_path::{PathError, get_full_path};

/// C# `LaunchArguments.Normalize(IEnumerable<string>)`: maps every argument, keeps the order and the count.
///
/// `Err` only when `Path.GetFullPath` fails on an existing folder (C# lets the exception escape; the caller decides, see
/// the contract's open questions).
pub fn normalize<S: AsRef<str>>(fs: &dyn FileSystem, args: &[S]) -> Result<Vec<String>, PathError> {
    args.iter().map(|arg| normalize_one(fs, arg.as_ref())).collect()
}

/// C# `LaunchArguments.NormalizeOne(string arg)` (internal; the tests call it directly). Steps, in order:
///
/// 1. `arg` empty or whitespace only (C# `IsNullOrWhiteSpace`): return `arg` unchanged.
/// 2. `candidate = arg`. When `arg` has more than one UTF-16 unit and ends with `'"'`, `repaired = arg[..last] + "\"`;
///    when `fs.directory_exists(repaired)`, `candidate = repaired`.
/// 3. `!fs.directory_exists(candidate)`: return the ORIGINAL `arg`, not `candidate`.
/// 4. `full = get_full_path(candidate)?`.
/// 5. `root_len` = UTF-16 length of `get_path_root(full)`, or 0 when that is `None`.
/// 6. `full.len_utf16() > root_len`: strip ALL trailing `'\\'` and `'/'`; otherwise (a drive root) return `full` as is.
pub fn normalize_one(fs: &dyn FileSystem, arg: &str) -> Result<String, PathError> {
    if arg.trim().is_empty() {
        return Ok(arg.to_owned());
    }

    let mut candidate = arg.to_owned();
    if utf16_len(arg) > 1 && arg.ends_with('"') {
        let repaired = format!("{}\\", &arg[..arg.len() - 1]);
        if fs.directory_exists(Path::new(&repaired)) {
            candidate = repaired;
        }
    }

    if !fs.directory_exists(Path::new(&candidate)) {
        return Ok(arg.to_owned());
    }

    let full = get_full_path(&candidate)?;
    let root_len = get_path_root(&full).map_or(0, |root| utf16_len(&root));
    if utf16_len(&full) > root_len {
        Ok(full.trim_end_matches(['\\', '/']).to_owned())
    } else {
        Ok(full)
    }
}

/// Length in UTF-16 code units (C# `string.Length`).
fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}
