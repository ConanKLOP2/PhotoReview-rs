//! Port of `tests/PhotoReview.App.Tests/Localization/LanguageOptionsTests.cs` (C# baseline d92d6aae, I18N L08).
//! Each test names its C# twin in a comment. Red tests (W8-04a): every call into `language_options` panics via
//! `todo!()` until the porter implements the bodies. Labels come from an explicit `Localizer` (no ambient state),
//! so these tests need no global lock.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pr_app::localization::language_options::{LanguageOption, build, index_of, to_setting};
use pr_core::abstractions::FileSystem;
use pr_l10n::loader::{LanguageInfo, LanguageLoader, UiCulture};
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_test_support::FakeFileSystem;

const SHIPPED: &str = r"C:\App\Languages";
const USER: &str = r"C:\Users\u\AppData\Local\PhotoReviewRs\Languages";
const VI_CATALOG: &str = include_str!("../../../assets/languages/vi.json");

fn info(code: &str, name: &str, native_name: &str, is_user_provided: bool) -> LanguageInfo {
    LanguageInfo {
        code: code.to_owned(),
        name: name.to_owned(),
        native_name: native_name.to_owned(),
        is_user_provided,
    }
}

/// C# `Languages` fixture: en, vi, and a user-provided fr.
fn languages() -> Vec<LanguageInfo> {
    vec![
        info("en", "English", "English", false),
        info("vi", "Vietnamese", "Tiếng Việt", false),
        info("fr", "French", "Français", true),
    ]
}

/// The Vietnamese localizer (C# default `Localizer.Current` in the Vietnamese test setup): the shipped `vi.json`
/// is loaded from a fake folder, so the labels are the real catalog text.
fn vietnamese() -> Localizer {
    let fs = Arc::new(FakeFileSystem::new());
    fs.add_dir(SHIPPED);
    fs.add_file(Path::new(SHIPPED).join("vi.json"), VI_CATALOG.as_bytes());
    let fs_dyn: Arc<dyn FileSystem> = fs.clone();
    LanguageLoader::new(fs_dyn, SHIPPED, Some(PathBuf::from(USER))).load(Some("vi"), Some(&UiCulture::from_name("vi")))
}

#[test]
fn build_auto_first_then_native_names_user_files_marked() {
    // C#: LanguageOptionsTests.Build_AutoFirst_ThenNativeNames_UserFilesMarked
    let text = vietnamese();
    let options = build(&languages(), &text);

    let codes: Vec<&str> = options.iter().map(|o| o.code.as_str()).collect();
    assert_eq!(codes, ["auto", "en", "vi", "fr"]);
    assert_eq!(options[0].display_name, "Tự động (theo Windows)");
    assert!(options[0].is_auto());
    assert_eq!(options[1].display_name, "English");
    assert_eq!(options[2].display_name, "Tiếng Việt");
    assert_eq!(options[3].display_name, "Français (tệp người dùng)");
}

#[test]
fn build_uses_current_language_for_its_own_labels() {
    // C#: LanguageOptionsTests.Build_UsesCurrentLanguageForItsOwnLabels (English localizer)
    let english = built_in_english_localizer();
    let options = build(&languages(), &english);

    assert_eq!(options[0].display_name, "Auto (Windows)");
    assert_eq!(options[3].display_name, "Français (user file)");
}

#[test]
fn index_of_maps_setting_value_to_entry() {
    // C#: LanguageOptionsTests.IndexOf_MapsSettingValueToEntry ([InlineData] rows, 8 cases)
    let options = build(&languages(), &built_in_english_localizer());
    let rows: &[(Option<&str>, Option<usize>)] = &[
        (Some("auto"), Some(0)),
        (Some("AUTO"), Some(0)),
        (None, Some(0)),
        (Some(""), Some(0)),
        (Some("en"), Some(1)),
        (Some("VI"), Some(2)),
        (Some(" fr "), Some(3)),
        (Some("de"), None),
    ];
    for (setting, expected) in rows {
        assert_eq!(index_of(&options, *setting), *expected, "setting {setting:?}");
    }
}

#[test]
fn to_setting_selected_entry_wins_no_selection_keeps_current() {
    // C#: LanguageOptionsTests.ToSetting_SelectedEntryWins_NoSelectionKeepsCurrent
    let options = build(&languages(), &built_in_english_localizer());

    assert_eq!(to_setting(Some(&options[0]), Some("vi")), "auto");
    assert_eq!(to_setting(Some(&options[2]), Some("auto")), "vi");
    // a removed user language is kept, not reset
    assert_eq!(to_setting(None, Some("de")), "de");
    assert_eq!(to_setting(None, Some("  ")), "auto");
}

#[test]
fn round_trip_every_setting_value_survives_picker_and_save() {
    // C#: LanguageOptionsTests.RoundTrip_EverySettingValueSurvivesPickerAndSave
    let options = build(&languages(), &built_in_english_localizer());
    for value in ["auto", "en", "vi", "fr"] {
        let index = index_of(&options, Some(value)).expect("every saved value has an entry");
        assert_eq!(to_setting(Some(&options[index]), Some("unused")), value);
    }
}

#[test]
fn auto_entry_is_always_first_even_with_no_languages() {
    // Rust-only: the picker always offers `auto`, even when no catalog was found (golden
    // `no-languages-only-auto-entry`).
    let options = build(&[], &built_in_english_localizer());

    assert_eq!(options.len(), 1);
    assert_eq!(options[0].code, "auto");
    assert!(options[0].is_auto());
}

#[test]
fn index_of_trims_setting_but_to_setting_does_not_trim() {
    // Rust-only (contract trap T3): `index_of` trims its input before matching, `to_setting` returns `current` as
    // given.
    let options = build(&languages(), &built_in_english_localizer());

    assert_eq!(index_of(&options, Some(" vi ")), Some(2));
    assert_eq!(to_setting(None, Some(" vi ")), " vi ");
}

#[test]
fn build_keeps_input_order() {
    // Rust-only (contract trap T14): `build` never sorts; `LanguageLoader::discover_languages` does.
    let reversed = vec![
        info("fr", "French", "Français", true),
        info("vi", "Vietnamese", "Tiếng Việt", false),
        info("en", "English", "English", false),
    ];
    let options: Vec<LanguageOption> = build(&reversed, &built_in_english_localizer());

    let codes: Vec<&str> = options.iter().map(|o| o.code.as_str()).collect();
    assert_eq!(codes, ["auto", "fr", "vi", "en"]);
}

#[test]
fn blank_native_name_uses_code() {
    // Rust-only (contract semantics 1): a blank native name falls back to the language code.
    let blank = vec![info("xx", "Unknown", "   ", false)];
    let options = build(&blank, &built_in_english_localizer());

    assert_eq!(options[1].code, "xx");
    assert_eq!(options[1].display_name, "xx");
}
