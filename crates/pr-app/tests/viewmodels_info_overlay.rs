//! Tests of `crates/pr-app/src/viewmodels/info_overlay.rs` (contract `contracts/W8-06b.md`, sub-part (d)): the C# twins of
//! `ViewModels/InfoOverlayFaultTests`, `InfoOverlayFontSizeTests` and `InfoOverlayZoomIndicatorTests` (the `DisplayZoomPercent_*`
//! facts of that file belong to W8-06a, not here), plus the `// New test:` cases that the C# suite does not have.
//! Port of `PhotoReview.App.Shared/ViewModels/InfoOverlayViewModel.cs` (196 lines).
//!
//! Labels: the English fallbacks of `main.folderInfo.*` (`"📁 {folder}"`, `"…"`, `"◀ {key}: {folder}"`, `"{key}: {folder} ▶"`).
//! The golden `info-overlay-view-model` (scripts, every step) is in `golden_info_overlay.rs`.
//!
//! Every test here fails through `todo!()` until the bodies land.

use pr_app::viewmodels::info_overlay::{
    FolderInfoLabels, InfoOverlayChanges, InfoOverlayProperty, InfoOverlayViewModel, PART_SEPARATOR, SearchOutcome,
    SiblingImageFolders, ViewInputs, display_name,
};
use pr_core::settings::app_settings::AppSettings;

/// English fallbacks of the folder-info catalog keys (what the C# golden and the C# tests see).
#[derive(Debug)]
struct EnglishLabels;

impl FolderInfoLabels for EnglishLabels {
    fn current(&self, folder_name: &str) -> String {
        format!("📁 {folder_name}")
    }

    fn previous(&self, key: &str, folder_name: &str) -> String {
        format!("◀ {key}: {folder_name}")
    }

    fn next(&self, key: &str, folder_name: &str) -> String {
        format!("{key}: {folder_name} ▶")
    }

    fn pending(&self) -> String {
        "…".to_owned()
    }
}

/// The placeholder line of a folder while its siblings are searched: `current(name) + PART_SEPARATOR + pending()`.
fn placeholder_for(folder_name: &str) -> String {
    format!("📁 {folder_name}{PART_SEPARATOR}…")
}

fn overlay_settings() -> AppSettings {
    let mut settings = AppSettings::default();
    settings.show_info_overlay = true;
    settings.show_folder_info = true;
    settings
}

// C#: InfoOverlayFaultTests.SiblingSearchFault_ShowsTheFolderWithoutSiblings_NotAStuckPlaceholder (5 InlineData rows:
// IOException, UnauthorizedAccessException, ArgumentException, NotSupportedException, InvalidOperationException)
// Any finder error maps to `SearchOutcome::Failed`: the folder is shown with no siblings, never the placeholder. The Rust
// twin is one test because the C# placeholder-holding trick (a blocking finder) needs threads; the golden covers the
// IOException and UnauthorizedAccessException rows as sequential cases.
#[test]
fn complete_search_failed_shows_folder_without_siblings_not_stuck_placeholder() {
    let settings = overlay_settings();
    let inputs = ViewInputs::default();
    let mut overlay = InfoOverlayViewModel::new(EnglishLabels);

    let first = overlay.set_folder(&settings, inputs, Some(r"C:\photos\trip"));
    let search = first
        .search
        .expect("a folder with the folder block on must start a sibling search");
    assert_eq!(
        overlay.folder_info_text(),
        placeholder_for("trip"),
        "placeholder while searching"
    );

    let second = overlay.complete_search(&settings, inputs, &search, SearchOutcome::Failed);

    assert_eq!(overlay.folder_info_text(), "📁 trip");
    assert!(second.changes.contains(InfoOverlayProperty::FolderInfoText));
    assert!(second.search.is_none(), "a failed search is not retried");
}

// C#: InfoOverlayFontSizeTests.FontSize_ReadsFromSettings_ExifFontSizeIsOnePointSmaller (3 InlineData rows: 8, 12, 24)
#[test]
fn font_size_reads_settings_exif_is_one_point_smaller() {
    for configured in [8.0, 12.0, 24.0] {
        let mut settings = AppSettings::default();
        settings.info_overlay_font_size = configured;

        assert_eq!(
            InfoOverlayViewModel::<EnglishLabels>::font_size(&settings),
            configured,
            "configured {configured}"
        );
        assert_eq!(
            InfoOverlayViewModel::<EnglishLabels>::exif_font_size(&settings),
            configured - 1.0,
            "configured {configured}"
        );
    }
}

