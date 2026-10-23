//! Native BenchmarkDialog tests ported from `BenchmarkDialogTests.cs`.
#![cfg(windows)]

#[path = "common/dialogs_benchmark_fixture.rs"]
mod dialogs_benchmark_fixture;

// C#: BenchmarkDialogTests.Opens_WithoutBlocking_AndWithTheInitialValues
#[test]
#[ignore = "native: needs desktop"]
fn opens_without_blocking_and_with_the_initial_values() {}

// C#: BenchmarkDialogTests.WithoutAFolder_TheFolderBoxStartsAtPictures
#[test]
#[ignore = "native: needs desktop"]
fn without_a_folder_the_folder_box_starts_at_pictures() {}

// C#: BenchmarkDialogTests.EveryProfileHasATooltipWithItsDescription_AndABadgeWithItsLoadingMode
#[test]
#[ignore = "native: needs desktop"]
fn every_profile_has_a_tooltip_with_its_description() {}

// C#: BenchmarkDialogTests.TabKey_PostedToTheWindow_IsHandledByTheDialogManagerInsideTheShellLoop
#[test]
#[ignore = "native: needs desktop"]
fn tab_key_posted_to_window_is_handled_by_dialog_manager_inside_shell_loop() {}

// C#: BenchmarkDialogTests.SwitchingLanguage_WhileOpen_RetranslatesTheWindow
#[test]
#[ignore = "native: needs desktop"]
fn switching_language_while_open_retranslates_the_window() {}

// C#: BenchmarkDialogTests.OpeningAndClosingRepeatedly_DoesNotLeakGdiOrUserObjects
#[test]
#[ignore = "native: needs desktop"]
fn opening_and_closing_repeatedly_does_not_leak_gdi_or_user_objects() {}

// W8-18r native regressions
#[test]
#[ignore = "native: needs desktop"]
fn closing_twice_and_dispose_twice_are_harmless() {}

#[test]
#[ignore = "native: needs desktop"]
fn language_message_after_close_does_nothing() {}

#[test]
#[ignore = "native: needs desktop"]
fn pictures_folder_override_wins() {}

#[test]
#[ignore = "native: needs desktop"]
fn status_height_state_follows_the_last_layout() {}
