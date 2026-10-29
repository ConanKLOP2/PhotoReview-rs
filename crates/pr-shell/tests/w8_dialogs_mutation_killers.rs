//! Standalone logic guards and a fake-factory modal result guard for the W8 dialogs mutation lane.
//! Port of NativeDialogChromePainter, SecondaryWindowPort, RecoveryDialog,
//! BenchmarkDialog and SettingsUi helpers at C# baseline 7eaee2a2.
#![cfg(windows)]

use std::cell::RefCell;
use std::collections::BTreeMap;

use pr_core::abstractions::NullLog;
use pr_platform_win::dialogs::benchmark::results::row_color_key;
use pr_platform_win::dialogs::native::painter::controls::frame_rects;
use pr_platform_win::dialogs::native::painter::frame::{BorderLayer, border_layers};
use pr_platform_win::dialogs::native::painter::geometry::paint_scale;
use pr_platform_win::dialogs::recovery::{actions::command_action, layout::*};
use pr_platform_win::dialogs::secondary_window_port::try_read_recovery_entries;
use pr_platform_win::dialogs::settings::catalog::{PageFactory, PendingSettingsPage, pending_keys_with};
use pr_platform_win::dialogs::settings::dialog::page_layout::bar_line;
use pr_platform_win::dialogs::settings::pages::shortcuts::find_problems;

#[test]
fn paint_scale_low_and_fractional_dpi_preserves_the_csharp_scale() {
    // C#: NativeDialogChromePainter.Draw. geometry.rs:301: / -> %, *, constant return.
    for (dpi, expected) in [(0, 1.0), (48, 1.0), (96, 1.0), (120, 1.25), (144, 1.5), (192, 2.0)] {
        assert_eq!(paint_scale(dpi), expected, "dpi={dpi}");
    }
}

#[test]
fn frame_rects_translated_rectangle_preserves_each_edge_and_thickness() {
    // C#: NativeDialogChromePainter.Frame. controls.rs:261/265/272/276: + and - replacements.
    let rects = frame_rects(7, 11, 43, 59, 3);
    let coords = rects.map(|r| (r.left, r.top, r.right, r.bottom));
    assert_eq!(
        coords,
        [(7, 11, 43, 14), (7, 56, 43, 59), (7, 11, 10, 59), (40, 11, 43, 59)]
    );
}

#[test]
fn border_layers_zero_single_and_client_edges_preserve_outline_order() {
    // C#: NativeDialogChromePainter.PaintBorder. frame.rs:64: <= -> >; :68: && -> ||, > -> ==/<.
    assert!(border_layers(50, 40, 0, true).is_empty());
    assert!(border_layers(50, 40, -1, false).is_empty());
    let outer = BorderLayer { inset: 0, outer: true };
    assert_eq!(border_layers(50, 40, 1, true), [outer]);
    assert_eq!(border_layers(50, 40, 3, false), [outer]);
    assert_eq!(
        border_layers(50, 40, 3, true),
        [
            outer,
            BorderLayer { inset: 1, outer: false },
            BorderLayer { inset: 2, outer: false }
        ]
    );
}

#[test]
fn try_read_recovery_entries_success_and_error_keep_results_distinct() {
    // C#: SecondaryWindowPort.TryReadRecoveryEntries. secondary_window_port.rs:669: Option return replacements.
    let shown = RefCell::new(Vec::new());
    let show = |title: &str, message: &str| shown.borrow_mut().push((title.to_owned(), message.to_owned()));
    assert_eq!(
        try_read_recovery_entries(|| Ok(vec![13_u8, 29]), &NullLog, &show),
        Some(vec![13, 29])
    );
    assert!(shown.borrow().is_empty());
    assert_eq!(
        try_read_recovery_entries::<u8>(|| Err(std::io::Error::other("isolated read failure")), &NullLog, &show),
        None
    );
    let shown = shown.borrow();
    assert_eq!(shown.len(), 1);
    assert!(!shown[0].0.is_empty());
    assert!(shown[0].1.contains("isolated read failure"));
}

#[test]
fn row_color_key_priority_and_negative_row_parity_match_csharp() {
    // C#: BenchmarkDialog.RowColors. benchmark/results.rs:20: constant returns; :24: == -> !=, % -> /,+.
    assert_eq!(row_color_key(true, true, 1), "Dark.SuccessBackground");
    assert_eq!(row_color_key(false, true, 0), "Dark.ErrorBackground");
    for row in [-3, -1, 1, 3] {
        assert_eq!(row_color_key(false, false, row), "Dark.RowAlt");
    }
    for row in [-2, 0, 2, 4] {
        assert_eq!(row_color_key(false, false, row), "Dark.Panel");
    }
}

