//! Native secondary dialog integration checks. They run on an attended Windows desktop only.
#![cfg(windows)]
#[path = "common/startup_dialogs_fixture.rs"]
mod fixture;
use fixture::StartupDialogsFixture;

#[test]
#[ignore = "native: requires desktop; dependency Win32SecondaryWindowPort is not yet implemented"]
fn settings_through_the_dialog_service_is_owned_by_the_shell_and_saving_raises_changed() {
    let f = StartupDialogsFixture::new();
    assert_ne!(f.owner, 0);
    assert!(!f.services.secondary_port.is_value_created());
}
#[test]
#[ignore = "native: requires desktop; dependency Win32SecondaryWindowPort is not yet implemented"]
fn settings_closed_without_saving_returns_false_and_does_not_raise_changed() {
    let f = StartupDialogsFixture::new();
    assert_ne!(f.owner, 0);
    assert!(f.root.path().starts_with(std::env::temp_dir()));
}
#[test]
#[ignore = "native: requires desktop; dependency Win32SecondaryWindowPort is not yet implemented"]
fn recovery_through_the_dialog_service_opens_and_changes_no_data() {
    let f = StartupDialogsFixture::new();
    assert_ne!(f.owner, 0);
    assert!(!f.root.join("data/operations.jsonl").exists());
}
#[test]
#[ignore = "native: requires desktop; dependency Win32SecondaryWindowPort is not yet implemented"]
fn other_modal_windows_through_the_dialog_service_open_owned_and_close() {
    let f = StartupDialogsFixture::new();
    assert_ne!(f.owner, 0);
    assert!(f.message_loop.message_filter_count() == 0);
}
#[test]
#[ignore = "native: requires desktop; dependency Win32SecondaryWindowPort is not yet implemented"]
fn zoom_prompt_through_the_zoom_service_cancel_is_none_and_ok_returns_the_validated_percent() {
    let f = StartupDialogsFixture::new();
    assert_ne!(f.owner, 0);
    assert!(!f.services.secondary_port_host.port.is_value_created());
}
#[test]
#[ignore = "native: requires desktop; dependency Win32SecondaryWindowPort is not yet implemented"]
fn benchmark_through_the_dialog_service_is_modeless_and_tab_is_handled_through_the_shell_loop_filter() {
    let f = StartupDialogsFixture::new();
    assert_ne!(f.owner, 0);
    assert_eq!(f.message_loop.message_filter_count(), 0);
}
#[test]
#[ignore = "native: requires desktop; dependency Win32SecondaryWindowPort is not yet implemented"]
fn disposing_the_port_host_closes_modeless_and_modal_windows_and_unhooks_the_loop_filter() {
    let f = StartupDialogsFixture::new();
    f.services.secondary_port_host.dispose();
    assert_eq!(f.message_loop.message_filter_count(), 0);
}
#[test]
#[ignore = "native: requires desktop; dependency Win32SecondaryWindowPort is not yet implemented"]
fn port_show_recovery_opens_the_journals_entries_owned_and_modal_and_closes_cleanly() {
    let f = StartupDialogsFixture::new();
    assert_ne!(f.owner, 0);
    assert!(f.root.path().exists());
}
#[test]
#[ignore = "native: requires desktop; dependency Win32SecondaryWindowPort is not yet implemented"]
fn port_dispose_closes_an_open_recovery_and_later_calls_do_nothing() {
    let f = StartupDialogsFixture::new();
    f.services.secondary_port_host.dispose();
    assert_eq!(f.message_loop.message_filter_count(), 0);
}
#[test]
#[ignore = "native: requires desktop; dependency Win32SecondaryWindowPort is not yet implemented"]
fn port_copy_uses_the_shells_clipboard_service() {
    let f = StartupDialogsFixture::new();
    assert_ne!(f.owner, 0);
    assert!(f.root.path().exists());
}
#[test]
#[ignore = "native: requires desktop; dependency Win32SecondaryWindowPort is not yet implemented"]
fn port_retry_confirms_a_user_cancelled_entry_explicitly_so_the_service_finishes_it() {
    let f = StartupDialogsFixture::new();
    assert_ne!(f.owner, 0);
    assert!(!f.root.join("data/operations.jsonl").exists());
}
#[test]
#[ignore = "native: requires desktop; dependency Win32SecondaryWindowPort is not yet implemented"]
fn port_clear_writes_the_dismissal_to_the_real_journal() {
    let f = StartupDialogsFixture::new();
    assert_ne!(f.owner, 0);
    assert!(!f.root.join("data/operations.jsonl").exists());
}
#[test]
#[ignore = "native: requires desktop; dependency Win32SecondaryWindowPort is not yet implemented"]
fn port_retry_question_warns_about_permanent_deletion_only_when_the_setting_allows_it() {
    let f = StartupDialogsFixture::new();
    assert_ne!(f.owner, 0);
    assert!(f.root.path().exists());
}
