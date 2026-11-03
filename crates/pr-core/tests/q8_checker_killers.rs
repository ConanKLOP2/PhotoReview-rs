//! Q8 (P-09) mutation killers for `find_property` in `pr_core::updates::checker` (2026-11-04 run on checker.rs:
//! `618:22 > -> >=` and `618:59 > -> >=` survived the four checker test binaries). C#: `JsonElement.TryGetProperty`
//! of .NET 10 throws for a property name holding a lone surrogate only when the lookup gets as far as unescaping it
//! (differential run recorded at `lone_surrogate_property_name_only_fails_when_compared`).
use pr_core::updates::checker::UpdateChecker;
use pr_core::updates::version::{AppVersion, UpdateCheckStatus, UpdateFailure};

fn current() -> AppVersion {
    AppVersion {
        major: 1,
        minor: 0,
        patch: 0,
        is_prerelease: false,
    }
}

#[test]
fn bad_name_as_long_as_the_key_is_skipped_not_unescaped() {
    // `ta\ud800` is 8 bytes like `tag_name`; the name is not LONGER than the key, so .NET never unescapes it.
    // Kills `raw.len() > key.len()` -> `>=` (checker.rs:618:22). The bad name is last, so it is examined first.
    let body = r#"{"tag_name":"v2.0.0","ta\ud800":1}"#;
    let result = UpdateChecker::interpret(body, current());
    assert_eq!(result.status(), UpdateCheckStatus::UpdateAvailable);
    assert_eq!(result.latest_version(), Some("2.0.0"));
}

#[test]
fn bad_name_whose_escape_starts_exactly_at_the_key_end_is_skipped() {
    // `tag_name\ud800` has its backslash at offset 8 == key length: the key is not longer than the offset, so the
    // name is skipped. Kills `key.len() > backslash` -> `>=` (checker.rs:618:59).
    let body = r#"{"tag_name":"v2.0.0","tag_name\ud800":1}"#;
    let result = UpdateChecker::interpret(body, current());
    assert_eq!(result.status(), UpdateCheckStatus::UpdateAvailable);
    assert_eq!(result.latest_version(), Some("2.0.0"));
}

#[test]
fn bad_name_longer_than_the_key_with_a_matching_prefix_still_throws() {
    // Counter-check of the two tests above: one more key byte before the escape and the name IS unescaped
    // (`tag_nam\ud800` has its backslash at 7 < key length 8 and is longer than the key), which .NET reports as
    // an exception -> BadResponse. Keeps the boundary tests from passing because the lookup never throws.
    let body = r#"{"tag_name":"v2.0.0","tag_nam\ud800":1}"#;
    assert_eq!(
        UpdateChecker::interpret(body, current()).failure(),
        UpdateFailure::BadResponse
    );
}
