//! Port of `tests/PhotoReview.App.Tests/Input/ReviewCommandAutoRepeatTests.cs` (unit W8-05a, contract `contracts/W8-05a.md`).
//!
//! Code under test: `input::review_command` (stub). Red stage: `ignores_auto_repeat`, `ReviewCommand::action`,
//! `move_to_folder` and `copy_to_folder` are `todo!()`, so the tests that call them fail until the porter fills them in.
//! `discriminants_follow_csharp_declaration_order` and the `FULLSCREEN` constant read stub data that is already final.

use pr_app::input::review_command::{ReviewCommand, ReviewCommandType};

/// All 30 kinds in C# declaration order (implicit values 0..=29). Its length is part of the type.
const ALL_KINDS: [ReviewCommandType; 30] = [
    ReviewCommandType::Fullscreen,
    ReviewCommandType::ExitFullscreen,
    ReviewCommandType::Close,
    ReviewCommandType::NextFolder,
    ReviewCommandType::PreviousFolder,
    ReviewCommandType::FirstImage,
    ReviewCommandType::LastImage,
    ReviewCommandType::Undo,
    ReviewCommandType::ToggleCompare,
    ReviewCommandType::RunAction,
    ReviewCommandType::Recycle,
    ReviewCommandType::Skip,
    ReviewCommandType::ToggleFit,
    ReviewCommandType::ZoomIn,
    ReviewCommandType::ZoomOut,
    ReviewCommandType::ZoomActualSize,
    ReviewCommandType::ToggleInfoOverlay,
    ReviewCommandType::Next,
    ReviewCommandType::Previous,
    ReviewCommandType::MoveToFolder,
    ReviewCommandType::CopyToFolder,
    ReviewCommandType::ClickZoom,
    ReviewCommandType::FitWidth,
    ReviewCommandType::FitHeight,
    ReviewCommandType::ToggleKeepZoom,
    ReviewCommandType::OpenFolder,
    ReviewCommandType::CustomZoom,
    ReviewCommandType::ToggleCaptureMember,
    ReviewCommandType::FitWidth2,
    ReviewCommandType::Refresh,
];

/// The 14 kinds for which C# `IgnoresAutoRepeat` is true.
const IGNORE_AUTO_REPEAT: [ReviewCommandType; 14] = [
    ReviewCommandType::Recycle,
    ReviewCommandType::RunAction,
    ReviewCommandType::Undo,
    ReviewCommandType::MoveToFolder,
    ReviewCommandType::CopyToFolder,
    ReviewCommandType::ToggleInfoOverlay,
    ReviewCommandType::Fullscreen,
    ReviewCommandType::ToggleCompare,
    ReviewCommandType::ClickZoom,
    ReviewCommandType::ToggleKeepZoom,
    ReviewCommandType::OpenFolder,
    ReviewCommandType::CustomZoom,
    ReviewCommandType::ToggleCaptureMember,
    ReviewCommandType::Refresh,
];

// C#: ReviewCommandAutoRepeatTests.FileChangingCommandsIgnoreAutoRepeat (10 InlineData rows, one test here)
#[test]
fn file_changing_commands_ignore_auto_repeat() {
    let kinds = [
        ReviewCommandType::Recycle,
        ReviewCommandType::RunAction,
        ReviewCommandType::Undo,
        ReviewCommandType::MoveToFolder,
        ReviewCommandType::CopyToFolder,
        ReviewCommandType::ToggleInfoOverlay,
        ReviewCommandType::Fullscreen,
        ReviewCommandType::ToggleCompare,
        ReviewCommandType::ClickZoom,
        ReviewCommandType::ToggleKeepZoom,
    ];
    for kind in kinds {
        assert!(kind.ignores_auto_repeat(), "{kind:?} must ignore auto-repeat");
    }
}

