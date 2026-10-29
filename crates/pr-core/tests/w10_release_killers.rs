//! Offline P-09 release-document checks; C# UpdateChecker.Interpret uses JsonDocument.Parse.
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
fn interpret_ignored_numeric_property_requires_complete_json_number() {
    // C#: UpdateChecker.Interpret catches JsonException even for an unused property.
    // Mutants checker.rs:737:13 delete match arm b'0'; :737:30 += -> -= / *=.
    // Also :743:37 && -> ||, :743:54 / :751:30 == -> != and :747/:749 cursor arithmetic.
    for number in [
        "0", "-0", "1", "-12", "0.1", "-12.345", "1e2", "1E+2", "1e-2", "-0.1E+12",
    ] {
        let body = format!(r#"{{"tag_name":"v2.0.0","unused":{number}}}"#);
        let result = UpdateChecker::interpret(&body, current());
        assert_eq!(result.status(), UpdateCheckStatus::UpdateAvailable, "{number}");
        assert_eq!(result.latest_version(), Some("2.0.0"));
    }
    for number in [
        "-", "+1", "01", "-01", ".1", "1.", "1.e2", "1e", "1E+", "1e-", "1e+-2", "1e2.0",
    ] {
        let body = format!(r#"{{"tag_name":"v2.0.0","unused":{number}}}"#);
        assert_eq!(
            UpdateChecker::interpret(&body, current()).failure(),
            UpdateFailure::BadResponse,
            "{number}"
        );
    }
}

#[test]
fn interpret_escaped_release_version_decodes_each_hex_digit() {
    // C#: UpdateChecker.Interpret / JsonDocument.Parse Unicode escapes.
    // Mutants checker.rs:670-676: hex4 arithmetic replacements and cursor increment.
    for tag in [r#"v\u0032.\u0030.\u0030"#, r#"\u00762.0.0"#, r#"\u00562.0.0"#] {
        let body = format!(r#"{{"tag_name":"{tag}"}}"#);
        let result = UpdateChecker::interpret(&body, current());
        assert_eq!(result.status(), UpdateCheckStatus::UpdateAvailable, "{tag}");
        assert_eq!(result.latest_version(), Some("2.0.0"));
    }
    for tag in [r#"v\u003.0.0"#, r#"v\u00g2.0.0"#, r#"v\uFF12.0.0"#] {
        let body = format!(r#"{{"tag_name":"{tag}"}}"#);
        assert_eq!(
            UpdateChecker::interpret(&body, current()).failure(),
            UpdateFailure::BadResponse,
            "{tag}"
        );
    }
}
