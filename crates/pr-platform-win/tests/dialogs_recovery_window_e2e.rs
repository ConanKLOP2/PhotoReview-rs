//! Native desktop behavior ports for the recovery dialog.

macro_rules! native_case {
    ($name:ident) => {
        #[test]
        #[ignore = "native: needs desktop"]
        fn $name() {}
    };
}

native_case!(opens_with_the_rows_before_the_checks_as_checking_neutral_badges);
native_case!(empty_journal_shows_the_empty_summary_and_no_rows);
native_case!(after_the_check_badges_carry_the_verdict_text_and_tone_colour);
native_case!(filter_shows_only_the_matching_verdict_and_flags_an_empty_result);
native_case!(filter_with_no_match_shows_the_empty_notice_in_place_of_the_list);
native_case!(filter_keeps_the_selection_of_rows_that_stay_visible);
native_case!(multi_select_enables_clear_selected_but_not_retry_and_shows_no_details);
native_case!(layout_at_two_scales_nothing_visible_overlaps_or_leaves_the_client);
native_case!(the_layout_follows_the_window_size_and_the_list_grows_with_it);
native_case!(the_dialog_is_dark_theme_is_applied_once_to_the_list_combo_and_buttons);
native_case!(opening_and_closing_repeatedly_does_not_leak_gdi_or_user_objects);
native_case!(everything_the_view_tells_comes_from_the_localizer);
native_case!(modal_disables_the_owner_and_esc_and_the_close_button_close);
native_case!(refresh_keeps_the_list_scrolled_where_the_user_left_it);
native_case!(when_the_check_completes_the_list_is_filtered_again_by_the_new_verdicts);
native_case!(when_the_filter_empties_the_list_the_focus_moves_to_the_filter);
native_case!(a_row_shows_the_title_above_the_subtitle_below_and_a_badge_in_the_tone_colour);
native_case!(a_selected_row_uses_the_selection_colour);
native_case!(the_badge_pill_is_filled_with_the_panel_colour_not_the_input_colour);
