use pr_app::presenters::recovery_window::RecoveryCheckView;
use pr_platform_win::dialogs::recovery::actions::{CheckEffect, CheckEffects, CommandAction, command_action};
use pr_platform_win::dialogs::recovery::layout::*;

#[test]
fn command_action_maps_filter_and_list_notifications() {
    assert_eq!(command_action(FILTER_ID, 1), Some(CommandAction::FilterChanged));
    assert_eq!(command_action(LIST_ID, 1), Some(CommandAction::SelectionChanged));
    assert_eq!(command_action(FILTER_ID, 0), None);
    assert_eq!(command_action(LIST_ID, 0), None);
}

#[test]
fn command_action_maps_buttons_only_when_clicked() {
    let ids = [
        RECHECK_ID,
        CLEAR_SELECTED_ID,
        CLEAR_ALL_ID,
        RETRY_ID,
        COPY_SOURCE_ID,
        SHOW_SOURCE_ID,
        COPY_DESTINATION_ID,
        SHOW_DESTINATION_ID,
    ];
    for id in ids {
        assert!(command_action(id, 0).is_some());
        assert_eq!(command_action(id, 1), None);
    }
}

#[test]
fn check_effects_records_in_call_order() {
    let mut effects = CheckEffects::default();
    effects.row_changed(2);
    effects.set_check_status("Checking");
    effects.set_recheck_enabled(false);
    effects.check_completed();
    assert_eq!(
        effects.effects,
        vec![
            CheckEffect::RowChanged(2),
            CheckEffect::CheckStatus("Checking".into()),
            CheckEffect::RecheckEnabled(false),
            CheckEffect::Completed
        ]
    );
}
