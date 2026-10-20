//! Port of `PhotoReview.App.Shared/ViewModels/SettingsReloadPath.cs` tests: `tests/PhotoReview.App.Tests/ViewModels/SettingsReloadPathTests.cs`
//! (C# baseline d92d6aae), plus the golden checks over `tests/golden/settings-reload-path.json` (216 cases, the full product of
//! the presented, group, entry, RAW and pair-mode values; oracle `oracle/W8-02b.cs`, command `settings-reload-path`).
//!
//! Unit W8-02b, contract `contracts/W8-02b.md`. Red stage: every stub body is `todo!()`, so each test here fails until the
//! porter implements the rule. `pairMode` does not change the result (C# `_ = pairMode`); the test rows still cover all modes.

mod format_support;

use format_support::{capture_group_of, opt_bool, opt_text, pair_mode_of, text_of};
use pr_app::viewmodels::format::settings_reload_path::choose_reload_path;
use pr_core::catalog::types::CaptureGroup;
use pr_core::model::RawPairMode;
use pr_test_support::golden;

const JPEG: &str = r"C:\p\a.jpg";
const RAW: &str = r"C:\p\a.cr2";

fn group() -> CaptureGroup {
    CaptureGroup::new(JPEG, RAW, None).expect("valid capture group")
}

const MODES: [RawPairMode; 3] = [RawPairMode::Separate, RawPairMode::PreferJpeg, RawPairMode::PreferRaw];

#[test]
fn choose_reload_path_raw_off_raw_member_falls_back_to_jpeg() {
    // C#: SettingsReloadPathTests.Choose_RawSupportOffAndRawMemberPresented_FallsBackToJpegPath (3 modes)
    let group = group();
    for mode in MODES {
        assert_eq!(
            choose_reload_path(Some(RAW), Some(&group), Some(JPEG), false, mode),
            Some(JPEG),
            "{mode:?}"
        );
    }
}

#[test]
fn choose_reload_path_raw_off_raw_member_other_case_falls_back_to_jpeg() {
    // C#: SettingsReloadPathTests.Choose_RawSupportOffAndRawMemberPresentedWithDifferentCase_FallsBackToJpegPath
    let group = group();
    let upper = RAW.to_uppercase();
    assert_eq!(
        choose_reload_path(
            Some(upper.as_str()),
            Some(&group),
            Some(JPEG),
            false,
            RawPairMode::PreferJpeg
        ),
        Some(JPEG)
    );
}

#[test]
fn choose_reload_path_raw_off_jpeg_member_is_kept() {
    // C#: SettingsReloadPathTests.Choose_RawSupportOffAndJpegMemberPresented_KeepsJpeg (3 modes)
    let group = group();
    for mode in MODES {
        assert_eq!(
            choose_reload_path(Some(JPEG), Some(&group), Some(JPEG), false, mode),
            Some(JPEG),
            "{mode:?}"
        );
    }
}

#[test]
fn choose_reload_path_raw_on_raw_member_is_kept() {
    // C#: SettingsReloadPathTests.Choose_RawSupportOnAndRawMemberPresented_KeepsTheRawPath (3 modes)
    // Separate: the RAW becomes its own entry; grouped modes: the group still resolves both members.
    let group = group();
    for mode in MODES {
        assert_eq!(
            choose_reload_path(Some(RAW), Some(&group), Some(JPEG), true, mode),
            Some(RAW),
            "{mode:?}"
        );
    }
}

#[test]
fn choose_reload_path_no_presented_uses_entry_path() {
    // C#: SettingsReloadPathTests.Choose_NoPresentedPath_UsesTheEntryPath
    assert_eq!(
        choose_reload_path(None, None, Some(JPEG), true, RawPairMode::Separate),
        Some(JPEG)
    );
    assert_eq!(choose_reload_path(None, None, None, true, RawPairMode::Separate), None);
}

#[test]
fn choose_reload_path_raw_off_without_group_keeps_presented() {
    // C#: SettingsReloadPathTests.Choose_RawSupportOffWithoutGroup_KeepsThePresentedPath
    assert_eq!(
        choose_reload_path(Some(RAW), None, Some(RAW), false, RawPairMode::Separate),
        Some(RAW)
    );
}

#[test]
fn choose_reload_path_raw_entry_raw_off_falls_back_to_jpeg_member() {
    // C#: SettingsReloadPathTests.Choose_NoPresentedPathRawEntryAndRawSupportOff_FallsBackToJpegMember (2 modes)
    let group = group();
    for mode in [RawPairMode::PreferJpeg, RawPairMode::PreferRaw] {
        assert_eq!(
            choose_reload_path(None, Some(&group), Some(RAW), false, mode),
            Some(JPEG),
            "{mode:?}"
        );
    }
}

#[test]
fn choose_reload_path_raw_entry_raw_on_keeps_entry_path() {
    // C#: SettingsReloadPathTests.Choose_NoPresentedPathRawEntryAndRawSupportOn_KeepsTheEntryPath
    let group = group();
    assert_eq!(
        choose_reload_path(None, Some(&group), Some(RAW), true, RawPairMode::PreferRaw),
        Some(RAW)
    );
}

#[test]
fn choose_reload_path_empty_presented_is_not_null() {
    // new: T19, C# `??` only coalesces null: an empty presented path is kept, it does not fall back to the entry path
    assert_eq!(
        choose_reload_path(Some(""), None, Some(JPEG), true, RawPairMode::Separate),
        Some("")
    );
}

#[test]
fn golden_settings_reload_path() {
    // new: golden-driven, every case of tests/golden/settings-reload-path.json (216 rows); collects all differences
    let doc = golden::load("settings-reload-path");
    let mut mismatches = Vec::new();
    for case in &doc.cases {
        let presented = opt_text(case, "presented");
        let entry = opt_text(case, "entry");
        let group = capture_group_of(&case["group"]);
        let raw_enabled = opt_bool(case, "raw_enabled").unwrap_or_else(|| panic!("no raw_enabled: {case}"));
        let pair_mode = pair_mode_of(&text_of(case, "pair_mode"));

        let actual = choose_reload_path(
            presented.as_deref(),
            group.as_ref(),
            entry.as_deref(),
            raw_enabled,
            pair_mode,
        )
        .map(str::to_owned);
        let expected = opt_text(case, "result");
        if actual != expected {
            mismatches.push(format!(
                "{}: expected {expected:?}, got {actual:?}",
                golden::str_of(case, "name")
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} settings-reload-path cases differ from the C# oracle:\n{}",
        mismatches.len(),
        doc.cases.len(),
        mismatches.join("\n")
    );
}
