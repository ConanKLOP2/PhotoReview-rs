//! Red tests for `pr_app::misc::build_info` (contract W8-04d). Port of `PhotoReview.App.Shared/BuildInfo.cs` (C# baseline
//! d92d6aae) and `PhotoReview.App.Tests/BuildInfoTests.cs` and `BuildInfoAssemblyGapTests.cs` (the pure tests only).
//!
//! The Localizer is the shipped catalog (`assets/languages/vi.json`, `en.json`) loaded through `LanguageLoader` on the
//! real file system: never a copy, never a hand-edited file (AGENTS.md, assets are synced from C#). The offset is
//! [`FixedOffset`], so no test depends on the machine's time zone.

use std::path::PathBuf;
use std::sync::Arc;

use pr_app::misc::build_info::{BuildStamp, FixedOffset, describe_parts, format_local_minute, parse_build_time};
use pr_core::io::physical::PhysicalFileSystem;
use pr_l10n::keys;
use pr_l10n::loader::LanguageLoader;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::LocArg;

/// `assets/languages` at the repository root, next to `crates/`.
fn shipped_languages_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets")
        .join("languages")
}

/// The shipped Vietnamese catalog (English fallback for missing keys), loaded from disk.
fn vi() -> Localizer {
    LanguageLoader::new(Arc::new(PhysicalFileSystem), shipped_languages_dir(), None).load(Some("vi"), None)
}

#[test]
fn describe_release_build_shows_version_local_build_time_and_sha() {
    // C#: BuildInfoTests.Describe_ReleaseBuild_ShowsVersionLocalBuildTimeAndSha
    let loc = vi();

    let text = describe_parts(
        &loc,
        Some("2.0.41+1a2b3c4d"),
        Some("2026-09-24 07:15:30 +07:00"),
        &FixedOffset(420),
    );

    assert_eq!(text, "Phiên bản 2.0.41 · build 24/09/2026 07:15 · 1a2b3c4d");
}

#[test]
fn describe_without_build_time_or_sha_shows_version_only() {
    // C#: BuildInfoTests.Describe_NoBuildTimeOrSha_ShowsVersionOnly
    let loc = vi();

    assert_eq!(
        describe_parts(&loc, Some("2.0.0-nogit"), None, &FixedOffset(0)),
        "Phiên bản 2.0.0-nogit"
    );
    assert_eq!(
        describe_parts(&loc, None, None, &FixedOffset(0)),
        "Phiên bản không xác định"
    );
}

#[test]
fn describe_with_a_version_that_starts_with_a_plus_shows_an_empty_version_and_the_commit() {
    // C#: BuildInfoAssemblyGapTests.Describe_WithAVersionThatStartsWithAPlus_ShowsAnEmptyVersionAndTheCommit
    let loc = vi();
    let empty_version_part = loc.format(keys::SETTINGS_VERSION_NUMBER, &[LocArg::new("version", "")]);

    let text = describe_parts(&loc, Some("+abc1234"), None, &FixedOffset(0));

    assert_eq!(text, format!("{empty_version_part} · abc1234"));
}

#[test]
fn whitespace_only_version_is_unknown_even_with_build_time() {
    // Trap 8 and rule 8 (new): None, "", whitespace-only and tab-only versions are unknown, build time or not.
    let loc = vi();
    let unknown = loc.get(keys::SETTINGS_VERSION_UNKNOWN);
    let build_time = Some("2026-09-24 07:15:30 +07:00");

    for version in [None, Some(""), Some("   "), Some("\t")] {
        assert_eq!(
            describe_parts(&loc, version, build_time, &FixedOffset(420)),
            unknown,
            "version {version:?}"
        );
    }
}

#[test]
fn split_is_at_the_first_plus() {
    // Trap 7 (new): the split is at the FIRST '+': the commit keeps every later '+'.
    let loc = vi();

    let text = describe_parts(&loc, Some("2.0.41+1a2b3+x"), None, &FixedOffset(0));

    assert_eq!(text, "Phiên bản 2.0.41 · 1a2b3+x");
}

#[test]
fn empty_commit_after_plus_is_omitted() {
    // Trap 8 and trap 16 (new): "2.0.41+" has a version and no commit part.
    let loc = vi();

    assert_eq!(
        describe_parts(&loc, Some("2.0.41+"), None, &FixedOffset(0)),
        "Phiên bản 2.0.41"
    );
}

#[test]
fn local_minute_uses_the_offset_at_the_instant_not_the_written_one() {
    // Trap 9 (new): the minute is shifted by FixedOffset(420) (+7h), not by the "+00:00" written in the string.
    let loc = vi();

    let text = describe_parts(
        &loc,
        Some("2.0.41+abc"),
        Some("2026-09-24 00:15:30 +00:00"),
        &FixedOffset(420),
    );

    assert_eq!(text, "Phiên bản 2.0.41 · build 24/09/2026 07:15 · abc");
}

