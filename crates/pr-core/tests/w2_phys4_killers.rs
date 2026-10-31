//! Focused physical-filesystem pattern/path mutation guards (CL-PHYS4). Pure functions, no disk, no sleeps, no clock.
//! Port of PhotoReview.Core/IO/PhysicalFileSystem.cs (`FileSystemName.MatchesWin32Expression` semantics).

use pr_core::abstractions::FileSystem;
use pr_core::io::physical::matches_win32_expression;
use pr_test_support::TempRoot;

const B: char = '\u{5c}';

// C#: FileSystemName.MatchPattern returns false for an empty name, even for the expression "*".
// physical.rs:236 `||` -> `&&` in match_pattern (the lone-`*` fast path would answer true).
#[test]
fn matches_win32_expression_empty_name_never_matches() {
    assert!(!matches_win32_expression("*", ""));
    assert!(!matches_win32_expression("*.*", ""));
    assert!(!matches_win32_expression("a*", ""));
    assert!(matches_win32_expression("*", "x"));
}

// C#: TranslateWin32Expression keeps a trailing '.' that does not follow a '*'.
// physical.rs:219 `<` -> `<=` and `-` -> `+` / `/` in translate_win32_expression: the lookahead would index past the end.
#[test]
fn matches_win32_expression_trailing_dot_after_a_literal_is_kept_literal() {
    assert!(matches_win32_expression("a.", "a."));
    assert!(!matches_win32_expression("a.", "a"));
    assert!(!matches_win32_expression("a.", "ab"));
    assert!(matches_win32_expression("ab.", "ab."));
}

// C#: the `\` escape of MatchPattern makes the next expression character literal (so `\*` is a star, not a wildcard).
// physical.rs:321 `+=` -> `-=` / `*=`, :322 `==` -> `!=`, :326 `offset * 2 + 2` arithmetic in match_pattern.
#[test]
fn matches_win32_expression_backslash_escapes_the_next_character() {
    assert!(matches_win32_expression(&format!("{B}a"), "a"));
    assert!(!matches_win32_expression(&format!("{B}a"), "b"));
    assert!(!matches_win32_expression(&format!("{B}a"), &format!("{B}a")));
    assert!(matches_win32_expression(&format!("{B}ab"), "ab"));
    assert!(!matches_win32_expression(&format!("{B}ab"), "ax"));
    assert!(!matches_win32_expression(&format!("{B}ab"), "a"));
    assert!(matches_win32_expression(&format!("x{B}*y"), "x*y"));
    assert!(!matches_win32_expression(&format!("x{B}*y"), "xzy"));
    assert!(matches_win32_expression(&format!("a{B}bc"), "abc"));
    assert!(!matches_win32_expression(&format!("a{B}bc"), "ab"));
    assert!(!matches_win32_expression(&format!("a{B}bc"), "abcd"));
}

// C#: Directory.CreateDirectory creates each missing ancestor once, root first, and nothing else.
// physical.rs:521 `is_sep` -> true / false, `||` -> `&&`, `==` -> `!=` (the walk back to the previous separator).
#[test]
fn create_directory_creates_exactly_the_missing_ancestors() {
    let root = TempRoot::new("phys4-mkdir");
    let target = root.path().join("abc").join("def").join("ghi");
    pr_core::io::physical::PhysicalFileSystem
        .create_directory(&target)
        .unwrap();
    let names = |dir: &std::path::Path| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    };
    assert_eq!(names(root.path()), vec!["abc".to_string()]);
    assert_eq!(names(&root.path().join("abc")), vec!["def".to_string()]);
    assert_eq!(names(&root.path().join("abc").join("def")), vec!["ghi".to_string()]);
    assert!(target.is_dir());
}