// C#: InfoOverlayFontSizeTests.Refresh_RaisesPropertyChanged_ForFontSizeAndExifFontSize
// The bit set (`InfoOverlayChanges`) replaces the C# `PropertyChanged` event list (D-W8-1).
#[test]
fn refresh_notifies_font_size_and_exif_font_size() {
    let mut settings = AppSettings::default();
    settings.info_overlay_font_size = 12.0;
    let mut overlay = InfoOverlayViewModel::new(EnglishLabels);

    settings.info_overlay_font_size = 20.0;
    let refresh = overlay.refresh(&settings, ViewInputs::default());

    assert!(refresh.changes.contains(InfoOverlayProperty::FontSize));
    assert!(refresh.changes.contains(InfoOverlayProperty::ExifFontSize));
    assert_eq!(InfoOverlayViewModel::<EnglishLabels>::font_size(&settings), 20.0);
    assert_eq!(InfoOverlayViewModel::<EnglishLabels>::exif_font_size(&settings), 19.0);
}

// C#: InfoOverlayZoomIndicatorTests.IsZoomIndicatorVisible_False_WhenSettingOff_EvenWithImage
#[test]
fn is_zoom_indicator_visible_false_when_setting_off_even_with_image() {
    let mut settings = AppSettings::default();
    settings.show_zoom_indicator = false;
    let inputs = ViewInputs {
        has_image: true,
        zoom_percent: 100,
    };

    assert!(!InfoOverlayViewModel::<EnglishLabels>::is_zoom_indicator_visible(
        &settings, inputs
    ));
}

// C#: InfoOverlayZoomIndicatorTests.IsZoomIndicatorVisible_False_WhenNoImage_EvenWithSettingOn
#[test]
fn is_zoom_indicator_visible_false_when_no_image_even_with_setting_on() {
    let mut settings = AppSettings::default();
    settings.show_zoom_indicator = true;
    let inputs = ViewInputs {
        has_image: false,
        zoom_percent: 100,
    };

    assert!(!InfoOverlayViewModel::<EnglishLabels>::is_zoom_indicator_visible(
        &settings, inputs
    ));
}

// C#: InfoOverlayZoomIndicatorTests.IsZoomIndicatorVisible_True_WhenSettingOnAndImageShown
#[test]
fn is_zoom_indicator_visible_true_when_setting_on_and_image_shown() {
    let mut settings = AppSettings::default();
    settings.show_zoom_indicator = true;
    let inputs = ViewInputs {
        has_image: true,
        zoom_percent: 100,
    };

    assert!(InfoOverlayViewModel::<EnglishLabels>::is_zoom_indicator_visible(
        &settings, inputs
    ));
}

// C#: InfoOverlayZoomIndicatorTests.ZoomIndicatorText_FormatsWholePercent (3 InlineData rows: 100, 37, 400)
#[test]
fn zoom_indicator_text_formats_whole_percent() {
    for percent in [100, 37, 400] {
        let inputs = ViewInputs {
            has_image: true,
            zoom_percent: percent,
        };

        assert_eq!(
            InfoOverlayViewModel::<EnglishLabels>::zoom_indicator_text(inputs),
            format!("{percent}%")
        );
    }
}

// New test: a search is requested only when the folder block needs one. A hidden block never searches, the same folder
// is not searched again while its search is pending, and once the siblings are known a refresh does not search again.
// New test:
#[test]
fn new_test_search_is_requested_only_when_needed() {
    let inputs = ViewInputs::default();

    let mut hidden_settings = AppSettings::default();
    hidden_settings.show_folder_info = false;
    let mut hidden = InfoOverlayViewModel::new(EnglishLabels);
    assert!(
        hidden
            .set_folder(&hidden_settings, inputs, Some(r"C:\photos\trip"))
            .search
            .is_none()
    );

    let settings = overlay_settings();
    let mut overlay = InfoOverlayViewModel::new(EnglishLabels);
    let first = overlay.set_folder(&settings, inputs, Some(r"C:\photos\trip"));
    let search = first.search.expect("first show of a folder searches");
    assert!(
        overlay.refresh(&settings, inputs).search.is_none(),
        "same folder already pending: no second search"
    );

    let done = overlay.complete_search(
        &settings,
        inputs,
        &search,
        SearchOutcome::Found(SiblingImageFolders {
            previous: Some(r"C:\photos\a".to_owned()),
            next: None,
        }),
    );
    assert!(done.search.is_none());
    assert!(
        overlay.refresh(&settings, inputs).search.is_none(),
        "siblings known for this folder: no search"
    );
}