#[test]
fn build_time_parse_is_strict() {
    // Rule 11 (new): exact shape, real calendar dates, signed hh:mm offset within .NET bounds (trap 13).
    // dotnet check of the zzz bounds (open question 8): NOT run in this red-test step; see the PR body.
    let valid = [
        "2026-09-24 07:15:30 +07:00",
        "2026-09-24 07:15:30 -00:00",
        "2026-09-24 07:15:30 +14:00",
        "2026-09-24 07:15:30 -14:00",
        "2028-02-29 12:00:00 +00:00",
        // .NET `zzz` takes a one-digit hour and an offset without the colon (verified on .NET 10).
        "2026-09-24 07:15:30 +7:00",
        "2026-09-24 07:15:30 +0700",
        "2026-09-24 07:15:30 -7:00",
        // A format space matches a space or U+00A0 (not a tab).
        "2026-09-24\u{A0}07:15:30 +07:00",
        "2026-09-24 07:15:30\u{A0}+07:00",
        // The UTC instant is exactly the DateTime bounds.
        "0001-01-01 05:00:00 +05:00",
        "9999-12-31 23:59:59 +00:00",
    ];
    let invalid = [
        "2026-02-30 07:15:30 +07:00",
        "2027-02-29 07:15:30 +00:00",
        "2026-09-24 07:15:30",
        "2026-09-24T07:15:30 +07:00",
        "26-09-24 07:15:30 +07:00",
        "2026-9-24 07:15:30 +07:00",
        "2026-09-24 07:15:30 +14:01",
        "2026-09-24 07:15:30 -14:01",
        "2026-09-24 07:15:30 +700",
        "2026-09-24  07:15:30 +07:00",
        "2026-09-24\t07:15:30 +07:00",
        " 2026-09-24 07:15:30 +07:00",
        "2026-09-24 07:15:30 +07:00 ",
        // UTC instant outside 0001..9999 (.NET returns false).
        "0001-01-01 00:00:00 +05:00",
        "9999-12-31 23:59:59 -05:00",
        "2026-09-24 07:15:30 UTC",
        "2026-09-24 25:15:30 +07:00",
        "",
    ];

    for text in valid {
        assert!(parse_build_time(Some(text)).is_some(), "{text} must parse");
    }
    for text in invalid {
        assert!(parse_build_time(Some(text)).is_none(), "{text} must not parse");
    }
    assert!(parse_build_time(None).is_none());
}

#[test]
fn build_time_parse_keeps_the_instant_not_the_written_offset() {
    // Rule 11 and trap 9 (new): the same instant written with two offsets parses to the same UTC second.
    let a = parse_build_time(Some("2026-09-24 07:15:30 +07:00")).expect("valid build time");
    let b = parse_build_time(Some("2026-09-24 00:15:30 +00:00")).expect("valid build time");

    assert_eq!(a, b);
}

#[test]
fn format_local_minute_handles_pre_1970_instants() {
    // Rule 12 (new, div_euclid): instants before 1970 must not truncate toward zero.
    assert_eq!(format_local_minute(-1, 0), "31/12/1969 23:59");
    assert_eq!(format_local_minute(-60, 0), "31/12/1969 23:59");
    assert_eq!(format_local_minute(-1, 60), "01/01/1970 00:59");
}

#[test]
fn format_local_minute_handles_leap_day() {
    // Rule 12 (new): 2028-02-29T12:00Z is unix 1835438400 (21243 days after 1970-01-01, plus 12 hours).
    // Lesson of Wave 2: leap-year mutants survive without this test.
    assert_eq!(format_local_minute(1_835_438_400, 0), "29/02/2028 12:00");
}

#[test]
fn describe_out_of_range_build_time_omits_the_build_part() {
    // Trap 13 (DIVERGENCE): C# ToLocalTime throws for a local time before year 0001; Rust omits the build part.
    // The written instant is 0001-01-01 00:30 UTC; FixedOffset(-60) moves it to 0000-12-31 23:30 (out of range).
    let loc = vi();

    let text = describe_parts(
        &loc,
        Some("2.0.41+abc"),
        Some("0001-01-01 00:30:00 +00:00"),
        &FixedOffset(-60),
    );

    assert_eq!(text, "Phiên bản 2.0.41 · abc");
}

#[test]
fn version_empty_string_is_some_empty_not_none() {
    // Trap 16 (new): BuildStamp::version() returns Some("") for a stored empty version, never None.
    let empty = BuildStamp {
        informational_version: Some(String::new()),
        build_time: None,
    };
    let absent = BuildStamp::default();

    assert_eq!(empty.version(), Some(""));
    assert_eq!(absent.version(), None);
}

#[test]
fn utc_overflow_build_time_has_no_build_part() {
    // .NET: TryParseExact is false when the UTC instant is outside 0001..9999 (verified on .NET 10).
    let loc = vi();
    let text = describe_parts(
        &loc,
        Some("1.0+abc"),
        Some("0001-01-01 00:00:00 +05:00"),
        &FixedOffset(420),
    );
    let with_build = describe_parts(
        &loc,
        Some("1.0+abc"),
        Some("2026-09-24 07:15:30 +07:00"),
        &FixedOffset(420),
    );
    assert!(
        with_build.chars().count() > text.chars().count(),
        "{text} / {with_build}"
    );
}
