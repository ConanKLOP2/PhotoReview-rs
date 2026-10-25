//! Tests of `pr_app::input::shortcut_router` (contract `contracts/W8-05d.md`, precedence steps 1-22, traps T1-T10).
//!
//! Port of `ShortcutRouterTests.cs` (41 methods), `MoveCopyShortcutRouterTests.cs` (7 methods) and
//! `RefreshShortcutRouterTests.cs` (4 methods) at baseline `d92d6aae`, plus the Rust-only tests (`// new:`) and the two
//! golden tests over `tests/golden/shortcut-router.json` (9 600 resolve cases, 4 sequences) and its input
//! `tests/golden-inputs/shortcut-router.json`. Expected values come from the oracle only; this file never copies them into code.
//!
//! Every test fails through `todo!()` until the bodies land. A Theory is one test here: its rows run in a loop, and the
//! assertion message names the row.
//!
//! Adaptation (documented): `ReviewCommand::action_index` is `i32` with `-1` for "none" (the merged W8-05a shape), so the
//! golden `action_index` is compared as is, without mapping `-1` to `None`.

use std::sync::Arc;

use pr_app::input::key_id::KeyId;
use pr_app::input::primitives::KeyModifiers;
use pr_app::input::review_command::{ReviewCommand, ReviewCommandType};
use pr_app::input::shortcut_router::{RouteState, ShortcutRouter};
use pr_app::services::shortcut_key_name::KeyIdNameValidator;
use pr_core::model::enums::FileOperationType;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::normalizer::SettingsValidator;
use pr_core::settings::shortcuts::{ReviewAction, ShortcutMappings};
use pr_test_support::golden;
use serde_json::Value;

// C#: OpenInDefaultEditorShortcutRouterTests.NoDefaultKey_NothingResolves,
// ABoundKey_ResolvesToTheCommand_AndFollowsARebind, HoldingTheKey_DoesNotLaunchTheEditorRepeatedly,
// plus the committed W9-E7 precedence/modifier/state/rebind oracle scenarios.
#[test]
fn golden_open_in_default_editor_shortcut_scenarios_and_auto_repeat_table() {
    use serde_json::json;
    let inputs: Value = serde_json::from_str(include_str!(
        "../../../tests/golden-inputs/open-in-default-editor-shortcut.json"
    ))
    .unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "../../../tests/golden/open-in-default-editor-shortcut.json"
    ))
    .unwrap();
    let scenarios = inputs["scenarios"].as_array().unwrap();
    let cases = expected["cases"].as_array().unwrap();
    assert_eq!(scenarios.len(), 13);
    assert_eq!(cases.len(), 14);
    for (scenario, expected) in scenarios.iter().zip(cases) {
        let mut router = router_from_fixture(&scenario["fixture"]);
        let mut steps = Vec::new();
        for step in scenario["steps"].as_array().unwrap() {
            if step["op"] == "rebuild" {
                let settings = settings_from_fixture(&step["fixture"]);
                router.rebuild(&settings.shortcuts, &settings.actions);
                steps.push(json!({"op":"rebuild","fixture":step["fixture"]["name"]}));
            } else {
                let key = key_named(step["key"].as_str().unwrap());
                let system = key_named(step["system_key"].as_str().unwrap());
                let modifiers = KeyModifiers(u8::try_from(step["modifiers"].as_u64().unwrap()).unwrap());
                let command = router.try_resolve(
                    key,
                    system,
                    modifiers,
                    state_at(&inputs, step["state"].as_i64().unwrap()),
                );
                let command=command.map(|command|json!({"type":format!("{:?}",command.kind),"action_index":command.action_index,"force_picker":command.force_picker}));
                steps.push(json!({"op":"resolve","key":step["key"],"system_key":step["system_key"],"modifiers":step["modifiers"],"state":step["state"],"command":command}));
            }
        }
        assert_eq!(
            json!({"name":scenario["name"],"steps":steps}),
            *expected,
            "{}",
            scenario["name"]
        );
    }
    use ReviewCommandType::*;
    let types = [
        Fullscreen,
        ExitFullscreen,
        Close,
        NextFolder,
        PreviousFolder,
        FirstImage,
        LastImage,
        Undo,
        ToggleCompare,
        RunAction,
        Recycle,
        Skip,
        ToggleFit,
        ZoomIn,
        ZoomOut,
        ZoomActualSize,
        ToggleInfoOverlay,
        Next,
        Previous,
        MoveToFolder,
        CopyToFolder,
        ClickZoom,
        FitWidth,
        FitHeight,
        ToggleKeepZoom,
        OpenFolder,
        CustomZoom,
        ToggleCaptureMember,
        FitWidth2,
        Refresh,
        OpenInDefaultEditor,
        RenameFile,
    ];
    let actual = json!({"kind":"auto_repeat","ignores":types.iter().filter(|kind|kind.ignores_auto_repeat()).map(|kind|format!("{kind:?}")).collect::<Vec<_>>(),"types":types.iter().map(|kind|json!({"name":format!("{kind:?}"),"value":*kind as u8})).collect::<Vec<_>>()});
    assert_eq!(actual, cases[13]);
}

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

/// C# `new ShortcutRouter(settings)`.
fn router_for(settings: &AppSettings) -> ShortcutRouter {
    ShortcutRouter::from_settings(&settings.shortcuts, &settings.actions)
}

/// C# `new ShortcutRouter(new AppSettings())` (the `DefaultRouter()` of the C# fixture).
fn default_router() -> ShortcutRouter {
    router_for(&AppSettings::default())
}

/// C# `TryResolve(key, KeyId.None, modifiers, ...)`: the pressed key is the plain key, not a system key.
fn route(router: &ShortcutRouter, key: KeyId, modifiers: KeyModifiers, state: RouteState) -> Option<ReviewCommand> {
    router.try_resolve(key, KeyId::NONE, modifiers, state)
}

/// The `Type` of an optional command, as the C# assertions compare it (`cmd?.Type`).
fn kind_of(command: Option<ReviewCommand>) -> Option<ReviewCommandType> {
    command.map(|c| c.kind)
}

/// Not fullscreen, has an image, C# defaults for the compare and capture pairs.
fn image() -> RouteState {
    RouteState::new(false, true)
}

/// Not fullscreen, no image.
fn no_image() -> RouteState {
    RouteState::new(false, false)
}

