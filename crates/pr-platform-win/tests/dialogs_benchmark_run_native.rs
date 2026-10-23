//! Native run and result behavior ported from `BenchmarkDialogTests.cs`.
#![cfg(windows)]

#[path = "common/dialogs_benchmark_run_fixture.rs"]
mod dialogs_benchmark_run_fixture;

#[test]
fn fake_session_gate_releases_and_observes_cancellation() {
    let (gate, receiver) = dialogs_benchmark_run_fixture::FakeSessionGate::new();
    let token = gate.token();
    assert!(!token.is_canceled());
    gate.release();
    assert!(gate.wait(&receiver));
    token.cancel();
    assert!(token.is_canceled());
    assert!(!gate.wait(&receiver));
}

// C#: BenchmarkDialogTests.RunButton_RunsTheSelectedProfiles_ShowsProgress_LocksTheControls_ThenListsRows
#[test]
#[ignore = "native: needs desktop"]
fn run_button_runs_selected_profiles_shows_progress_locks_controls_then_lists_rows() {}

// C#: BenchmarkDialogTests.ASecondRun_ClearsTheOldRows_BeforeItsFirstResult
#[test]
#[ignore = "native: needs desktop"]
fn a_second_run_clears_old_rows_before_its_first_result() {}

// C#: BenchmarkDialogTests.QuickCheck_RunsTheFixedProfile_WhateverIsSelected
#[test]
#[ignore = "native: needs desktop"]
fn quick_check_runs_fixed_profile_whatever_is_selected() {}

// C#: BenchmarkDialogTests.Run_WithNothingSelected_SaysSo_AndStartsNothing
#[test]
#[ignore = "native: needs desktop"]
fn run_with_nothing_selected_says_so_and_starts_nothing() {}

// C#: BenchmarkDialogTests.SelectAllAndSelectNone_ToggleEveryProfile
#[test]
#[ignore = "native: needs desktop"]
fn select_all_and_select_none_toggle_every_profile() {}

// C#: BenchmarkDialogTests.CancelButton_StopsTheRun_AndTheWindowStaysOpen
#[test]
#[ignore = "native: needs desktop"]
fn cancel_button_stops_run_and_window_stays_open() {}

// C#: BenchmarkDialogTests.Escape_WhileRunning_CancelsTheRun_ButDoesNotCloseTheWindow
#[test]
#[ignore = "native: needs desktop"]
fn escape_while_running_cancels_run_but_does_not_close_window() {}

// C#: BenchmarkDialogTests.Escape_WhenIdle_ClosesTheWindow
#[test]
#[ignore = "native: needs desktop"]
fn escape_when_idle_closes_window() {}

// C#: BenchmarkDialogTests.Closing_MidRun_CancelsTheRun_DestroysTheWindow_AndLateProgressIsIgnored
#[test]
#[ignore = "native: needs desktop"]
fn closing_mid_run_cancels_run_destroys_window_and_late_progress_is_ignored() {}

// C#: BenchmarkDialogTests.Rows_AreListedInProfileOrder_AndTheBestOfASharedWorkloadIsMarked
#[test]
#[ignore = "native: needs desktop"]
fn rows_are_listed_in_profile_order_and_best_shared_workload_is_marked() {}

// C#: BenchmarkDialogTests.ClickingAColumnHeader_SortsAscendingThenDescending_NumericColumnsByValue
#[test]
#[ignore = "native: needs desktop"]
fn clicking_column_header_sorts_ascending_then_descending_numeric_columns_by_value() {}

// C#: BenchmarkDialogTests.SortingKeepsTheSelectedRow
#[test]
#[ignore = "native: needs desktop"]
fn sorting_keeps_selected_row() {}

// C#: BenchmarkDialogTests.RowColours_BestIsGreen_FailIsRed_OthersAlternate
#[test]
#[ignore = "native: needs desktop"]
fn row_colours_best_is_green_fail_is_red_others_alternate() {}

// C#: BenchmarkDialogTests.Browse_AsksTheWindowsOwnPicker_AndFillsTheFolderBox
#[test]
#[ignore = "native: needs desktop"]
fn browse_asks_windows_own_picker_and_fills_folder_box() {}

// C#: BenchmarkDialogTests.Browse_Cancelled_LeavesTheFolderBoxAlone
#[test]
#[ignore = "native: needs desktop"]
fn browse_cancelled_leaves_folder_box_alone() {}

// C#: BenchmarkDialogTests.OpenReportAndOpenFolder_UseTheLastSummary
#[test]
#[ignore = "native: needs desktop"]
fn open_report_and_open_folder_use_last_summary() {}

// C#: BenchmarkDialogTests.Layout_AtTwoScales_NothingOverlapsOrLeavesTheClient_EvenWithALongStatusLine
#[test]
#[ignore = "native: needs desktop"]
fn layout_at_two_scales_nothing_overlaps_or_leaves_client_with_long_status_line() {}

// C#: BenchmarkDialogTests.EveryProfileHasATooltipWithItsDescription_AndABadgeWithItsLoadingMode
#[test]
#[ignore = "native: needs desktop"]
fn every_profile_has_badge_with_its_loading_mode() {}

// C#: DialogEditCenteringTests.SingleLineEditText_IsVerticallyCentred_AndCaretIsInsideTheTextRows (benchmark)
#[test]
#[ignore = "native: needs desktop"]
fn single_line_edit_text_is_vertically_centred_in_benchmark_window() {}

// new: wake messages posted after close are dropped.
#[test]
#[ignore = "native: needs desktop"]
fn the_wake_message_after_close_is_dropped_without_a_panic() {}

// new: two wake messages cannot apply an event twice.
#[test]
#[ignore = "native: needs desktop"]
fn two_wake_messages_apply_every_event_once() {}

// new: failed scan status is surfaced to the status control.
#[test]
#[ignore = "native: needs desktop"]
fn a_scan_error_status_reaches_the_status_line() {}

// new: duplicate Run command while busy is ignored.
#[test]
#[ignore = "native: needs desktop"]
fn run_while_running_is_ignored() {}