#[test]
fn command_action_only_expected_notifications_dispatch_recovery_actions() {
    // C#: RecoveryDialog.OnCommand. recovery/actions.rs:154-163: delete match arms; :152: Option return replacements.
    use pr_platform_win::dialogs::recovery::actions::CommandAction as A;
    for (id, notification, action) in [
        (FILTER_ID, 1, A::FilterChanged),
        (LIST_ID, 1, A::SelectionChanged),
        (RECHECK_ID, 0, A::Recheck),
        (CLEAR_SELECTED_ID, 0, A::ClearSelected),
        (CLEAR_ALL_ID, 0, A::ClearAll),
        (RETRY_ID, 0, A::Retry),
        (COPY_SOURCE_ID, 0, A::CopySource),
        (SHOW_SOURCE_ID, 0, A::ShowSource),
        (COPY_DESTINATION_ID, 0, A::CopyDestination),
        (SHOW_DESTINATION_ID, 0, A::ShowDestination),
    ] {
        assert_eq!(command_action(id, notification), Some(action));
        assert_eq!(command_action(id, 1 - notification), None);
        assert_eq!(command_action(id, -1), None);
    }
    assert_eq!(command_action(-1, 0), None);
}

#[test]
fn pending_keys_with_exact_matching_retains_missing_navigation_order() {
    // C#: SettingsPageCatalog.PendingKeys. settings/catalog.rs:81: Vec return replacements; :83: delete !, == -> !=.
    let factory: PageFactory = || Box::new(PendingSettingsPage::new("Known"));
    assert_eq!(
        pending_keys_with(&["Known", "Other", "known", "Other"], &[("Known", factory)]),
        ["Other", "known", "Other"]
    );
    assert!(pending_keys_with(&[], &[("Known", factory)]).is_empty());
}

#[test]
fn find_problems_required_optional_invalid_duplicate_and_unique_keys() {
    // C#: ShortcutsSettingsPage.FindProblems. shortcuts.rs:171: set return replacements; :176: delete !; :190: > -> ==/< />=.
    let texts = BTreeMap::from([
        ("Next".into(), "".into()),
        ("LastImage".into(), "".into()),
        ("Previous".into(), "not a valid key".into()),
        ("FirstImage".into(), "A".into()),
        ("NextFolder".into(), "a".into()),
        ("PreviousFolder".into(), "B".into()),
    ]);
    assert_eq!(
        find_problems(&texts).into_iter().collect::<Vec<_>>(),
        ["FirstImage", "Next", "NextFolder", "Previous"]
    );
    assert!(find_problems(&BTreeMap::new()).is_empty());
}

#[test]
fn bar_line_nonpositive_and_positive_metrics_preserve_minimum_and_step() {
    // C#: SettingsDialog.ScrollByBar. page_layout.rs:36: replace return with 0/1/-1.
    for (height, expected) in [(-7, 1), (0, 1), (1, 2), (16, 32), (23, 46)] {
        assert_eq!(bar_line(height), expected);
    }
}

mod editor_result {
    use std::path::Path;
    use std::rc::Rc;
    use std::sync::Arc;

    use pr_core::abstractions::{BatchReviewItem, NullLog, SettingsTarget, SkippedEntry};
    use pr_core::settings::shortcuts::ReviewAction;
    use pr_platform_win::dialogs::dialog_service::SecondaryWindowPort;
    use pr_platform_win::dialogs::native::dialog::{DialogContent, ID_CANCEL, ID_OK, NativeDialog};
    use pr_platform_win::dialogs::native::template::DialogItem;
    use pr_platform_win::dialogs::secondary_window_port::{
        ActionProfilesOpener, PortOptions, PreparedDialog, SecondaryWindowDialogs, Win32SecondaryWindowPort,
    };
    use pr_platform_win::shell_startup::apartment::StaApartment;
    use pr_test_support::TempRoot;

    struct EmptyPrompt;

    impl DialogContent for EmptyPrompt {
        fn title(&self) -> String {
            "Isolated editor result fixture".into()
        }
        fn items(&self) -> Vec<DialogItem> {
            Vec::new()
        }
        fn preferred_client_size(&self, _: &NativeDialog) -> (i32, i32) {
            (160, 80)
        }
        fn layout_controls(&self, _: &NativeDialog, _: i32, _: i32) {}
    }

    struct Factory {
        picked: String,
    }

