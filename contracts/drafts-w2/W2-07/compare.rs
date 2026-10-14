//! Port of `PhotoReview.Core/Catalog/ComparePairService.cs` (C# baseline d92d6aae): pairs `X.ext` with `X (n).ext` in the
//! same folder and extension for Compare mode (PARITY C-25).
//!
//! Two entry points that must agree: [`find`] (one query, O(n log n) per call) and [`build_index`] (all queries at once,
//! used on every navigation by the presenter). The agreement is not perfect in C# (the extension of a path with trailing
//! dots/spaces differs between the two, see contracts/W2-07.md quirks); port both exactly as written.
//!
//! The two C# `[GeneratedRegex]` patterns are hand-written matchers here (no `regex` crate, no new dependency):
//! [`numbered_stem_base`] (`^(.*) \(\d+\)$`) and [`numbered_suffix`] (`\((\d+)\)$` + `int.TryParse`).
// Temporary (D11): bodies are `todo!()`; the porter removes this line with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::collections::HashMap;

use crate::text::dotnet_path::PathError;

/// `NumberedStemPattern` = `^(.*) \(\d+\)$` applied to a file stem; returns group 1 (the stem without the ` (n)`), or
/// `None` when the stem does not match.
/// .NET regex rules to reproduce: `\d` is `char.IsDigit` (Unicode Nd of the BMP: use
/// [`crate::catalog::natural_cmp::is_dotnet_digit`], so `" (１)"` and `" (٣)"` match but a supplementary-plane digit does
/// not); `.` matches any char except `'\n'`; `$` matches at the very end OR just before a final `'\n'`; the match is
/// anchored at the start (`^`) so a `'\n'` anywhere inside group 1 means no match. `"a (1)"` => `Some("a")`,
/// `"a (1)\n"` => `Some("a")`, `"a (1) (2)"` => `Some("a (1)")`, `" (1)"` => `Some("")`, `"a  (1)"` => `Some("a ")`,
/// `"(1)"`, `"a(1)"`, `"a ( 1)"`, `"a (1) "`, `"a (1)\r\n"`, `"a\n (1)"` => `None`.
pub fn numbered_stem_base(stem: &str) -> Option<&str> {
    todo!()
}

/// `NumberedSuffixPattern` + `int.TryParse`: the sort key `ComparePairService.NumberedSuffix` gives a candidate. Matches
/// `\((\d+)\)$` (NOT anchored at the start, no space needed: `"a(1)"` matches; same `\d`/`$` rules as
/// [`numbered_stem_base`]) against the file stem and parses group 1 with `int.TryParse` (invariant ASCII digits only,
/// culture `NumberStyles.Integer`): a value `0..=i32::MAX` (leading zeros allowed: `"(01)"` => 1); when there is no
/// match, or the digits are not all ASCII (`"(１)"`, `"(٣)"`), or the number overflows `i32` (`"(99999999999)"`),
/// the result is `i32::MAX` (C# `int.MaxValue`). Examples: `"a (1)"` => 1, `"a (0)"` => 0, `"a (10)"` => 10,
/// `"a (１)"` => `i32::MAX`, `"a"` => `i32::MAX`.
pub fn numbered_suffix(stem: &str) -> i32 {
    todo!()
}