// C#: ReviewCommandAutoRepeatTests.NavigationCommandsKeepAutoRepeat (13 InlineData rows, one test here)
#[test]
fn navigation_commands_keep_auto_repeat() {
    let kinds = [
        ReviewCommandType::Next,
        ReviewCommandType::Previous,
        ReviewCommandType::ZoomIn,
        ReviewCommandType::ZoomOut,
        ReviewCommandType::FirstImage,
        ReviewCommandType::LastImage,
        ReviewCommandType::ZoomActualSize,
        ReviewCommandType::Skip,
        ReviewCommandType::NextFolder,
        ReviewCommandType::PreviousFolder,
        ReviewCommandType::FitWidth,
        ReviewCommandType::FitWidth2,
        ReviewCommandType::FitHeight,
    ];
    for kind in kinds {
        assert!(!kind.ignores_auto_repeat(), "{kind:?} must keep auto-repeat");
    }
}

// new: the C# method is true for exactly 14 kinds. The two C# tests above do not cover ToggleCaptureMember or
// ExitFullscreen, so every kind is checked here against the full list.
#[test]
fn ignore_auto_repeat_is_exactly_the_fourteen_listed_commands() {
    assert_eq!(IGNORE_AUTO_REPEAT.len(), 14);
    for kind in ALL_KINDS {
        let expected = IGNORE_AUTO_REPEAT.contains(&kind);
        assert_eq!(kind.ignores_auto_repeat(), expected, "{kind:?}");
    }
    assert!(ReviewCommandType::ToggleCaptureMember.ignores_auto_repeat());
    assert!(!ReviewCommandType::ExitFullscreen.ignores_auto_repeat());
}

// new: the discriminants are the C# implicit values, in declaration order. Reordering breaks the shortcut tables.
#[test]
fn discriminants_follow_csharp_declaration_order() {
    for (index, kind) in ALL_KINDS.iter().enumerate() {
        assert_eq!(*kind as u8, index as u8, "{kind:?}");
    }
    assert_eq!(ReviewCommandType::Fullscreen as u8, 0);
    assert_eq!(ReviewCommandType::Refresh as u8, 29);
}

// new: `new(kind)` and the constants use ActionIndex -1, and only `action` sets a real index.
#[test]
fn constructors_use_action_index_minus_one_unless_it_is_an_action() {
    assert_eq!(
        ReviewCommand::FULLSCREEN,
        ReviewCommand {
            kind: ReviewCommandType::Fullscreen,
            action_index: -1,
            force_picker: false,
        },
        "C# ReviewCommand.Fullscreen has ActionIndex -1, ForcePicker false"
    );
    assert_eq!(
        ReviewCommand::new(ReviewCommandType::Undo),
        ReviewCommand::UNDO,
        "constant equals the constructor"
    );

    let run = ReviewCommand::action(3);
    assert_eq!(run.kind, ReviewCommandType::RunAction);
    assert_eq!(run.action_index, 3);
    assert!(!run.force_picker);

    let forced = ReviewCommand::move_to_folder(true);
    assert_eq!(forced.kind, ReviewCommandType::MoveToFolder);
    assert!(forced.force_picker);
    assert_eq!(forced.action_index, -1);

    let copy = ReviewCommand::copy_to_folder(false);
    assert_eq!(copy.kind, ReviewCommandType::CopyToFolder);
    assert_eq!(copy.action_index, -1);
    assert!(!copy.force_picker);

    let forced_copy = ReviewCommand::copy_to_folder(true);
    assert_eq!(forced_copy.kind, ReviewCommandType::CopyToFolder);
    assert!(forced_copy.force_picker);
    assert_eq!(forced_copy.action_index, -1);
}

// new: kills a mutant that drops force_picker in copy_to_folder.
#[test]
fn review_command_constants_map_to_the_right_kind() {
    assert!(ReviewCommand::copy_to_folder(true).force_picker);
    assert!(ReviewCommand::move_to_folder(true).force_picker);
    assert!(!ReviewCommand::move_to_folder(false).force_picker);
}
