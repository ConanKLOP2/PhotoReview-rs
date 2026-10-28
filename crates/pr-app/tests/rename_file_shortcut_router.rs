use pr_app::input::key_id::KeyId;
use pr_app::input::primitives::KeyModifiers;
use pr_app::input::review_command::ReviewCommandType;
use pr_app::input::shortcut_router::{RouteState, ShortcutRouter};
use pr_core::settings::app_settings::AppSettings;

fn route(router: &ShortcutRouter, key: KeyId, modifiers: KeyModifiers, has_photo: bool) -> Option<ReviewCommandType> {
    router
        .try_resolve(key, KeyId::NONE, modifiers, RouteState::new(false, has_photo))
        .map(|command| command.kind)
}

#[test]
fn f2_by_default_resolves_to_rename_file() {
    // C#: RenameFileShortcutRouterTests.F2_ByDefault_ResolvesToRenameFile
    let router = ShortcutRouter::from_settings(&AppSettings::default().shortcuts, &AppSettings::default().actions);
    assert_eq!(
        route(&router, KeyId::F2, KeyModifiers::NONE, true),
        Some(ReviewCommandType::RenameFile)
    );
}

#[test]
fn without_a_photo_nothing_resolves() {
    // C#: RenameFileShortcutRouterTests.WithoutAPhoto_NothingResolves
    let settings = AppSettings::default();
    let router = ShortcutRouter::from_settings(&settings.shortcuts, &settings.actions);
    assert_eq!(route(&router, KeyId::F2, KeyModifiers::NONE, false), None);
}

#[test]
fn with_a_modifier_does_not_rename() {
    // C#: RenameFileShortcutRouterTests.WithAModifier_DoesNotRename
    let settings = AppSettings::default();
    let router = ShortcutRouter::from_settings(&settings.shortcuts, &settings.actions);
    for modifiers in [KeyModifiers::CONTROL, KeyModifiers::SHIFT, KeyModifiers::ALT] {
        assert_ne!(
            route(&router, KeyId::F2, modifiers, true),
            Some(ReviewCommandType::RenameFile)
        );
    }
}

#[test]
fn a_rebind_follows_the_settings_and_clearing_the_key_disables_it() {
    // C#: RenameFileShortcutRouterTests.ARebind_FollowsTheSettings_AndClearingTheKeyDisablesIt
    let mut settings = AppSettings::default();
    settings.shortcuts.rename_file = "F6".into();
    let mut router = ShortcutRouter::from_settings(&settings.shortcuts, &settings.actions);
    assert_eq!(route(&router, KeyId::F2, KeyModifiers::NONE, true), None);
    assert_eq!(
        route(&router, KeyId::F6, KeyModifiers::NONE, true),
        Some(ReviewCommandType::RenameFile)
    );
    settings.shortcuts.rename_file.clear();
    router.rebuild(&settings.shortcuts, &settings.actions);
    assert_eq!(route(&router, KeyId::F6, KeyModifiers::NONE, true), None);
}

#[test]
fn an_action_bound_to_the_same_key_takes_precedence() {
    // C#: RenameFileShortcutRouterTests.AnActionBoundToTheSameKey_TakesPrecedence
    let mut settings = AppSettings::default();
    settings.actions[0].shortcut = "F2".into();
    let router = ShortcutRouter::from_settings(&settings.shortcuts, &settings.actions);
    assert_eq!(
        route(&router, KeyId::F2, KeyModifiers::NONE, true),
        Some(ReviewCommandType::RunAction)
    );
}

#[test]
fn it_does_not_disturb_the_other_keys() {
    // C#: RenameFileShortcutRouterTests.ItDoesNotDisturbTheOtherKeys
    let settings = AppSettings::default();
    let router = ShortcutRouter::from_settings(&settings.shortcuts, &settings.actions);
    assert_eq!(
        route(&router, KeyId::RIGHT, KeyModifiers::NONE, true),
        Some(ReviewCommandType::Next)
    );
    assert_eq!(
        route(&router, KeyId::DELETE, KeyModifiers::NONE, true),
        Some(ReviewCommandType::Recycle)
    );
    assert_eq!(
        route(&router, KeyId::F3, KeyModifiers::NONE, true),
        Some(ReviewCommandType::RunAction)
    );
}

#[test]
fn holding_the_key_does_not_open_the_box_repeatedly() {
    // C#: RenameFileShortcutRouterTests.HoldingTheKey_DoesNotOpenTheBoxRepeatedly
    assert!(ReviewCommandType::RenameFile.ignores_auto_repeat());
}
