//! Port of `PhotoReview.Core/Catalog/ManagedNaturalComparer.cs` (C# baseline e441d67a).
//!
//! Natural ("a2" < "a10") and case-insensitive file-name order, independent of the user's culture.
//! The comparison is done on a *natural key*:
//!
//! * every run of consecutive digits becomes `zero-padded run length (10 wide)` + `the run without its leading
//!   ASCII '0's` + `'\0'`;
//! * every other char is lower-cased with .NET `char.ToLowerInvariant`.
//!
//! Two names compare as: key (OrdinalIgnoreCase), then the original names (OrdinalIgnoreCase), then the original
//! names (Ordinal). The result is a strict total order: it is `Equal` only for identical strings.
//!
//! "Digit" means .NET `char.IsDigit` on one UTF-16 code unit, i.e. Unicode category Nd *in the BMP only*
//! ([`DECIMAL_DIGIT_RANGES`]). Digits are NOT converted to numeric values: Arabic-Indic or fullwidth digits form a run
//! and are copied into the key as they are (they only count towards the run length). A digit outside the BMP (a
//! surrogate pair in C#) is not a digit at all.
//!
//! Pinned by `tests/golden/natural-cmp.json` and `tests/golden/natural-key.json` (see contracts/W1-06/golden.md).
use std::cmp::Ordering;

use crate::abstractions::NaturalComparer;
use crate::text::cmp_ignore_case;

/// Width of the zero-padded digit-count prefix in a key (the former `ToString("D10")`).
pub const LENGTH_PREFIX_WIDTH: usize = 10;

/// Inclusive `(first, last)` code-point ranges of the BMP characters for which .NET `char.IsDigit` is true
/// (Unicode Nd), generated from .NET 10.0.12 (the C# app's runtime). Sorted. Rust's `char::is_numeric` /
/// `is_ascii_digit` are NOT equivalent; use [`is_dotnet_digit`].
pub const DECIMAL_DIGIT_RANGES: &[(u32, u32)] = &[
    (0x0030, 0x0039),
    (0x0660, 0x0669),
    (0x06F0, 0x06F9),
    (0x07C0, 0x07C9),
    (0x0966, 0x096F),
    (0x09E6, 0x09EF),
    (0x0A66, 0x0A6F),
    (0x0AE6, 0x0AEF),
    (0x0B66, 0x0B6F),
    (0x0BE6, 0x0BEF),
    (0x0C66, 0x0C6F),
    (0x0CE6, 0x0CEF),
    (0x0D66, 0x0D6F),
    (0x0DE6, 0x0DEF),
    (0x0E50, 0x0E59),
    (0x0ED0, 0x0ED9),
    (0x0F20, 0x0F29),
    (0x1040, 0x1049),
    (0x1090, 0x1099),
    (0x17E0, 0x17E9),
    (0x1810, 0x1819),
    (0x1946, 0x194F),
    (0x19D0, 0x19D9),
    (0x1A80, 0x1A89),
    (0x1A90, 0x1A99),
    (0x1B50, 0x1B59),
    (0x1BB0, 0x1BB9),
    (0x1C40, 0x1C49),
    (0x1C50, 0x1C59),
    (0xA620, 0xA629),
    (0xA8D0, 0xA8D9),
    (0xA900, 0xA909),
    (0xA9D0, 0xA9D9),
    (0xA9F0, 0xA9F9),
    (0xAA50, 0xAA59),
    (0xABF0, 0xABF9),
    (0xFF10, 0xFF19),
];

/// BMP letters that Rust's lower-casing maps (Unicode 16/17 additions) but .NET 10 `char.ToLowerInvariant` leaves
/// unchanged. [`lower_invariant`] must return these as they are.
pub const NOT_LOWERED_BY_DOTNET: &[char] = &[
    '\u{1C89}', '\u{A7CB}', '\u{A7CC}', '\u{A7CE}', '\u{A7D2}', '\u{A7D4}', '\u{A7DA}', '\u{A7DC}',
];

