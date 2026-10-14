//! Port of `PhotoReview.Core/Catalog/ComparePairService.cs` (C# baseline d92d6aae): pairs `X.ext` with `X (n).ext` in the
//! same folder and extension for Compare mode (PARITY C-25).
//!
//! Two entry points that must agree: [`find`] (one query, O(n log n) per call) and [`build_index`] (all queries at once,
//! used on every navigation by the presenter). The agreement is not perfect in C# (the extension of a path with trailing
//! dots/spaces differs between the two, see contracts/W2-07.md quirks); port both exactly as written.
//!
//! The two C# `[GeneratedRegex]` patterns are hand-written matchers here (no `regex` crate, no new dependency):
//! [`numbered_stem_base`] (`^(.*) \(\d+\)$`) and [`numbered_suffix`] (`\((\d+)\)$` + `int.TryParse`).

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::catalog::natural_cmp::is_dotnet_digit;
use crate::catalog::types::get_extension;
use crate::text::dotnet_path::{PathError, get_directory_name, get_file_name_without_extension, get_full_path};
use crate::text::{cmp_ignore_case, eq_ignore_case, ignore_case_key};

/// Splits `s` the way `\((\d+)\)$` anchors: `$` matches at the end or just before ONE final `'\n'`. Returns
/// `(text before the '(', the digit run)` when the (newline-trimmed) text ends with `(` digits+ `)`.
fn split_numbered_tail(s: &str) -> Option<(&str, &str)> {
    let t = s.strip_suffix('\n').unwrap_or(s);
    let t = t.strip_suffix(')')?;
    let digits_start = t
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_dotnet_digit(c))
        .last()
        .map(|(i, _)| i)?;
    let before = t[..digits_start].strip_suffix('(')?;
    Some((before, &t[digits_start..]))
}

/// UTF-16 ordinal comparison (`string.CompareOrdinal`).
fn cmp_ordinal(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `NumberedStemPattern` = `^(.*) \(\d+\)$` applied to a file stem; returns group 1 (the stem without the ` (n)`), or
/// `None` when the stem does not match.
/// .NET regex rules to reproduce: `\d` is `char.IsDigit` (Unicode Nd of the BMP: use
/// [`crate::catalog::natural_cmp::is_dotnet_digit`], so `" (１)"` and `" (٣)"` match but a supplementary-plane digit does
/// not); `.` matches any char except `'\n'`; `$` matches at the very end OR just before a final `'\n'`; the match is
/// anchored at the start (`^`) so a `'\n'` anywhere inside group 1 means no match. `"a (1)"` => `Some("a")`,
/// `"a (1)\n"` => `Some("a")`, `"a (1) (2)"` => `Some("a (1)")`, `" (1)"` => `Some("")`, `"a  (1)"` => `Some("a ")`,
/// `"(1)"`, `"a(1)"`, `"a ( 1)"`, `"a (1) "`, `"a (1)\r\n"`, `"a\n (1)"` => `None`.
pub fn numbered_stem_base(stem: &str) -> Option<&str> {
    let (before, _) = split_numbered_tail(stem)?;
    let base = before.strip_suffix(' ')?;
    if base.contains('\n') { None } else { Some(base) }
}

/// `NumberedSuffixPattern` + `int.TryParse`: the sort key `ComparePairService.NumberedSuffix` gives a candidate. Matches
/// `\((\d+)\)$` (NOT anchored at the start, no space needed: `"a(1)"` matches; same `\d`/`$` rules as
/// [`numbered_stem_base`]) against the file stem and parses group 1 with `int.TryParse` (invariant ASCII digits only,
/// culture `NumberStyles.Integer`): a value `0..=i32::MAX` (leading zeros allowed: `"(01)"` => 1); when there is no
/// match, or the digits are not all ASCII (`"(１)"`, `"(٣)"`), or the number overflows `i32` (`"(99999999999)"`),
/// the result is `i32::MAX` (C# `int.MaxValue`). Examples: `"a (1)"` => 1, `"a (0)"` => 0, `"a (10)"` => 10,
/// `"a (１)"` => `i32::MAX`, `"a"` => `i32::MAX`.
pub fn numbered_suffix(stem: &str) -> i32 {
    let Some((_, digits)) = split_numbered_tail(stem) else {
        return i32::MAX;
    };
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return i32::MAX;
    }
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() {
        return 0;
    }
    if trimmed.len() > 10 {
        return i32::MAX;
    }
    match trimmed.parse::<i64>() {
        Ok(n) => i32::try_from(n).unwrap_or(i32::MAX),
        Err(_) => i32::MAX,
    }
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
    let selected_folder = get_directory_name(&get_full_path(path)?);
    let selected_extension = get_extension(path);
    let mut keyed: Vec<(String, &String)> = Vec::new();
    for candidate in files {
        let full = get_full_path(candidate)?;
        let same_folder = match (&get_directory_name(&full), &selected_folder) {
            (None, None) => true,
            (Some(a), Some(b)) => eq_ignore_case(a, b),
            _ => false,
        };
        if same_folder && eq_ignore_case(get_extension(candidate), selected_extension) {
            keyed.push((full, candidate));
        }
    }
    keyed.sort_by(|a, b| cmp_ignore_case(&a.0, &b.0).then_with(|| cmp_ordinal(&a.0, &b.0)));
    let available: Vec<(&str, &str)> = keyed.iter().map(|(full, c)| (full.as_str(), c.as_str())).collect();

    let stem = get_file_name_without_extension(path);
    let matched = numbered_stem_base(stem);
    let base_stem = matched.unwrap_or(stem);
    let original = available
        .iter()
        .find(|(_, c)| eq_ignore_case(get_file_name_without_extension(c), base_stem))
        .map(|(_, c)| *c);
    let full_path = get_full_path(path)?;
    let selected = if matched.is_some() {
        available
            .iter()
            .find(|(full, _)| eq_ignore_case(full, &full_path))
            .map(|(_, c)| *c)
    } else {
        None
    };
    let numbered = selected.or_else(|| {
        available
            .iter()
            .filter(|(_, c)| is_numbered_variant_of(c, base_stem))
            .min_by_key(|(_, c)| numbered_suffix(get_file_name_without_extension(c)))
            .map(|(_, c)| *c)
    });
    Ok(match (original, numbered) {
        (Some(o), Some(n)) => Some((o.to_owned(), n.to_owned())),
        _ => None,
    })
}

