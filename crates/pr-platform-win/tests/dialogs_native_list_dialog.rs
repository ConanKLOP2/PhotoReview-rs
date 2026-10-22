use pr_platform_win::dialogs::native::list_dialog::{CANCEL_ID, CONFIRM_ID, HEADING_ID, INTRO_ID, LIST_ID};

// C#: ListDialog.HeadingId / IntroId / ListId / ConfirmId / CancelId
#[test]
fn list_dialog_control_ids_are_stable_and_non_colliding() {
    assert_eq!(HEADING_ID, 300);
    assert_eq!(INTRO_ID, 301);
    assert_eq!(LIST_ID, 302);
    assert_eq!(CONFIRM_ID, 1);
    assert_eq!(CANCEL_ID, 2);
    assert_ne!(HEADING_ID, CONFIRM_ID);
    assert_ne!(INTRO_ID, CANCEL_ID);
    assert_ne!(LIST_ID, CONFIRM_ID);
}