/// .NET `char.IsDigit(c)` for the UTF-16 code unit `c`: true only for BMP characters inside
/// [`DECIMAL_DIGIT_RANGES`]. Any `c` above U+FFFF is `false` (C# sees two surrogate halves, neither is a digit).
pub fn is_dotnet_digit(c: char) -> bool {
    let v = c as u32;
    if v > 0xFFFF {
        return false;
    }
    DECIMAL_DIGIT_RANGES
        .binary_search_by(|&(first, last)| {
            if last < v {
                Ordering::Less
            } else if first > v {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        })
        .is_ok()
}

/// .NET `char.ToLowerInvariant(c)` applied to one UTF-16 code unit.
///
/// * ASCII: `to_ascii_lowercase`.
/// * Above U+FFFF: unchanged (C# lowers each surrogate half separately and surrogates have no case).
/// * Members of [`NOT_LOWERED_BY_DOTNET`]: unchanged.
/// * Otherwise: the simple lower-case mapping, i.e. `c.to_lowercase()` when it yields exactly ONE char, else `c`
///   (this keeps `'İ'` U+0130 unchanged, as .NET does).
pub fn lower_invariant(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    if c as u32 > 0xFFFF || NOT_LOWERED_BY_DOTNET.contains(&c) {
        return c;
    }
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(one), None) => one,
        _ => c,
    }
}

/// `ManagedNaturalComparer.BuildNaturalKey`. An empty name gives an empty key.
///
/// For each maximal run of [`is_dotnet_digit`] chars: strip leading ASCII `'0'` only (all-zero run => empty digits),
/// append the number of remaining chars as exactly [`LENGTH_PREFIX_WIDTH`] decimal ASCII digits (zero-padded on the
/// left), then the remaining chars unchanged, then `'\0'`. Every other char is appended as [`lower_invariant`] of it.
///
/// Examples: `"a2"` -> `"a00000000012\0"`; `"007.jpg"` -> `"00000000017\0.jpg"`; `"0"` -> `"0000000000\0"`.
pub fn build_natural_key(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut key = String::with_capacity(name.len() + LENGTH_PREFIX_WIDTH + 1);
    let mut i = 0;
    while i < chars.len() {
        if is_dotnet_digit(chars[i]) {
            let start = i;
            while i < chars.len() && is_dotnet_digit(chars[i]) {
                i += 1;
            }
            let run = &chars[start..i];
            // Strip leading ASCII '0' only; an all-zero run leaves no digits.
            let significant = run.iter().position(|&c| c != '0').unwrap_or(run.len());
            let digits = &run[significant..];
            key.push_str(&format!("{:0width$}", digits.len(), width = LENGTH_PREFIX_WIDTH));
            key.extend(digits.iter());
            key.push('\0');
        } else {
            key.push(lower_invariant(chars[i]));
            i += 1;
        }
    }
    key
}

/// `ManagedNaturalComparer.Compare` for two non-null names. See the module doc for the three comparison steps.
/// String comparison uses `crate::text::cmp_ignore_case` (OrdinalIgnoreCase) and `str::cmp` (Ordinal: UTF-8 byte
/// order equals code-point order, which equals the C# UTF-16 order whenever the two strings differ only in case).
pub fn compare_natural(x: &str, y: &str) -> Ordering {
    let by_key = cmp_ignore_case(&build_natural_key(x), &build_natural_key(y));
    if by_key != Ordering::Equal {
        return by_key;
    }
    let by_text = cmp_ignore_case(x, y);
    if by_text != Ordering::Equal {
        return by_text;
    }
    x.cmp(y)
}

/// `ManagedNaturalComparer` (C# singleton `ManagedNaturalComparer.Instance`). Stateless.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManagedNaturalComparer;

impl ManagedNaturalComparer {
    /// C# `ManagedNaturalComparer.Instance`.
    pub const INSTANCE: ManagedNaturalComparer = ManagedNaturalComparer;
}

impl NaturalComparer for ManagedNaturalComparer {
    /// `None` is a null string: `(None, None)` => Equal; `(None, Some)` => Less; `(Some, None)` => Greater;
    /// otherwise [`compare_natural`].
    fn compare(&self, a: Option<&str>, b: Option<&str>) -> Ordering {
        match (a, b) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(x), Some(y)) => compare_natural(x, y),
        }
    }
}