/// `IsNumberedVariantOf(candidate, baseStem)`: the candidate's stem is longer than `base_stem` (UTF-16 length: use
/// `encode_utf16().count()`), matches [`numbered_stem_base`], and its group 1 equals `base_stem` (OrdinalIgnoreCase).
fn is_numbered_variant_of(candidate: &str, base_stem: &str) -> bool {
    let stem = get_file_name_without_extension(candidate);
    if stem.encode_utf16().count() <= base_stem.encode_utf16().count() {
        return false;
    }
    numbered_stem_base(stem).is_some_and(|b| eq_ignore_case(b, base_stem))
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
        self.map.get(&ignore_case_key(path)).map(|(_, pair)| pair)
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
    struct Item<'a> {
        path: &'a str,
        full: String,
        input: usize,
    }
    let mut ordered: Vec<Item> = Vec::with_capacity(files.len());
    for (input, path) in files.iter().enumerate() {
        ordered.push(Item {
            path,
            full: get_full_path(path)?,
            input,
        });
    }
    ordered.sort_by(|a, b| {
        cmp_ignore_case(&a.full, &b.full)
            .then_with(|| cmp_ordinal(&a.full, &b.full))
            .then_with(|| a.input.cmp(&b.input))
    });

    type GroupKey = (Option<String>, String);
    let mut group_index: HashMap<GroupKey, usize> = HashMap::new();
    let mut groups: Vec<Vec<&Item>> = Vec::new();
    for item in &ordered {
        let key: GroupKey = (
            get_directory_name(&item.full).map(|d| ignore_case_key(&d)),
            ignore_case_key(get_extension(&item.full)),
        );
        let idx = *group_index.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[idx].push(item);
    }

    let mut result = CompareIndex::default();
    let mut put = |path: &str, pair: (String, String)| {
        let key = ignore_case_key(path);
        match result.map.get_mut(&key) {
            Some(slot) => slot.1 = pair,
            None => {
                result.map.insert(key, (path.to_owned(), pair));
            }
        }
    };
    for members in &groups {
        let mut originals: HashMap<String, &str> = HashMap::new();
        let mut variants: HashMap<String, Vec<&str>> = HashMap::new();
        for m in members {
            let stem = get_file_name_without_extension(m.path);
            originals.entry(ignore_case_key(stem)).or_insert(m.path);
            if let Some(base) = numbered_stem_base(stem) {
                variants.entry(ignore_case_key(base)).or_default().push(m.path);
            }
        }
        for list in variants.values_mut() {
            list.sort_by_key(|p| numbered_suffix(get_file_name_without_extension(p)));
        }
        for m in members {
            let stem = get_file_name_without_extension(m.path);
            let matched = numbered_stem_base(stem);
            let base = matched.unwrap_or(stem);
            let key = ignore_case_key(base);
            let Some(&original) = originals.get(&key) else {
                continue;
            };
            if matched.is_some() {
                put(m.path, (original.to_owned(), m.path.to_owned()));
            } else if let Some(list) = variants.get(&key) {
                put(m.path, (original.to_owned(), list[0].to_owned()));
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::is_numbered_variant_of;
    use pr_test_support::golden;

    /// Golden `numbered-pattern`: the 16 `IsNumberedVariantOf` pairs (the function is private in C# and here).
    #[test]
    fn is_numbered_variant_of_matches_golden() {
        let g = golden::load("numbered-pattern");
        let mut n = 0;
        for case in &g.cases {
            let Some(candidate) = case.get("candidate").and_then(|v| v.as_str()) else {
                continue;
            };
            n += 1;
            let base = golden::str_of(case, "base_stem");
            assert_eq!(
                is_numbered_variant_of(candidate, base),
                golden::bool_of(case, "is_numbered_variant"),
                "{candidate} / {base}"
            );
        }
        assert_eq!(n, 16);
    }
}