/// `ComparePairService.Find(files, path)`: `Some((original, numbered))` or `None`.
/// 1. `selected_folder = get_directory_name(&get_full_path(path)?)` (may be `None` for a drive root),
///    `selected_extension = get_extension(path)` (of the RAW argument).
/// 2. `available` = the entries of `files` whose `get_directory_name(&get_full_path(candidate)?)` equals
///    `selected_folder` (`None == None` is true; otherwise OrdinalIgnoreCase) AND whose `get_extension(candidate)`
///    equals `selected_extension` (OrdinalIgnoreCase), sorted by `get_full_path(candidate)` OrdinalIgnoreCase, then
///    Ordinal (stable: equal full paths keep input order). The candidates themselves (not their full paths) are kept.
/// 3. `stem = get_file_name_without_extension(path)`; `base_stem = numbered_stem_base(stem)` or `stem` itself.
///    `original` = the first of `available` whose own stem equals `base_stem` (OrdinalIgnoreCase).
/// 4. `numbered` = (only when `path` itself is numbered, i.e. `numbered_stem_base(stem)` matched) the first `available`
///    entry whose full path equals `get_full_path(path)` (OrdinalIgnoreCase); otherwise, or when none, the first of the
///    `available` entries `c` for which `is_numbered_variant_of(c, base_stem)` holds, ordered by `numbered_suffix` of its
///    stem (stable sort, ties keep the `available` order). `None` if there is none.
/// 5. `Some((original, numbered))` only when BOTH exist; they can be the same string only if the stems say so (a path
///    is never its own pair: `original` has stem `base_stem`, a numbered variant is longer).
///
/// Errors: [`PathError`] from `get_full_path` of `path` or of any candidate (C# lets the exception escape).
pub fn find(files: &[String], path: &str) -> Result<Option<(String, String)>, PathError> {
    todo!()
}

/// `IsNumberedVariantOf(candidate, baseStem)`: the candidate's stem is longer than `base_stem` (UTF-16 length: use
/// `encode_utf16().count()`), matches [`numbered_stem_base`], and its group 1 equals `base_stem` (OrdinalIgnoreCase).
fn is_numbered_variant_of(candidate: &str, base_stem: &str) -> bool {
    todo!()
}

/// The result of [`build_index`]: C# `Dictionary<string, (string Left, string Right)>(StringComparer.OrdinalIgnoreCase)`.
/// Keys are compared OrdinalIgnoreCase (store `ignore_case_key(path)` in the map); a later insert for an equal key
/// replaces the value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompareIndex {
    /// `ignore_case_key(path)` -> (key as first inserted, pair).
    map: HashMap<String, (String, (String, String))>,
}

impl CompareIndex {
    /// `TryGetValue(path)`: OrdinalIgnoreCase lookup.
    pub fn get(&self, path: &str) -> Option<&(String, String)> {
        todo!()
    }

    /// `Count`.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// `Count == 0`.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// All (key, pair) entries in unspecified order (C# dictionary enumeration order is not a contract here).
    pub fn iter(&self) -> impl Iterator<Item = (&str, &(String, String))> {
        self.map.values().map(|(key, pair)| (key.as_str(), pair))
    }
}

/// `ComparePairService.BuildIndex(files)`: one pass equivalent to calling [`find`] for every path of `files`.
/// 1. `ordered` = every input path with its `get_full_path` and its input position, sorted by full path
///    OrdinalIgnoreCase, then Ordinal, then input position (a total order; `sort_by`).
/// 2. Group `ordered` (keeping its order inside each group) by (`get_directory_name(full)` or null,
///    `get_extension(full)`) — NOTE the extension of the FULL path here — both OrdinalIgnoreCase, `None == None`.
/// 3. Per group: `originals` (OrdinalIgnoreCase map, first wins) stem -> path for every member
///    (`get_file_name_without_extension(path)` of the ORIGINAL spelling, not the full path); `variants`
///    (OrdinalIgnoreCase) base stem -> paths of the members whose stem matches `numbered_stem_base`, each list stable-sorted
///    by `numbered_suffix(stem of that path)` (ties keep the list order).
/// 4. For each member `m` in group order: `base = numbered_stem_base(stem(m)).unwrap_or(stem(m))`; no `originals[base]` =>
///    skip; if `m` is numbered: `result[m.path] = (originals[base], m.path)`; else if `variants[base]` exists:
///    `result[m.path] = (originals[base], variants[base][0])`; else nothing.
///
/// Keys are the input spellings (a path listed twice, or in two cases, is one key; the last assignment wins).
///
/// Errors: [`PathError`] from `get_full_path` of any input path.
pub fn build_index(files: &[String]) -> Result<CompareIndex, PathError> {
    todo!()
}
