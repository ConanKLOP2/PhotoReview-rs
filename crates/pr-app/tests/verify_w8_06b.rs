//! Adversarial verification tests for W8-06b (NOT committed). Expected values come from a .NET 10 run of
//! `Path.TrimEndingDirectorySeparator` + `Path.GetFileName` (the C# `DisplayName`) and `string.Equals(OrdinalIgnoreCase)`.

use pr_app::coordinators::transition::ImageTransitionDecision;
use pr_app::viewmodels::info_overlay::display_name;

#[test]
fn display_name_unc_share_root_is_whole_path() {
    assert_eq!(display_name(r"\\nas\photos"), r"\\nas\photos");
}

#[test]
fn display_name_unc_share_root_trailing_sep_is_whole_path() {
    assert_eq!(display_name(r"\\nas\photos\"), r"\\nas\photos\");
}

#[test]
fn display_name_unc_server_only() {
    assert_eq!(display_name(r"\\nas\"), r"\\nas\");
}

#[test]
fn display_name_unc_share_root_forward_slashes() {
    assert_eq!(display_name("//nas/share/"), "//nas/share/");
    assert_eq!(display_name("//nas/share"), "//nas/share");
}

#[test]
fn display_name_extended_drive_root() {
    assert_eq!(display_name(r"\\?\C:\"), r"\\?\C:\");
    assert_eq!(display_name(r"\\?\C:"), r"\\?\C:");
}

#[test]
fn display_name_extended_unc_root() {
    assert_eq!(display_name(r"\\?\UNC\nas\share\"), r"\\?\UNC\nas\share\");
    assert_eq!(display_name(r"\\?\UNC\nas\share"), r"\\?\UNC\nas\share");
}

#[test]
fn display_name_drive_relative() {
    assert_eq!(display_name("C:foo"), "foo");
}

#[test]
fn display_name_unc_subfolder_ok() {
    assert_eq!(display_name(r"\\nas\photos\a"), "a");
}

#[test]
fn transition_ordinal_ignore_case_edges() {
    let t = |a: &str, b: &str| ImageTransitionDecision::should_transition(Some(a), Some(b), false);
    assert!(t("\u{131}", "I")); // dotless i vs I: differ -> transition
    assert!(t("\u{17F}", "S"));
    assert!(t("\u{212A}", "k")); // Kelvin sign differs from k in .NET
    assert!(!t("\u{1C6}", "\u{1C5}"));
    assert!(t("\u{DF}", "\u{1E9E}"));
    assert!(!t("\u{10400}", "\u{10428}"));
    assert!(!t("\u{3C2}", "\u{3C3}"));
    assert!(t("\u{130}", "i"));
}

use pr_app::viewmodels::info_overlay::{
    FolderInfoLabels, InfoOverlayViewModel, SearchOutcome, SiblingImageFolders, ViewInputs,
};
use pr_core::settings::app_settings::AppSettings;

#[derive(Debug)]
struct L;
impl FolderInfoLabels for L {
    fn current(&self, n: &str) -> String {
        format!("C:{n}")
    }
    fn previous(&self, k: &str, n: &str) -> String {
        format!("P:{k}:{n}")
    }
    fn next(&self, k: &str, n: &str) -> String {
        format!("N:{k}:{n}")
    }
    fn pending(&self) -> String {
        "...".to_owned()
    }
}

#[test]
fn set_folder_same_folder_again_drops_siblings_and_searches_again() {
    let s = AppSettings {
        show_info_overlay: true,
        show_folder_info: true,
        ..AppSettings::default()
    };
    let i = ViewInputs::default();
    let mut vm = InfoOverlayViewModel::new(L);
    let r = vm.set_folder(&s, i, Some(r"C:\a"));
    let search = r.search.expect("search");
    vm.complete_search(
        &s,
        i,
        &search,
        SearchOutcome::Found(SiblingImageFolders {
            previous: Some(r"C:\p".into()),
            next: None,
        }),
    );
    assert!(vm.folder_info_text().contains("P:"));
    let r2 = vm.set_folder(&s, i, Some(r"C:\a"));
    assert!(
        r2.search.is_some(),
        "C# SetFolder clears _siblingsFolder so a new search starts"
    );
    assert_eq!(vm.folder_info_text(), "C:a     ...");
}

#[test]
fn hiding_folder_block_invalidates_pending_search() {
    let mut s = AppSettings {
        show_info_overlay: true,
        show_folder_info: true,
        ..AppSettings::default()
    };
    let i = ViewInputs::default();
    let mut vm = InfoOverlayViewModel::new(L);
    let search = vm.set_folder(&s, i, Some(r"C:\a")).search.expect("search");
    s.show_folder_info = false;
    vm.refresh(&s, i);
    // The late result belongs to a cancelled search (C# CancelPending bumped _request): dropped.
    vm.complete_search(
        &s,
        i,
        &search,
        SearchOutcome::Found(SiblingImageFolders {
            previous: Some(r"C:\p".into()),
            next: None,
        }),
    );
    s.show_folder_info = true;
    let r = vm.refresh(&s, i);
    assert!(
        r.search.is_some(),
        "stale result must have been dropped, so a fresh search is needed"
    );
    assert_eq!(vm.folder_info_text(), "C:a     ...");
}