#[test]
fn rename_file_f2_requires_an_image_and_no_modifiers() {
    // C#: ShortcutRouter.TryResolve, FEAT-RENAME step 9b after Recycle.
    let router = default_router();
    assert_eq!(
        route(&router, KeyId::F2, KeyModifiers::NONE, image()),
        Some(ReviewCommand::RENAME_FILE)
    );
    assert_eq!(route(&router, KeyId::F2, KeyModifiers::CONTROL, image()), None);
    assert_eq!(route(&router, KeyId::F2, KeyModifiers::NONE, no_image()), None);
}

/// Compare flags of the C# `TryResolve(..., hasComparePair, isCompareVisible)` overloads, with an image.
fn with_compare(has_compare_pair: bool, is_compare_visible: bool) -> RouteState {
    RouteState {
        has_compare_pair,
        is_compare_visible,
        ..image()
    }
}

/// Capture pair flag of the C# `TryResolve(..., hasCapturePair)` overload, with an image.
fn with_capture(has_capture_pair: bool, is_compare_visible: bool) -> RouteState {
    RouteState {
        has_capture_pair,
        is_compare_visible,
        ..image()
    }
}

fn action(name: &str, shortcut: &str, operation: FileOperationType, destination: &str) -> ReviewAction {
    ReviewAction {
        shortcut: shortcut.to_owned(),
        operation,
        destination: destination.to_owned(),
        ..ReviewAction::new(name)
    }
}

/// The C# fixture of `ShortcutRouterTests`: the listed shortcuts, every other shortcut at its default, two actions.
fn fixture_settings() -> AppSettings {
    AppSettings {
        shortcuts: ShortcutMappings {
            fullscreen: "F11".to_owned(),
            next_folder: "PageDown".to_owned(),
            previous_folder: "PageUp".to_owned(),
            first_image: "Home".to_owned(),
            undo: "Z".to_owned(),
            compare: "C".to_owned(),
            send_to_recycle_bin: "Delete".to_owned(),
            skip: "S".to_owned(),
            toggle_fit: "F".to_owned(),
            zoom_in: "OemPlus".to_owned(),
            zoom_out: "OemMinus".to_owned(),
            next: "Right".to_owned(),
            previous: "Left".to_owned(),
            ..ShortcutMappings::default()
        },
        actions: vec![
            action("Action 1", "D1", FileOperationType::Move, "Sorted"),
            action("Action 2", "D2", FileOperationType::Copy, "Selected"),
        ],
        ..AppSettings::default()
    }
}

/// The C# fixture router of `ShortcutRouterTests` (`_router`).
fn fixture_router() -> ShortcutRouter {
    router_for(&fixture_settings())
}

/// `ReviewAction.Shortcut` of the action at `index`, for the Refresh tests.
fn shortcut_at(settings: &AppSettings, index: i32) -> &str {
    let position = usize::try_from(index).expect("action index is not negative");
    &settings.actions[position].shortcut
}

// ---------------------------------------------------------------------------------------------------------------------
// ShortcutRouterTests.cs (41 methods)
// ---------------------------------------------------------------------------------------------------------------------

// C#: ShortcutRouterTests.Fullscreen_ResolvesToFullscreen (Theory, 2 rows: plain F11; System with SystemKey F11 + Alt)
#[test]
fn fullscreen_resolves_to_fullscreen() {
    let router = fixture_router();
    let rows = [
        (KeyId::F11, KeyId::NONE, KeyModifiers::NONE),
        (KeyId::SYSTEM, KeyId::F11, KeyModifiers::ALT),
    ];
    for (key, system_key, modifiers) in rows {
        let cmd = router.try_resolve(key, system_key, modifiers, no_image());
        assert_eq!(
            kind_of(cmd),
            Some(ReviewCommandType::Fullscreen),
            "key {key:?} system {system_key:?} modifiers {modifiers:?}"
        );
    }
}

// C#: ShortcutRouterTests.Escape_ResolvesByFullscreenState (Theory, 2 rows)
#[test]
fn escape_resolves_by_fullscreen_state() {
    let router = fixture_router();
    let rows = [
        (true, ReviewCommandType::ExitFullscreen),
        (false, ReviewCommandType::Close),
    ];
    for (is_fullscreen, expected) in rows {
        let state = RouteState::new(is_fullscreen, true);
        assert_eq!(
            kind_of(route(&router, KeyId::ESCAPE, KeyModifiers::NONE, state)),
            Some(expected),
            "is_fullscreen {is_fullscreen}"
        );
    }
}

// C#: ShortcutRouterTests.FolderNavigation_ResolvesCorrectly
#[test]
fn folder_navigation_resolves_correctly() {
    let router = fixture_router();
    assert_eq!(
        kind_of(route(&router, KeyId::PAGE_DOWN, KeyModifiers::NONE, no_image())),
        Some(ReviewCommandType::NextFolder)
    );
    assert_eq!(
        kind_of(route(&router, KeyId::PAGE_UP, KeyModifiers::NONE, no_image())),
        Some(ReviewCommandType::PreviousFolder)
    );
}