// New test: the placeholder is already in the text when `set_folder` returns the search, so a caller that completes
// the search on the same thread cannot see a late placeholder overwrite the answer (contract trap T11).
// New test:
#[test]
fn new_test_placeholder_is_written_before_the_request_is_returned() {
    let settings = overlay_settings();
    let mut overlay = InfoOverlayViewModel::new(EnglishLabels);

    let refresh = overlay.set_folder(&settings, ViewInputs::default(), Some(r"C:\photos\trip"));

    assert!(refresh.search.is_some());
    assert!(refresh.changes.contains(InfoOverlayProperty::FolderInfoText));
    assert_eq!(overlay.folder_info_text(), placeholder_for("trip"));
}

// New test: a result for a request the user already left is dropped (C# `_request` check). The stale search changes
// nothing and the newer folder keeps its own placeholder until its own result arrives.
// New test:
#[test]
fn new_test_stale_search_result_is_dropped() {
    let settings = overlay_settings();
    let inputs = ViewInputs::default();
    let mut overlay = InfoOverlayViewModel::new(EnglishLabels);

    let stale = overlay
        .set_folder(&settings, inputs, Some(r"C:\photos\old"))
        .search
        .expect("search for old");
    let current = overlay
        .set_folder(&settings, inputs, Some(r"C:\photos\trip"))
        .search
        .expect("search for trip");
    assert_ne!(stale.request, current.request);

    let late = overlay.complete_search(
        &settings,
        inputs,
        &stale,
        SearchOutcome::Found(SiblingImageFolders {
            previous: Some(r"C:\photos\x".to_owned()),
            next: None,
        }),
    );

    assert!(late.changes.is_empty(), "stale result raises nothing");
    assert!(late.search.is_none());
    assert_eq!(overlay.folder_info_text(), placeholder_for("trip"));

    let answer = overlay.complete_search(
        &settings,
        inputs,
        &current,
        SearchOutcome::Found(SiblingImageFolders::default()),
    );
    assert_eq!(overlay.folder_info_text(), "📁 trip");
    assert!(answer.changes.contains(InfoOverlayProperty::FolderInfoText));
}

// New test: a cancelled search changes nothing (contract T10: `Cancelled` is not `Failed`). The placeholder stays, and no
// new search starts for the folder that is still marked as pending.
// New test:
#[test]
fn new_test_cancelled_search_changes_nothing() {
    let settings = overlay_settings();
    let inputs = ViewInputs::default();
    let mut overlay = InfoOverlayViewModel::new(EnglishLabels);
    let search = overlay
        .set_folder(&settings, inputs, Some(r"C:\photos\trip"))
        .search
        .expect("search for trip");

    let cancelled = overlay.complete_search(&settings, inputs, &search, SearchOutcome::Cancelled);

    assert!(cancelled.changes.is_empty());
    assert!(cancelled.search.is_none());
    assert_eq!(overlay.folder_info_text(), placeholder_for("trip"));
    assert!(overlay.refresh(&settings, inputs).search.is_none());
}

// New test: `display_name` edge cases (contract semantics 10 and trap T7). One trailing separator is removed, a root is
// not trimmed, an empty last name returns the input unchanged, and both separator kinds count.
// New test:
#[test]
fn new_test_display_name_edge_cases() {
    let rows = [
        (r"C:\photos\trip", "trip"),
        (r"C:\photos\trip\", "trip"),
        (r"C:\photos\\", r"C:\photos\\"),
        (r"C:\", r"C:\"),
        (r"\", r"\"),
        ("/", "/"),
        ("/home/user/pics/", "pics"),
        ("C:/photos/trip", "trip"),
        ("trip", "trip"),
        ("", ""),
    ];
    for (folder, expected) in rows {
        assert_eq!(display_name(folder), expected, "display_name({folder:?})");
    }
}

// New test: `InfoOverlayChanges` keeps its bits and iterates in enum (C# raise) order, whatever the insertion order.
// New test:
#[test]
fn new_test_info_overlay_changes_iter_is_in_enum_order() {
    let mut changes = InfoOverlayChanges::none();
    assert!(changes.is_empty());
    assert_eq!(changes, InfoOverlayChanges::default());

    changes.insert(InfoOverlayProperty::FolderInfoText);
    changes.insert(InfoOverlayProperty::FontSize);
    changes.insert(InfoOverlayProperty::IsFileInfoVisible);

    assert!(!changes.is_empty());
    assert!(changes.contains(InfoOverlayProperty::FontSize));
    assert!(!changes.contains(InfoOverlayProperty::ZoomIndicatorText));
    let order: Vec<InfoOverlayProperty> = changes.iter().collect();
    assert_eq!(
        order,
        vec![
            InfoOverlayProperty::IsFileInfoVisible,
            InfoOverlayProperty::FontSize,
            InfoOverlayProperty::FolderInfoText,
        ]
    );
}