    impl SecondaryWindowDialogs for Factory {
        fn settings(&self, _: SettingsTarget, _: ActionProfilesOpener) -> Option<PreparedDialog<bool>> {
            None
        }
        fn recovery(&self, _: isize) -> Option<PreparedDialog<()>> {
            None
        }
        fn benchmark(&self, _: Option<&Path>) -> Option<NativeDialog> {
            None
        }
        fn action_profiles(&self, _: &[ReviewAction]) -> Option<PreparedDialog<Option<Vec<ReviewAction>>>> {
            None
        }
        fn diagnostics(&self) -> Option<PreparedDialog<()>> {
            None
        }
        fn skipped_files(&self, _: &[SkippedEntry]) -> Option<PreparedDialog<()>> {
            None
        }
        fn batch_review(&self, _: &[BatchReviewItem]) -> Option<PreparedDialog<bool>> {
            None
        }
        fn custom_zoom(&self, _: i32) -> Option<PreparedDialog<Option<i32>>> {
            None
        }
        fn pick_executable(&self, _: isize, _: Option<&str>) -> Option<String> {
            Some(self.picked.clone())
        }
        fn text_prompt(&self, _: &str, _: &str, _: &str) -> Option<PreparedDialog<Option<String>>> {
            Some(PreparedDialog::new(
                NativeDialog::new(Rc::new(EmptyPrompt)),
                Box::new(|result| (result == ID_OK).then(|| "Chosen isolated editor".into())),
            ))
        }
    }

    // C#: Win32SecondaryWindowPort.PromptAddExternalEditor.
    // secondary_window_port.rs:474/475: delete field name/path from ExternalEditor { ..new() }.
    #[test]
    #[ignore = "native: opens only a test-owned prompt with fake picker and immediate ready-hook completion"]
    fn prompt_add_external_editor_ok_preserves_name_and_path_cancel_returns_none() {
        let _apartment = StaApartment::enter().unwrap();
        let root = TempRoot::new("w8-dialogs-editor-result");
        let exe = root.join("isolated-editor.exe");
        std::fs::write(&exe, b"fixture without executable version resources").unwrap();
        let picked = exe.to_string_lossy().into_owned();
        for result in [ID_OK, ID_CANCEL] {
            let port = Win32SecondaryWindowPort::new(
                Rc::new(Factory { picked: picked.clone() }),
                Arc::new(NullLog),
                PortOptions {
                    ready: Some(Rc::new(move |dialog| dialog.end_modal(result))),
                    ..Default::default()
                },
            );
            let outcome = port.prompt_add_external_editor(0, None);
            if result == ID_OK {
                let editor = outcome.unwrap();
                assert_eq!(editor.name, "Chosen isolated editor");
                assert_eq!(editor.path, picked);
            } else {
                assert!(outcome.is_none());
            }
            assert_eq!(port.open_count(), 0);
            port.dispose();
        }
    }
}

mod router_editor {
    use std::path::Path;
    use std::rc::Rc;

    use pr_core::abstractions::{BatchReviewItem, SettingsTarget, SkippedEntry};
    use pr_core::settings::external_editors::ExternalEditor;
    use pr_platform_win::dialogs::dialog_service::{SecondaryWindowKind, SecondaryWindowPort};
    use pr_platform_win::dialogs::secondary_window_port::SecondaryWindowRouter;

    struct Native {
        supported: bool,
    }

    impl SecondaryWindowPort for Native {
        fn show_settings(&self, _: isize, _: SettingsTarget) -> bool {
            false
        }
        fn show_recovery(&self, _: isize) {}
        fn show_diagnostics(&self, _: isize) {}
        fn show_benchmark(&self, _: isize, _: Option<&Path>) {}
        fn show_skipped_files(&self, _: isize, _: &[SkippedEntry]) {}
        fn show_batch_review(&self, _: isize, _: &[BatchReviewItem]) -> bool {
            false
        }
        fn prompt_custom_zoom(&self, _: isize, _: i32) -> Option<i32> {
            None
        }
        fn supports(&self, _: SecondaryWindowKind) -> bool {
            self.supported
        }
        fn prompt_add_external_editor(&self, _: isize, _: Option<&Path>) -> Option<ExternalEditor> {
            let mut editor = ExternalEditor::new();
            editor.name = "Routed isolated editor".into();
            editor.path = "isolated-editor.exe".into();
            Some(editor)
        }
    }

    // C#: SecondaryWindowRouter.PromptAddExternalEditor.
    // secondary_window_port.rs:635: replace the router method body with None / Some(Default).
    #[test]
    fn router_prompt_add_external_editor_forwards_supported_and_returns_none_otherwise() {
        let routed = SecondaryWindowRouter::new(Rc::new(Native { supported: true }))
            .prompt_add_external_editor(0, None)
            .unwrap();
        assert_eq!(routed.name, "Routed isolated editor");
        assert_eq!(routed.path, "isolated-editor.exe");
        assert!(
            SecondaryWindowRouter::new(Rc::new(Native { supported: false }))
                .prompt_add_external_editor(0, None)
                .is_none()
        );
    }
}
