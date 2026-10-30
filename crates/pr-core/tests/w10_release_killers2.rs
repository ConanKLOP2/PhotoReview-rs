//! Port of PhotoReview.Core/Updates/UpdateChecker.cs (7eaee2a2): JsonDocument.Parse in Interpret.
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
fn interpret_unused_string_accepts_each_json_escape() {
    // C#: UpdateChecker.Interpret parses the entire document, including unread values.
    // Mutants updates/checker.rs:695-703: delete each escape match arm in Scanner::string.
    for escaped in [r#"\""#, r"\\", r"\/", r"\b", r"\f", r"\n", r"\r", r"\t", r"\u0061"] {
        let body = format!(r#"{{"tag_name":"v2.0.0","unused":"{escaped}"}}"#);
        let result = UpdateChecker::interpret(&body, current());
        assert_eq!(result.status(), UpdateCheckStatus::UpdateAvailable, "{escaped}");
        assert_eq!(result.latest_version(), Some("2.0.0"), "{escaped}");
    }
}

#[test]
fn interpret_unused_string_rejects_bad_escapes_and_unescaped_controls() {
    // C#: UpdateChecker.Interpret catches JsonException from unread values too.
    // Mutant updates/checker.rs:689: delete match arm 0..=0x1F in Scanner::string.
    for invalid in ["\0", "\u{1f}", "\n", "\r", "\t", r"\x", r"\u12", "\\"] {
        let body = format!(r#"{{"tag_name":"v2.0.0","unused":"{invalid}"}}"#);
        assert_eq!(
            UpdateChecker::interpret(&body, current()).failure(),
            UpdateFailure::BadResponse,
            "{invalid:?}"
        );
    }
}

#[test]
fn interpret_unused_number_requires_digits_after_decimal_point_and_exponent() {
    // C#: UpdateChecker.Interpret catches JsonException for incomplete JSON numbers.
    // Mutants updates/checker.rs:731:18: replace - with + / / in Scanner::digits.
    for number in ["1.", "1.e2", "1e", "1e+", "1e-"] {
        let body = format!(r#"{{"tag_name":"v2.0.0","unused":{number}}}"#);
        assert_eq!(
            UpdateChecker::interpret(&body, current()).failure(),
            UpdateFailure::BadResponse,
            "{number}"
        );
    }
}
