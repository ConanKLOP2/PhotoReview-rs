//! `StringComparison.OrdinalIgnoreCase`, the comparison the C# app uses for every path and file name (AGENTS.md:
//! "Ordinal/OrdinalIgnoreCase for file/path names").
//!
//! It is NOT Rust's `to_lowercase`/`to_uppercase` and not Unicode case folding: .NET maps each code point to one
//! "ordinal upper" code point (no `ß` → `SS`, no `ı` → `I`, no `ſ` → `S`) and then compares code points. The mapping
//! table is generated from .NET itself (`tools/oracle.ps1 -Command ordinal-casing-table`) and the behaviour is pinned
//! by `tests/golden/ordinal-ignore-case.json`. Use these helpers; never hand-roll case-insensitive path logic.

mod ordinal_casing_table;

use std::cmp::Ordering;
use std::path::Path;

use ordinal_casing_table::ORDINAL_UPPER;

/// The code point .NET's OrdinalIgnoreCase compares `c` as.
pub fn ordinal_upper(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_uppercase();
    }
    match ORDINAL_UPPER.binary_search_by_key(&(c as u32), |&(from, _)| from) {
        Ok(i) => char::from_u32(ORDINAL_UPPER[i].1).unwrap_or(c),
        Err(_) => c,
    }
}

/// `string.Equals(a, b, StringComparison.OrdinalIgnoreCase)`.
pub fn eq_ignore_case(a: &str, b: &str) -> bool {
    cmp_ignore_case(a, b) == Ordering::Equal
}

/// Sign of `string.Compare(a, b, StringComparison.OrdinalIgnoreCase)`.
pub fn cmp_ignore_case(a: &str, b: &str) -> Ordering {
    a.chars().map(ordinal_upper).cmp(b.chars().map(ordinal_upper))
}

/// A key two strings share exactly when they are OrdinalIgnoreCase-equal: use it for `HashMap`/`HashSet` keys
/// where C# used `StringComparer.OrdinalIgnoreCase`.
pub fn ignore_case_key(s: &str) -> String {
    s.chars().map(ordinal_upper).collect()
}

/// Path flavour of [`eq_ignore_case`]. Windows file names that are not valid UTF-16 (unpaired surrogates) are compared
/// lossily; the C# app cannot represent them faithfully either.
pub fn path_eq_ignore_case(a: &Path, b: &Path) -> bool {
    eq_ignore_case(&a.to_string_lossy(), &b.to_string_lossy())
}

/// Path flavour of [`ignore_case_key`].
pub fn path_key(p: &Path) -> String {
    ignore_case_key(&p.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_never_maps_to_itself() {
        assert!(ORDINAL_UPPER.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(ORDINAL_UPPER.iter().all(|&(from, to)| from != to && from >= 0x80));
    }

    #[test]
    fn ascii_is_compared_upper_cased() {
        // '_' (0x5F) sorts after 'A' (0x41) because 'a' is compared as 'A'.
        assert_eq!(cmp_ignore_case("_", "a"), Ordering::Greater);
        assert!(eq_ignore_case("IMG_001.JPG", "img_001.jpg"));
    }

    #[test]
    fn ignore_case_key_matches_equality() {
        assert_eq!(ignore_case_key("Ảnh.jpg"), ignore_case_key("ẢNH.JPG"));
        assert_ne!(ignore_case_key("ß"), ignore_case_key("SS"));
    }
}
