//! Port of the message tests in `tests/PhotoReview.App.Tests/Localization/LocalizationServiceTests.cs` (C# baseline
//! d92d6aae, `TranslationProblems`, I18N "Reload translations") plus the Rust-only contract tests of W8-04a.
//! Red tests: every call into `translation_problems` panics via `todo!()` until the porter implements the bodies.
//! The localizer is passed explicitly, so these tests need no global lock.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pr_app::localization::translation_problems::{MAX_LISTED, has_problems, message};
use pr_core::abstractions::FileSystem;
use pr_l10n::loader::{LanguageLoader, UiCulture};
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_l10n::template::LocArg;
use pr_test_support::FakeFileSystem;

const SHIPPED: &str = r"C:\App\Languages";
const USER: &str = r"C:\Users\u\AppData\Local\PhotoReviewRs\Languages";
const VI_CATALOG: &str = include_str!("../../../assets/languages/vi.json");

/// The Vietnamese localizer, loaded from the shipped `vi.json` through a fake folder.
fn vietnamese() -> Localizer {
    let fs = Arc::new(FakeFileSystem::new());
    fs.add_dir(SHIPPED);
    fs.add_file(Path::new(SHIPPED).join("vi.json"), VI_CATALOG.as_bytes());
    let fs_dyn: Arc<dyn FileSystem> = fs.clone();
    LanguageLoader::new(fs_dyn, SHIPPED, Some(PathBuf::from(USER))).load(Some("vi"), Some(&UiCulture::from_name("vi")))
}

/// `problem-01`, `problem-02`, ... (two digits, as the golden `repeat` input builds them).
fn numbered(count: usize) -> Vec<String> {
    (1..=count).map(|i| format!("problem-{i:02}")).collect()
}

#[test]
fn max_listed_is_fifteen() {
    // C#: TranslationProblems.MaxListed (the golden records `max_listed` = 15)
    assert_eq!(MAX_LISTED, 15);
}

#[test]
fn problems_message_no_problems_says_so() {
    // C#: LocalizationServiceTests.ProblemsMessage_NoProblems_SaysSo
    let text = vietnamese();

    assert_eq!(message(&[], &text), text.get("dialog.reloadTranslations.ok"));
    assert!(!has_problems(&[]));
}

#[test]
fn problems_message_lists_each_problem_and_counts_the_rest() {
    // C#: LocalizationServiceTests.ProblemsMessage_ListsEachProblem_AndCountsTheRest
    let text = vietnamese();
    let warnings = numbered(MAX_LISTED + 3);

    let result = message(&warnings, &text);

    assert!(has_problems(&warnings));
    assert!(
        result.contains(&(MAX_LISTED + 3).to_string()),
        "total count is in the message: {result}"
    );
    for warning in &warnings[..MAX_LISTED] {
        assert!(result.contains(&format!("• {warning}")), "listed: {warning}");
    }
    assert!(
        !result.contains(&warnings[MAX_LISTED]),
        "the 16th problem is counted, not listed"
    );
    assert!(result.ends_with(&text.format("dialog.reloadTranslations.more", &[LocArg::new("count", 3_i64)])));
}

#[test]
fn problems_message_few_problems_lists_all_without_more_line() {
    // C#: LocalizationServiceTests.ProblemsMessage_FewProblems_ListsAllWithoutMoreLine
    let text = vietnamese();

    let result = message(&["a.json: invalid JSON".to_owned()], &text);

    assert!(result.ends_with("• a.json: invalid JSON"), "{result}");
    // no "more" line: the Vietnamese and English "more" texts both start with an ellipsis
    assert!(!result.contains('…'), "{result}");
}

#[test]
fn message_counts_total_not_listed_in_plural_and_count() {
    // Rust-only (contract trap T5): the plural and `{count}` use the TOTAL (16), not the 15 listed.
    let english = built_in_english_localizer();
    let warnings = numbered(16);

    let result = message(&warnings, &english);

    let first_line = result.lines().next().expect("the message has a first line");
    assert!(
        first_line.contains("16 problems"),
        "total count in the head: {first_line}"
    );
    assert!(
        !first_line.contains("15 problems"),
        "listed count is not used: {first_line}"
    );
}

#[test]
fn more_line_uses_remaining_count() {
    // Rust-only (contract trap T5): with 18 warnings the "more" line says 3 (18 minus 15 listed).
    let english = built_in_english_localizer();
    let warnings = numbered(18);

    let result = message(&warnings, &english);

    assert!(
        result.ends_with("… and 3 more (all problems are in the log)."),
        "{result}"
    );
}

#[test]
fn placeholder_in_warning_is_not_expanded() {
    // Rust-only (contract trap T6): `{count}`, `{0}` and `{details}` inside a warning are plain text.
    let english = built_in_english_localizer();
    let warning = "file {count}.json: key 'x{0}' {details}".to_owned();

    let result = message(std::slice::from_ref(&warning), &english);

    assert!(result.ends_with(&format!("• {warning}")), "{result}");
}

#[test]
fn has_problems_false_for_empty() {
    // Rust-only (contract semantics 5): `has_problems` is `!warnings.is_empty()`.
    assert!(!has_problems(&[]));
    assert!(has_problems(&["x".to_owned()]));
}