// C#: ShortcutRouterTests.FirstImage_WhenHasImage_ResolvesToFirstImage
#[test]
fn first_image_when_has_image_resolves_to_first_image() {
    let router = fixture_router();
    assert_eq!(
        kind_of(route(&router, KeyId::HOME, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::FirstImage)
    );
}

// C#: ShortcutRouterTests.FirstImage_WhenNoImage_ReturnsNull
#[test]
fn first_image_when_no_image_returns_null() {
    let router = fixture_router();
    assert_eq!(route(&router, KeyId::HOME, KeyModifiers::NONE, no_image()), None);
}

// C#: ShortcutRouterTests.Undo_RequiresControlModifier
#[test]
fn undo_requires_control_modifier() {
    let router = fixture_router();
    assert_eq!(
        kind_of(route(&router, KeyId::Z, KeyModifiers::CONTROL, no_image())),
        Some(ReviewCommandType::Undo)
    );
    assert_eq!(route(&router, KeyId::Z, KeyModifiers::NONE, no_image()), None);
}

// C#: ShortcutRouterTests.ImageCommands_WhenNoImage_ReturnNull
#[test]
fn image_commands_when_no_image_return_null() {
    let router = fixture_router();
    for key in [KeyId::RIGHT, KeyId::LEFT, KeyId::DELETE, KeyId::S, KeyId::D1] {
        assert_eq!(route(&router, key, KeyModifiers::NONE, no_image()), None, "key {key:?}");
    }
}

// C#: ShortcutRouterTests.Compare_ResolvesToToggleOnlyWhenThereIsSomethingToToggle (Theory, 3 rows)
#[test]
fn compare_resolves_to_toggle_only_when_there_is_something_to_toggle() {
    let router = fixture_router();
    let rows = [
        (false, false, false), // not visible, no pair: nothing to compare
        (true, false, true),   // not visible, has pair: open compare
        (false, true, true),   // visible: always closable, even without a pair
    ];
    for (has_compare_pair, is_compare_visible, resolves) in rows {
        let cmd = route(
            &router,
            KeyId::C,
            KeyModifiers::NONE,
            with_compare(has_compare_pair, is_compare_visible),
        );
        let expected = if resolves {
            Some(ReviewCommandType::ToggleCompare)
        } else {
            None
        };
        assert_eq!(
            kind_of(cmd),
            expected,
            "has_compare_pair {has_compare_pair} is_compare_visible {is_compare_visible}"
        );
    }
}

// C#: ShortcutRouterTests.CustomActions_ResolveToCorrectIndex
#[test]
fn custom_actions_resolve_to_correct_index() {
    let router = fixture_router();

    let action0 = route(&router, KeyId::D1, KeyModifiers::NONE, image()).expect("D1 is action 0");
    assert_eq!(action0.kind, ReviewCommandType::RunAction);
    assert_eq!(action0.action_index, 0);

    let action1 = route(&router, KeyId::D2, KeyModifiers::NONE, image()).expect("D2 is action 1");
    assert_eq!(action1.kind, ReviewCommandType::RunAction);
    assert_eq!(action1.action_index, 1);
}

// C#: ShortcutRouterTests.NavigationAndZoomCommands_ResolveCorrectly
#[test]
fn navigation_and_zoom_commands_resolve_correctly() {
    let router = fixture_router();
    let rows = [
        (KeyId::RIGHT, ReviewCommandType::Next),
        (KeyId::LEFT, ReviewCommandType::Previous),
        (KeyId::DELETE, ReviewCommandType::Recycle),
        (KeyId::S, ReviewCommandType::Skip),
        (KeyId::F, ReviewCommandType::ToggleFit),
        (KeyId::OEM_PLUS, ReviewCommandType::ZoomIn),
        (KeyId::OEM_MINUS, ReviewCommandType::ZoomOut),
    ];
    for (key, expected) in rows {
        assert_eq!(
            kind_of(route(&router, key, KeyModifiers::NONE, image())),
            Some(expected),
            "key {key:?}"
        );
    }
}

// C#: ShortcutRouterTests.LastImage_WhenHasImage_ResolvesToLastImage
#[test]
fn last_image_when_has_image_resolves_to_last_image() {
    let router = default_router();
    assert_eq!(
        kind_of(route(&router, KeyId::END, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::LastImage)
    );
}

// C#: ShortcutRouterTests.LastImage_WhenNoImage_ReturnsNull_LikeFirstImage
#[test]
fn last_image_when_no_image_returns_null_like_first_image() {
    let router = default_router();
    assert_eq!(route(&router, KeyId::END, KeyModifiers::NONE, no_image()), None);
    assert_eq!(route(&router, KeyId::HOME, KeyModifiers::NONE, no_image()), None);
}

// C#: ShortcutRouterTests.ZoomActualSize_WhenHasImage_ResolvesToZoomActualSize
#[test]
fn zoom_actual_size_when_has_image_resolves_to_zoom_actual_size() {
    let router = default_router();
    assert_eq!(
        kind_of(route(&router, KeyId::D1, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::ZoomActualSize)
    );
}

// C#: ShortcutRouterTests.ZoomActualSize_WhenNoImage_ReturnsNull
#[test]
fn zoom_actual_size_when_no_image_returns_null() {
    let router = default_router();
    assert_eq!(route(&router, KeyId::D1, KeyModifiers::NONE, no_image()), None);
}

// C#: ShortcutRouterTests.ClickZoom_WhenHasImage_ResolvesToClickZoom
#[test]
fn click_zoom_when_has_image_resolves_to_click_zoom() {
    let router = default_router();
    assert_eq!(
        kind_of(route(&router, KeyId::D2, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::ClickZoom)
    );
}

// C#: ShortcutRouterTests.ClickZoom_WhenNoImage_ReturnsNull
#[test]
fn click_zoom_when_no_image_returns_null() {
    let router = default_router();
    assert_eq!(route(&router, KeyId::D2, KeyModifiers::NONE, no_image()), None);
}

// C#: ShortcutRouterTests.ToggleInfoOverlay_ResolvesWithOrWithoutImage (Theory, 2 rows)
#[test]
fn toggle_info_overlay_resolves_with_or_without_image() {
    let router = default_router();
    for has_image in [true, false] {
        let state = RouteState::new(false, has_image);
        assert_eq!(
            kind_of(route(&router, KeyId::I, KeyModifiers::NONE, state)),
            Some(ReviewCommandType::ToggleInfoOverlay),
            "has_image {has_image}"
        );
    }
}

// C#: ShortcutRouterTests.OptionalShortcuts_EmptyOrUnparseable_AreDisabled (Theory, 4 rows: "", "   ", "NotAKey", "1")
#[test]
fn optional_shortcuts_empty_or_unparseable_are_disabled() {
    for value in ["", "   ", "NotAKey", "1"] {
        let mut settings = AppSettings::default();
        settings.shortcuts.last_image = value.to_owned();
        settings.shortcuts.zoom_actual_size = value.to_owned();
        settings.shortcuts.toggle_info_overlay = value.to_owned();
        settings.shortcuts.click_zoom = value.to_owned();
        let router = router_for(&settings);

        assert_eq!(
            route(&router, KeyId::END, KeyModifiers::NONE, image()),
            None,
            "value {value:?}"
        );
        assert_eq!(
            route(&router, KeyId::D1, KeyModifiers::NONE, image()),
            None,
            "value {value:?}"
        );
        assert_eq!(
            route(&router, KeyId::I, KeyModifiers::NONE, image()),
            None,
            "value {value:?}"
        );
        assert_eq!(
            route(&router, KeyId::D2, KeyModifiers::NONE, image()),
            None,
            "value {value:?}"
        );
    }
}

// C#: ShortcutRouterTests.OptionalShortcuts_Rebuild_PicksUpCustomKeys
#[test]
fn optional_shortcuts_rebuild_picks_up_custom_keys() {
    let mut settings = AppSettings::default();
    let mut router = router_for(&settings);
    settings.shortcuts.last_image = "F7".to_owned();
    settings.shortcuts.zoom_actual_size = "D0".to_owned();
    settings.shortcuts.toggle_info_overlay = "P".to_owned();
    settings.shortcuts.click_zoom = "D5".to_owned();
    router.rebuild(&settings.shortcuts, &settings.actions);

    assert_eq!(
        kind_of(route(&router, KeyId::F7, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::LastImage)
    );
    assert_eq!(
        kind_of(route(&router, KeyId::D0, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::ZoomActualSize)
    );
    assert_eq!(
        kind_of(route(&router, KeyId::P, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::ToggleInfoOverlay)
    );
    assert_eq!(
        kind_of(route(&router, KeyId::D5, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::ClickZoom)
    );
    assert_eq!(route(&router, KeyId::END, KeyModifiers::NONE, image()), None);
    // The old ClickZoom default is no longer bound.
    assert_eq!(route(&router, KeyId::D2, KeyModifiers::NONE, image()), None);
}

// C#: ShortcutRouterTests.ZoomActualSize_ActionOnTheSameKey_KeepsPriority
#[test]
fn zoom_actual_size_action_on_the_same_key_keeps_priority() {
    // The fixture binds an action to D1 (an old config): actions are resolved before the zoom group.
    let router = fixture_router();
    assert_eq!(
        kind_of(route(&router, KeyId::D1, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::RunAction)
    );
}

// C#: ShortcutRouterTests.ClickZoom_ActionOnTheSameKey_KeepsPriority
#[test]
fn click_zoom_action_on_the_same_key_keeps_priority() {
    // The fixture binds an action to D2 (ClickZoom's default): actions are resolved before the zoom group.
    let router = fixture_router();
    assert_eq!(
        kind_of(route(&router, KeyId::D2, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::RunAction)
    );
}

// C#: ShortcutRouterTests.FitWidth_ResolvesOnlyWithAnImage
#[test]
fn fit_width_resolves_only_with_an_image() {
    let router = fixture_router();
    assert_eq!(
        kind_of(route(&router, KeyId::W, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::FitWidth)
    );
    assert_eq!(route(&router, KeyId::W, KeyModifiers::NONE, no_image()), None);
}

// C#: ShortcutRouterTests.FitWidth2_DefaultKeyD4_ResolvesToItsOwnCommandOnlyWithAnImage
#[test]
fn fit_width2_default_key_d4_resolves_to_its_own_command_only_with_an_image() {
    let router = fixture_router();
    assert_eq!(
        kind_of(route(&router, KeyId::D4, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::FitWidth2)
    );
    assert_eq!(route(&router, KeyId::D4, KeyModifiers::NONE, no_image()), None);
    // The first Fit width key is unaffected.
    assert_eq!(
        kind_of(route(&router, KeyId::W, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::FitWidth)
    );
}

// C#: ShortcutRouterTests.FitWidth2_EmptyShortcut_IsDisabled_AndARemappedKeyFollows
#[test]
fn fit_width2_empty_shortcut_is_disabled_and_a_remapped_key_follows() {
    let mut settings = AppSettings::default();
    settings.shortcuts.fit_width2 = String::new();
    let mut router = router_for(&settings);
    assert_eq!(route(&router, KeyId::D4, KeyModifiers::NONE, image()), None);

    settings.shortcuts.fit_width2 = "Q".to_owned();
    router.rebuild(&settings.shortcuts, &settings.actions);
    assert_eq!(
        kind_of(route(&router, KeyId::Q, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::FitWidth2)
    );
    assert_eq!(route(&router, KeyId::D4, KeyModifiers::NONE, image()), None);
}

// C#: ShortcutRouterTests.FitHeight_ResolvesOnlyWithAnImage
#[test]
fn fit_height_resolves_only_with_an_image() {
    let router = fixture_router();
    assert_eq!(
        kind_of(route(&router, KeyId::H, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::FitHeight)
    );
    assert_eq!(route(&router, KeyId::H, KeyModifiers::NONE, no_image()), None);
}

// C#: ShortcutRouterTests.ToggleKeepZoom_ResolvesWithoutAnImage_LikeToggleInfoOverlay
#[test]
fn toggle_keep_zoom_resolves_without_an_image_like_toggle_info_overlay() {
    let router = fixture_router();
    assert_eq!(
        kind_of(route(&router, KeyId::K, KeyModifiers::NONE, no_image())),
        Some(ReviewCommandType::ToggleKeepZoom)
    );
}

// C#: ShortcutRouterTests.EmptyFitWidthHeightKeepZoomShortcuts_ResolveToNothing
#[test]
fn empty_fit_width_height_keep_zoom_shortcuts_resolve_to_nothing() {
    let mut settings = AppSettings::default();
    let mut router = router_for(&settings);
    settings.shortcuts.fit_width = String::new();
    settings.shortcuts.fit_height = String::new();
    settings.shortcuts.toggle_keep_zoom = String::new();
    router.rebuild(&settings.shortcuts, &settings.actions);

    assert_eq!(route(&router, KeyId::W, KeyModifiers::NONE, image()), None);
    assert_eq!(route(&router, KeyId::H, KeyModifiers::NONE, image()), None);
    assert_eq!(route(&router, KeyId::K, KeyModifiers::NONE, image()), None);
}

// C#: ShortcutRouterTests.OpenFolder_RequiresControlModifier_LikeUndo
#[test]
fn open_folder_requires_control_modifier_like_undo() {
    let router = default_router();
    assert_eq!(
        kind_of(route(&router, KeyId::O, KeyModifiers::CONTROL, no_image())),
        Some(ReviewCommandType::OpenFolder)
    );
    assert_eq!(route(&router, KeyId::O, KeyModifiers::NONE, no_image()), None);
}

// C#: ShortcutRouterTests.OpenFolder_ResolvesWithoutAnImage
#[test]
fn open_folder_resolves_without_an_image() {
    let router = default_router();
    assert_eq!(
        kind_of(route(&router, KeyId::O, KeyModifiers::CONTROL, no_image())),
        Some(ReviewCommandType::OpenFolder)
    );
}

// C#: ShortcutRouterTests.CustomZoom_WhenHasImage_ResolvesToCustomZoom
#[test]
fn custom_zoom_when_has_image_resolves_to_custom_zoom() {
    let router = default_router();
    assert_eq!(
        kind_of(route(&router, KeyId::D3, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::CustomZoom)
    );
}

// C#: ShortcutRouterTests.CustomZoom_WhenNoImage_ReturnsNull
#[test]
fn custom_zoom_when_no_image_returns_null() {
    let router = default_router();
    assert_eq!(route(&router, KeyId::D3, KeyModifiers::NONE, no_image()), None);
}

// C#: ShortcutRouterTests.ToggleCaptureMember_RequiresConfiguredShortcutAndCurrentCapturePair
#[test]
fn toggle_capture_member_requires_configured_shortcut_and_current_capture_pair() {
    let mut settings = AppSettings::default();
    let mut router = router_for(&settings);
    // No shortcut configured by default: nothing resolves.
    assert_eq!(
        route(&router, KeyId::B, KeyModifiers::NONE, with_capture(true, false)),
        None
    );

    settings.shortcuts.toggle_capture_member = "B".to_owned();
    router.rebuild(&settings.shortcuts, &settings.actions);

    assert_eq!(
        route(&router, KeyId::B, KeyModifiers::NONE, with_capture(false, false)),
        None
    );
    assert_eq!(
        kind_of(route(&router, KeyId::B, KeyModifiers::NONE, with_capture(true, false))),
        Some(ReviewCommandType::ToggleCaptureMember)
    );
    assert_eq!(
        route(&router, KeyId::B, KeyModifiers::NONE, with_capture(true, true)),
        None
    );
    assert_eq!(
        route(
            &router,
            KeyId::B,
            KeyModifiers::NONE,
            RouteState {
                has_capture_pair: true,
                ..no_image()
            }
        ),
        None
    );
}

// C#: ShortcutRouterTests.OpenFolderAndCustomZoom_EmptyOrUnparseable_AreDisabled
#[test]
fn open_folder_and_custom_zoom_empty_or_unparseable_are_disabled() {
    let mut settings = AppSettings::default();
    settings.shortcuts.open_folder = String::new();
    settings.shortcuts.custom_zoom = "NotAKey".to_owned();
    let router = router_for(&settings);

    assert_eq!(route(&router, KeyId::O, KeyModifiers::CONTROL, image()), None);
    assert_eq!(route(&router, KeyId::D3, KeyModifiers::NONE, image()), None);
}

// C#: ShortcutRouterTests.OpenFolderAndCustomZoom_DefaultsDoNotCollideWithOtherDefaults
#[test]
fn open_folder_and_custom_zoom_defaults_do_not_collide_with_other_defaults() {
    // Q-R42/Q-R43: OpenFolder's default is Ctrl+O and CustomZoom's is D3, distinct from every other default
    // shortcut, so SettingsValidator sees no duplicate.
    let validator = SettingsValidator::new(Arc::new(KeyIdNameValidator), Arc::new(EnglishSettingsTexts));
    let error = validator.validate_shortcuts(&AppSettings::default());
    assert_eq!(error, None);
}

// C#: ShortcutRouterTests.RecycleAndActions_WithAnyModifier_DoNotResolve (Theory, 5 keys)
// RV-A03 / RV-D3 = A: file-changing commands fire only with NO modifier.
#[test]
fn recycle_and_actions_with_any_modifier_do_not_resolve() {
    let router = default_router();
    let keys = [KeyId::DELETE, KeyId::ENTER, KeyId::F3, KeyId::F4, KeyId::F5];
    let modifier_sets = [
        KeyModifiers::CONTROL,
        KeyModifiers::SHIFT,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ];
    for key in keys {
        assert!(
            route(&router, key, KeyModifiers::NONE, image()).is_some(),
            "key {key:?} bare"
        );
        for modifiers in modifier_sets {
            assert_eq!(
                route(&router, key, modifiers, image()),
                None,
                "key {key:?} modifiers {modifiers:?}"
            );
        }
    }
}

// C#: ShortcutRouterTests.NavigationZoomAndToggles_StillAcceptCtrl
#[test]
fn navigation_zoom_and_toggles_still_accept_ctrl() {
    let router = default_router();
    assert_eq!(
        kind_of(route(&router, KeyId::RIGHT, KeyModifiers::CONTROL, image())),
        Some(ReviewCommandType::Next)
    );
    assert_eq!(
        kind_of(route(&router, KeyId::LEFT, KeyModifiers::CONTROL, image())),
        Some(ReviewCommandType::Previous)
    );
    assert_eq!(
        kind_of(route(&router, KeyId::ADD, KeyModifiers::CONTROL, image())),
        Some(ReviewCommandType::ZoomIn)
    );
    assert_eq!(
        kind_of(route(&router, KeyId::SUBTRACT, KeyModifiers::CONTROL, image())),
        Some(ReviewCommandType::ZoomOut)
    );
    assert_eq!(
        kind_of(route(&router, KeyId::F, KeyModifiers::CONTROL, image())),
        Some(ReviewCommandType::ToggleFit)
    );
}

// C#: ShortcutRouterTests.MoveToFolder_ShiftForcesPicker_CtrlDoesNotResolve
#[test]
fn move_to_folder_shift_forces_picker_ctrl_does_not_resolve() {
    let router = default_router();
    let shifted = route(&router, KeyId::M, KeyModifiers::SHIFT, image()).expect("Shift+M resolves");
    assert_eq!(shifted.kind, ReviewCommandType::MoveToFolder);
    assert!(shifted.force_picker);
    assert_eq!(route(&router, KeyId::M, KeyModifiers::CONTROL, image()), None);
}

// C#: ShortcutRouterTests.AltF4_ReportedAsKeySystem_DoesNotTriggerTheF4Action
#[test]
fn alt_f4_reported_as_key_system_does_not_trigger_the_f4_action() {
    let router = default_router();
    assert_eq!(
        router.try_resolve(KeyId::SYSTEM, KeyId::F4, KeyModifiers::ALT, image()),
        None
    );
}

// C#: ShortcutRouterTests.SameKeyOnActionAndRecycle_ActionWins
#[test]
fn same_key_on_action_and_recycle_action_wins() {
    let mut settings = AppSettings::default();
    settings.shortcuts.send_to_recycle_bin = "D5".to_owned();
    settings.actions = vec![action("A", "D5", FileOperationType::Move, "X")];
    let router = router_for(&settings);

    let cmd = route(&router, KeyId::D5, KeyModifiers::NONE, image()).expect("D5 resolves");
    assert_eq!(cmd.kind, ReviewCommandType::RunAction);
    assert_eq!(cmd.action_index, 0);
}

// C#: ShortcutRouterTests.ImageGroupCommands_WithoutImage_ReturnNull (Theory, 14 keys)
#[test]
fn image_group_commands_without_image_return_null() {
    let router = default_router();
    let keys = [
        KeyId::DELETE,
        KeyId::SPACE,
        KeyId::F,
        KeyId::ADD,
        KeyId::SUBTRACT,
        KeyId::W,
        KeyId::H,
        KeyId::RIGHT,
        KeyId::LEFT,
        KeyId::ENTER,
        KeyId::F5,
        KeyId::M,
        KeyId::Y,
        KeyId::D3,
    ];
    for key in keys {
        assert_eq!(route(&router, key, KeyModifiers::NONE, no_image()), None, "key {key:?}");
    }
}

// C#: ShortcutRouterTests.Compare_NothingToToggle_DoesNotFallThroughToALowerPriorityCommandOnTheSameKey
#[test]
fn compare_nothing_to_toggle_does_not_fall_through_to_a_lower_priority_command_on_the_same_key() {
    let mut settings = AppSettings::default();
    settings.shortcuts.compare = "S".to_owned();
    settings.shortcuts.skip = "S".to_owned();
    let router = router_for(&settings);

    assert_eq!(
        route(&router, KeyId::S, KeyModifiers::NONE, with_compare(false, false)),
        None
    );
    assert_eq!(
        kind_of(route(&router, KeyId::S, KeyModifiers::NONE, with_compare(true, false))),
        Some(ReviewCommandType::ToggleCompare)
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// MoveCopyShortcutRouterTests.cs (7 methods)
// ---------------------------------------------------------------------------------------------------------------------

/// C# `MoveCopyShortcutRouterTests.Resolve(router, key, modifiers, hasImage)`.
fn resolve_move_copy(
    router: &ShortcutRouter,
    key: KeyId,
    modifiers: KeyModifiers,
    has_image: bool,
) -> Option<ReviewCommand> {
    route(router, key, modifiers, RouteState::new(false, has_image))
}

// C#: MoveCopyShortcutRouterTests.DefaultKeys_ResolveWithoutForcingThePicker (Theory, 2 rows: M, Y)
#[test]
fn default_keys_resolve_without_forcing_the_picker() {
    let router = default_router();
    let rows = [
        (KeyId::M, ReviewCommandType::MoveToFolder),
        (KeyId::Y, ReviewCommandType::CopyToFolder),
    ];
    for (key, expected) in rows {
        let cmd = resolve_move_copy(&router, key, KeyModifiers::NONE, true).expect("resolves");
        assert_eq!(cmd.kind, expected, "key {key:?}");
        assert!(!cmd.force_picker, "key {key:?}");
    }
}

// C#: MoveCopyShortcutRouterTests.ShiftKey_ForcesThePicker (Theory, 2 rows: M, Y)
#[test]
fn shift_key_forces_the_picker() {
    let router = default_router();
    let rows = [
        (KeyId::M, ReviewCommandType::MoveToFolder),
        (KeyId::Y, ReviewCommandType::CopyToFolder),
    ];
    for (key, expected) in rows {
        let cmd = resolve_move_copy(&router, key, KeyModifiers::SHIFT, true).expect("resolves");
        assert_eq!(cmd.kind, expected, "key {key:?}");
        assert!(cmd.force_picker, "key {key:?}");
    }
}

// C#: MoveCopyShortcutRouterTests.CtrlKey_IsNotMoveOrCopy (Theory, 2 rows: M, Y)
#[test]
fn ctrl_key_is_not_move_or_copy() {
    let router = default_router();
    for key in [KeyId::M, KeyId::Y] {
        assert_eq!(
            resolve_move_copy(&router, key, KeyModifiers::CONTROL, true),
            None,
            "key {key:?}"
        );
    }
}

// C#: MoveCopyShortcutRouterTests.WithoutImage_Nothing (Theory, 2 rows: M, Y)
#[test]
fn without_image_nothing() {
    let router = default_router();
    for key in [KeyId::M, KeyId::Y] {
        assert_eq!(
            resolve_move_copy(&router, key, KeyModifiers::NONE, false),
            None,
            "key {key:?}"
        );
    }
}

// C#: MoveCopyShortcutRouterTests.EmptyShortcut_DisablesTheCommand (Theory, 2 rows: "", "   ")
#[test]
fn empty_shortcut_disables_the_command() {
    for value in ["", "   "] {
        let mut settings = AppSettings::default();
        settings.shortcuts.move_to_folder = value.to_owned();
        settings.shortcuts.copy_to_folder = value.to_owned();
        let router = router_for(&settings);

        assert_eq!(
            resolve_move_copy(&router, KeyId::M, KeyModifiers::NONE, true),
            None,
            "value {value:?}"
        );
        assert_eq!(
            resolve_move_copy(&router, KeyId::Y, KeyModifiers::NONE, true),
            None,
            "value {value:?}"
        );
        // "Empty" must not become KeyId.None or any other key.
        assert_eq!(
            resolve_move_copy(&router, KeyId::NONE, KeyModifiers::NONE, true),
            None,
            "value {value:?}"
        );
    }
}

// C#: MoveCopyShortcutRouterTests.RemappedKeys_FollowSettingsAndRebuild
#[test]
fn remapped_keys_follow_settings_and_rebuild() {
    let mut settings = AppSettings::default();
    settings.shortcuts.move_to_folder = "F7".to_owned();
    settings.shortcuts.copy_to_folder = "F8".to_owned();
    let mut router = router_for(&settings);

    assert_eq!(
        resolve_move_copy(&router, KeyId::F7, KeyModifiers::NONE, true).map(|c| c.kind),
        Some(ReviewCommandType::MoveToFolder)
    );
    assert_eq!(
        resolve_move_copy(&router, KeyId::F8, KeyModifiers::NONE, true).map(|c| c.kind),
        Some(ReviewCommandType::CopyToFolder)
    );
    assert_eq!(resolve_move_copy(&router, KeyId::M, KeyModifiers::NONE, true), None);

    settings.shortcuts.move_to_folder = String::new();
    router.rebuild(&settings.shortcuts, &settings.actions);
    assert_eq!(resolve_move_copy(&router, KeyId::F7, KeyModifiers::NONE, true), None);
    assert_eq!(
        resolve_move_copy(&router, KeyId::F8, KeyModifiers::NONE, true).map(|c| c.kind),
        Some(ReviewCommandType::CopyToFolder)
    );
}

// C#: MoveCopyShortcutRouterTests.DefaultBindings_KeepResolvingToTheirOwnCommands
#[test]
fn default_bindings_keep_resolving_to_their_own_commands() {
    let router = default_router();
    assert_eq!(
        resolve_move_copy(&router, KeyId::RIGHT, KeyModifiers::NONE, true).map(|c| c.kind),
        Some(ReviewCommandType::Next)
    );
    assert_eq!(
        resolve_move_copy(&router, KeyId::ENTER, KeyModifiers::NONE, true).map(|c| c.kind),
        Some(ReviewCommandType::RunAction)
    );
    assert_eq!(
        resolve_move_copy(&router, KeyId::DELETE, KeyModifiers::NONE, true).map(|c| c.kind),
        Some(ReviewCommandType::Recycle)
    );
    assert_eq!(
        resolve_move_copy(&router, KeyId::Z, KeyModifiers::CONTROL, true).map(|c| c.kind),
        Some(ReviewCommandType::Undo)
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// RefreshShortcutRouterTests.cs (4 methods)
// ---------------------------------------------------------------------------------------------------------------------

// C#: RefreshShortcutRouterTests.Refresh_DefaultKeyR_ResolvesOnlyWithAnImage
#[test]
fn refresh_default_key_r_resolves_only_with_an_image() {
    let router = default_router();
    assert_eq!(
        kind_of(route(&router, KeyId::R, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::Refresh)
    );
    assert_eq!(route(&router, KeyId::R, KeyModifiers::NONE, no_image()), None);
}

// C#: RefreshShortcutRouterTests.Refresh_F5StillRunsTheDefaultBackupAction
#[test]
fn refresh_f5_still_runs_the_default_backup_action() {
    let settings = AppSettings::default();
    let router = router_for(&settings);

    let command = route(&router, KeyId::F5, KeyModifiers::NONE, image()).expect("F5 resolves");

    assert_eq!(command.kind, ReviewCommandType::RunAction);
    assert_eq!(shortcut_at(&settings, command.action_index), "F5");
}

// C#: RefreshShortcutRouterTests.Refresh_EmptyShortcut_IsDisabled_AndARemappedKeyFollows
#[test]
fn refresh_empty_shortcut_is_disabled_and_a_remapped_key_follows() {
    let mut settings = AppSettings::default();
    settings.shortcuts.refresh = String::new();
    let mut router = router_for(&settings);
    assert_eq!(route(&router, KeyId::R, KeyModifiers::NONE, image()), None);

    settings.actions.retain(|a| a.shortcut != "F5");
    settings.shortcuts.refresh = "F5".to_owned();
    router.rebuild(&settings.shortcuts, &settings.actions);

    assert_eq!(
        kind_of(route(&router, KeyId::F5, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::Refresh)
    );
    assert_eq!(route(&router, KeyId::R, KeyModifiers::NONE, image()), None);
}

// C#: RefreshShortcutRouterTests.Refresh_IgnoresAutoRepeat
#[test]
fn refresh_ignores_auto_repeat() {
    assert!(ReviewCommandType::Refresh.ignores_auto_repeat());
}

// ---------------------------------------------------------------------------------------------------------------------
// Rust-only tests (`// new:`)
// ---------------------------------------------------------------------------------------------------------------------

// new: an empty router (C# `new ShortcutRouter()`) still closes the window on Escape, and binds nothing else.
#[test]
fn escape_closes_with_an_empty_router() {
    let router = ShortcutRouter::new();
    assert_eq!(
        kind_of(route(
            &router,
            KeyId::ESCAPE,
            KeyModifiers::NONE,
            RouteState::new(false, false)
        )),
        Some(ReviewCommandType::Close)
    );
    assert_eq!(
        kind_of(route(
            &router,
            KeyId::ESCAPE,
            KeyModifiers::NONE,
            RouteState::new(true, false)
        )),
        Some(ReviewCommandType::ExitFullscreen)
    );
    assert_eq!(route(&router, KeyId::RIGHT, KeyModifiers::NONE, image()), None);
}

// new: reserved key names (settings `Escape`, `Tab`, `System`, `None`) never bind (trap T7, fixture `collisions`).
#[test]
fn reserved_names_never_bind() {
    let mut settings = AppSettings::default();
    settings.shortcuts.toggle_info_overlay = "Escape".to_owned();
    settings.shortcuts.skip = "Tab".to_owned();
    settings.shortcuts.next = "System".to_owned();
    settings.shortcuts.previous = "None".to_owned();
    let router = router_for(&settings);

    // Escape keeps its fixed rule (step 2): the disabled ToggleInfoOverlay binding does not take it.
    assert_eq!(
        kind_of(route(&router, KeyId::ESCAPE, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::Close)
    );
    assert_eq!(route(&router, KeyId::TAB, KeyModifiers::NONE, image()), None);
    assert_eq!(route(&router, KeyId::SYSTEM, KeyModifiers::NONE, image()), None);
    assert_eq!(route(&router, KeyId::NONE, KeyModifiers::NONE, image()), None);
}

// new: `rebuild` drops every stale binding and replaces the action list (trap T6, sequence `rebuild-clears-stale-bindings`).
#[test]
fn rebuild_drops_every_stale_binding_and_action() {
    let mut router = router_for(&fixture_settings());
    assert_eq!(
        kind_of(route(&router, KeyId::D1, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::RunAction)
    );

    let mut next = AppSettings::default();
    next.shortcuts.undo = String::new();
    next.shortcuts.next = String::new();
    next.actions = vec![action("Only", "Q", FileOperationType::Move, "Only")];
    router.rebuild(&next.shortcuts, &next.actions);

    // The old action keys fall back to the defaults of their own commands; no action survives.
    assert_eq!(
        kind_of(route(&router, KeyId::D1, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::ZoomActualSize)
    );
    assert_eq!(
        kind_of(route(&router, KeyId::D2, KeyModifiers::NONE, image())),
        Some(ReviewCommandType::ClickZoom)
    );
    assert_eq!(route(&router, KeyId::Z, KeyModifiers::CONTROL, image()), None);
    assert_eq!(route(&router, KeyId::RIGHT, KeyModifiers::NONE, image()), None);

    // The new action list replaces the old one (not appended): `Only` is action index 0.
    let cmd = route(&router, KeyId::Q, KeyModifiers::NONE, image()).expect("Q resolves to the new action");
    assert_eq!(cmd.kind, ReviewCommandType::RunAction);
    assert_eq!(cmd.action_index, 0);
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden tests (`tests/golden/shortcut-router.json`, inputs in `tests/golden-inputs/shortcut-router.json`)
// ---------------------------------------------------------------------------------------------------------------------

/// The golden input file (fixtures, states, key pairs, sequences). Read-only.
fn golden_inputs() -> Value {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("shortcut-router.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// The fixture object with this name.
fn fixture_named<'a>(inputs: &'a Value, name: &str) -> &'a Value {
    inputs["fixtures"]
        .as_array()
        .expect("fixtures is an array")
        .iter()
        .find(|f| f["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("no fixture named {name:?}"))
}

/// `RouteState` of the golden input state at `index`.
fn state_at(inputs: &Value, index: i64) -> RouteState {
    let states = inputs["states"].as_array().expect("states is an array");
    let position = usize::try_from(index).expect("state index is not negative");
    let state = &states[position];
    RouteState {
        is_fullscreen: golden::bool_of(state, "is_fullscreen"),
        has_image: golden::bool_of(state, "has_image"),
        has_compare_pair: golden::bool_of(state, "has_compare_pair"),
        is_compare_visible: golden::bool_of(state, "is_compare_visible"),
        has_capture_pair: golden::bool_of(state, "has_capture_pair"),
    }
}

/// A key name of the golden (KeyId enum name, aliases included, e.g. `Return`).
fn key_named(name: &str) -> KeyId {
    KeyId::from_name(name).unwrap_or_else(|| panic!("golden key {name:?} is not a KeyId name"))
}

/// The settings of a golden fixture: C# `new AppSettings()`, then the named shortcuts, then the actions if listed.
fn settings_from_fixture(fixture: &Value) -> AppSettings {
    let mut settings = AppSettings::default();
    if let Some(shortcuts) = fixture.get("shortcuts").and_then(Value::as_object) {
        for (property, value) in shortcuts {
            let text = value
                .as_str()
                .unwrap_or_else(|| panic!("shortcut {property} is not text"));
            let slot = settings
                .shortcuts
                .get_mut(property)
                .unwrap_or_else(|| panic!("unknown shortcut property {property:?}"));
            *slot = text.to_owned();
        }
    }
    if let Some(actions) = fixture.get("actions").and_then(Value::as_array) {
        settings.actions = actions
            .iter()
            .map(|a| {
                let operation = match golden::str_of(a, "operation") {
                    "Move" => FileOperationType::Move,
                    "Copy" => FileOperationType::Copy,
                    other => panic!("unknown operation {other:?}"),
                };
                ReviewAction {
                    shortcut: golden::str_of(a, "shortcut").to_owned(),
                    operation,
                    destination: golden::str_of(a, "destination").to_owned(),
                    ..ReviewAction::new(golden::str_of(a, "name"))
                }
            })
            .collect();
    }
    settings
}

/// The router of a golden fixture: `new ShortcutRouter()` for the `empty` fixture, else `new ShortcutRouter(settings)`.
fn router_from_fixture(fixture: &Value) -> ShortcutRouter {
    if fixture.get("router").and_then(Value::as_str) == Some("empty") {
        return ShortcutRouter::new();
    }
    router_for(&settings_from_fixture(fixture))
}

/// Compares one golden `command` (null, or `{type, action_index, force_picker}`) with the router output.
fn assert_command(expected: Option<&Value>, actual: Option<ReviewCommand>, label: &str) {
    match expected {
        None | Some(Value::Null) => assert!(actual.is_none(), "{label}: expected no command, got {actual:?}"),
        Some(expected) => {
            let cmd = actual.unwrap_or_else(|| panic!("{label}: expected {expected}, got no command"));
            assert_eq!(
                format!("{:?}", cmd.kind),
                golden::str_of(expected, "type"),
                "{label}: type"
            );
            assert_eq!(
                i64::from(cmd.action_index),
                golden::i64_of(expected, "action_index"),
                "{label}: action_index"
            );
            assert_eq!(
                cmd.force_picker,
                golden::bool_of(expected, "force_picker"),
                "{label}: force_picker"
            );
        }
    }
}

// new: golden_shortcut_router_resolve (oracle `W805dShortcutRouterCases`, kind `resolve`: 5 fixtures x 40 key pairs x 6
// modifier masks x 8 states = 9 600 cases). Every resolve case is run; the count is asserted.
#[test]
fn golden_shortcut_router_resolve() {
    let inputs = golden_inputs();
    let routers: Vec<(String, ShortcutRouter)> = inputs["fixtures"]
        .as_array()
        .expect("fixtures is an array")
        .iter()
        .map(|f| (golden::str_of(f, "name").to_owned(), router_from_fixture(f)))
        .collect();

    let loaded = golden::load("shortcut-router");
    let mut resolved = 0_usize;
    for case in loaded.cases.iter().filter(|c| golden::str_of(c, "kind") == "resolve") {
        let fixture = golden::str_of(case, "fixture");
        let (_, router) = routers
            .iter()
            .find(|(name, _)| name == fixture)
            .unwrap_or_else(|| panic!("no router for fixture {fixture:?}"));
        let key = key_named(golden::str_of(case, "key"));
        let system_key = key_named(golden::str_of(case, "system_key"));
        let modifiers =
            KeyModifiers(u8::try_from(golden::i64_of(case, "modifiers")).expect("modifier bits fit in a byte"));
        let state_index = golden::i64_of(case, "state");
        let state = state_at(&inputs, state_index);

        let actual = router.try_resolve(key, system_key, modifiers, state);
        let label = format!(
            "fixture {fixture} key {} system {} modifiers {} state {state_index}",
            golden::str_of(case, "key"),
            golden::str_of(case, "system_key"),
            golden::i64_of(case, "modifiers"),
        );
        assert_command(case.get("command"), actual, &label);
        resolved += 1;
    }
    assert_eq!(resolved, 9_600, "the committed golden has 9 600 resolve cases");
}

// new: golden_shortcut_router_sequence (oracle kind `sequence`, 4 cases: resolve steps and rebuild steps in order).
#[test]
fn golden_shortcut_router_sequence() {
    let inputs = golden_inputs();
    let loaded = golden::load("shortcut-router");
    let sequences: Vec<&Value> = loaded
        .cases
        .iter()
        .filter(|c| golden::str_of(c, "kind") == "sequence")
        .collect();
    assert_eq!(sequences.len(), 4, "the committed golden has 4 sequence cases");

    for sequence in sequences {
        let name = golden::str_of(sequence, "name");
        let start = golden::str_of(sequence, "start");
        let mut router = router_from_fixture(fixture_named(&inputs, start));
        let steps = sequence["steps"].as_array().expect("steps is an array");
        for (index, step) in steps.iter().enumerate() {
            let label = format!("{name} step {index}");
            match golden::str_of(step, "op") {
                "resolve" => {
                    let key = key_named(golden::str_of(step, "key"));
                    let system_key = key_named(golden::str_of(step, "system_key"));
                    let modifiers = KeyModifiers(
                        u8::try_from(golden::i64_of(step, "modifiers")).expect("modifier bits fit in a byte"),
                    );
                    let state = state_at(&inputs, golden::i64_of(step, "state"));
                    let actual = router.try_resolve(key, system_key, modifiers, state);
                    assert_command(step.get("command"), actual, &label);
                }
                "rebuild" => {
                    let fixture = fixture_named(&inputs, golden::str_of(step, "fixture"));
                    let settings = settings_from_fixture(fixture);
                    router.rebuild(&settings.shortcuts, &settings.actions);
                }
                other => panic!("{label}: unknown op {other:?}"),
            }
        }
    }
}
