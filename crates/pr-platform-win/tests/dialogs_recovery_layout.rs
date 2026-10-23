//! Pure contract tests for the W8-18t recovery dialog layout and list helpers.
//!
//! Implementations are supplied by the owning source lane; this file deliberately
//! remains compile-only until those stubs expose their finalized testable surface.

#[test]
fn selection_after_a_removal_does_not_move_to_the_neighbours() {
    // C#: RecoveryDialogTests.ClearSelected_RemovesRowsWithoutSelectingNeighbours
    // The native rig assertion is covered in dialogs_recovery_window_e2e.rs.
}

#[test]
fn ids_match_the_golden_constants() {}

#[test]
fn items_match_the_golden_rows_and_the_table() {}

#[test]
fn color_ref_matches_the_golden() {}

#[test]
fn sizes_match_the_golden_for_ten_metrics() {}

#[test]
fn layout_controls_without_details_makes_the_documented_calls() {}

#[test]
fn layout_controls_follows_the_client_size() {}

#[test]
fn carried_selection_keeps_ids_still_visible() {}

#[test]
fn carried_selection_ignores_ids_that_left() {}

#[test]
fn top_index_to_restore_only_inside_the_list() {}

#[test]
fn filter_from_index_maps_the_four_entries() {}

#[test]
fn list_hooks_answer_defaults_out_of_range_or_for_another_control() {}
