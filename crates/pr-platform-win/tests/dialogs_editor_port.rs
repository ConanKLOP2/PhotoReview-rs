//! W9-E8 source-derived routing tests, C# Win32SecondaryWindowPort.cs at7eaee2a2.
#[path = "common/dialogs_dialog_fixture.rs"]
mod native;
use pr_core::{
    abstractions::{BatchReviewItem, NullLog, SettingsTarget, SkippedEntry},
    settings::{external_editors::ExternalEditor, shortcuts::ReviewAction},
};
use pr_platform_win::{
    dialogs::{
        dialog_service::{SecondaryWindowKind, SecondaryWindowPort},
        native::{dialog::NativeDialog, external_editors_dialog::ExternalEditorsEdit, list_dialog::ListDialogContent},
        secondary_window_port::{
            ActionProfilesOpener, ExternalEditorsOpener, PortOptions, PreparedDialog, SecondaryWindowDialogs,
            SecondaryWindowRouter, Win32SecondaryWindowPort,
        },
    },
    win32::structs::Hwnd,
};
use std::{
    cell::{Cell, RefCell},
    path::Path,
    rc::Rc,
    sync::Arc,
};

type EditorCall = (Vec<ExternalEditor>, Option<String>);
#[derive(Default)]
struct Factory {
    prepare: Cell<bool>,
    name: RefCell<Option<String>>,
    selected: Cell<Option<usize>>,
    picked: RefCell<Option<String>>,
    picks: RefCell<Vec<(Hwnd, Option<String>)>>,
    prompts: RefCell<Vec<(String, String, String)>>,
    choices: RefCell<Vec<ListDialogContent>>,
    editors: RefCell<Vec<EditorCall>>,
    opener: RefCell<Option<ExternalEditorsOpener>>,
    old_settings: Cell<usize>,
}
impl SecondaryWindowDialogs for Factory {
    fn settings(&self, _: SettingsTarget, _: ActionProfilesOpener) -> Option<PreparedDialog<bool>> {
        self.old_settings.set(self.old_settings.get() + 1);
        None
    }
    fn settings_with_editors(
        &self,
        _: SettingsTarget,
        _: ActionProfilesOpener,
        opener: ExternalEditorsOpener,
    ) -> Option<PreparedDialog<bool>> {
        *self.opener.borrow_mut() = Some(opener);
        None
    }
    fn recovery(&self, _: Hwnd) -> Option<PreparedDialog<()>> {
        None
    }
    fn diagnostics(&self) -> Option<PreparedDialog<()>> {
        None
    }
    fn benchmark(&self, _: Option<&Path>) -> Option<NativeDialog> {
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
    fn action_profiles(&self, _: &[ReviewAction]) -> Option<PreparedDialog<Option<Vec<ReviewAction>>>> {
        None
    }
    fn external_editors(
        &self,
        editors: &[ExternalEditor],
        default: Option<&str>,
    ) -> Option<PreparedDialog<Option<ExternalEditorsEdit>>> {
        self.editors
            .borrow_mut()
            .push((editors.to_vec(), default.map(str::to_owned)));
        if !self.prepare.get() {
            return None;
        }
        let result = ExternalEditorsEdit {
            editors: editors.to_vec(),
            default_id: default.unwrap_or_default().into(),
        };
        Some(PreparedDialog::new(
            test_dialog(),
            Box::new(move |id| (id == 1).then_some(result)),
        ))
    }
    fn pick_executable(&self, owner: Hwnd, initial: Option<&str>) -> Option<String> {
        self.picks.borrow_mut().push((owner, initial.map(str::to_owned)));
        self.picked.borrow().clone()
    }
    fn text_prompt(&self, title: &str, label: &str, initial: &str) -> Option<PreparedDialog<Option<String>>> {
        self.prompts
            .borrow_mut()
            .push((title.into(), label.into(), initial.into()));
        if !self.prepare.get() {
            return None;
        }
        let result = self.name.borrow().clone();
        Some(PreparedDialog::new(
            test_dialog(),
            Box::new(move |id| if id == 1 { result } else { None }),
        ))
    }
    fn choice_list(&self, content: ListDialogContent) -> Option<PreparedDialog<Option<usize>>> {
        self.choices.borrow_mut().push(content);
        if !self.prepare.get() {
            return None;
        }
        let result = self.selected.get();
        Some(PreparedDialog::new(
            test_dialog(),
            Box::new(move |id| if id == 1 { result } else { None }),
        ))
    }
}

struct Content;
impl pr_platform_win::dialogs::native::dialog::DialogContent for Content {
    fn title(&self) -> String {
        "E8 fake dialog".into()
    }
    fn items(&self) -> Vec<pr_platform_win::dialogs::native::template::DialogItem> {
        Vec::new()
    }
    fn preferred_client_size(&self, _: &NativeDialog) -> (i32, i32) {
        (160, 80)
    }
    fn layout_controls(&self, _: &NativeDialog, _: i32, _: i32) {}
}
fn test_dialog() -> NativeDialog {
    NativeDialog::new(Rc::new(Content))
}
fn modal_port(factory: Rc<Factory>, result: i32) -> Rc<Win32SecondaryWindowPort> {
    Win32SecondaryWindowPort::new(
        factory,
        Arc::new(NullLog),
        PortOptions {
            ready: Some(Rc::new(move |dialog| dialog.end_modal(result))),
            ..Default::default()
        },
    )
}

// Source-derived fake-factory results through the real modal run/finish path.
#[test]
#[ignore = "native: creates test-owned modal windows"]
fn editor_manager_modal_apply_cancel_and_dispose_results() {
    native::on_sta(|| {
        for result in [1, 2] {
            let factory = Rc::new(Factory::default());
            factory.prepare.set(true);
            let port = modal_port(factory.clone(), result);
            let outcome = port.show_external_editors(0, &editors(), Some("b"));
            if result == 1 {
                let outcome = outcome.unwrap();
                assert_eq!(outcome.editors, editors());
                assert_eq!(outcome.default_id, "b");
            } else {
                assert!(outcome.is_none());
            }
            assert_eq!(port.open_count(), 0);
            port.dispose();
            assert!(port.show_external_editors(0, &editors(), None).is_none());
            assert_eq!(factory.editors.borrow().len(), 1);
        }
    });
}

#[test]
#[ignore = "native: creates test-owned modal windows"]
fn add_editor_modal_ok_returns_fresh_editor_and_cancel_returns_none() {
    native::on_sta(|| {
        for result in [1, 2] {
            let factory = Rc::new(Factory::default());
            factory.prepare.set(true);
            *factory.picked.borrow_mut() = Some(r"C:\Missing\CaptureOne.exe".into());
            *factory.name.borrow_mut() = Some("Chosen editor".into());
            let port = modal_port(factory, result);
            let outcome = port.prompt_add_external_editor(0, None);
            if result == 1 {
                let outcome = outcome.unwrap();
                assert_eq!(outcome.name, "Chosen editor");
                assert_eq!(outcome.path, r"C:\Missing\CaptureOne.exe");
                assert_eq!(outcome.id.len(), 32);
                assert_eq!(outcome.arguments, ExternalEditor::DEFAULT_ARGUMENTS);
            } else {
                assert!(outcome.is_none());
            }
            assert_eq!(port.open_count(), 0);
        }
    });
}

#[test]
#[ignore = "native: creates test-owned modal windows"]
fn default_editor_modal_valid_invalid_and_cancel_results() {
    native::on_sta(|| {
        for (result, index, expected) in [
            (1, Some(0), Some("a")),
            (1, Some(1), Some("b")),
            (1, Some(2), None),
            (1, None, None),
            (2, Some(1), None),
        ] {
            let factory = Rc::new(Factory::default());
            factory.prepare.set(true);
            factory.selected.set(index);
            let port = modal_port(factory, result);
            assert_eq!(
                port.prompt_default_external_editor(0, &editors(), Some("b")).as_deref(),
                expected
            );
            assert_eq!(port.open_count(), 0);
        }
    });
}
fn port(factory: Rc<Factory>) -> Rc<Win32SecondaryWindowPort> {
    Win32SecondaryWindowPort::new(factory, Arc::new(NullLog), PortOptions::default())
}
fn editors() -> Vec<ExternalEditor> {
    vec![
        ExternalEditor {
            id: "a".into(),
            name: "Alpha".into(),
            path: r"C:\a.exe".into(),
            ..ExternalEditor::default()
        },
        ExternalEditor {
            id: "b".into(),
            name: "Beta".into(),
            path: r"C:\b.exe".into(),
            ..ExternalEditor::default()
        },
    ]
}

// C#: SecondaryWindowRoutingTests.Win32Port_Supports_ExactlyTheNativeWindows (new kind).
#[test]
fn external_editor_support_and_router_forward_all_three_members() {
    let factory = Rc::new(Factory::default());
    let native = port(factory.clone());
    assert!(native.supports(SecondaryWindowKind::ExternalEditors));
    let router = SecondaryWindowRouter::new(native);
    assert!(router.show_external_editors(17, &editors(), Some("b")).is_none());
    assert!(
        router
            .prompt_add_external_editor(18, Some(Path::new(r"C:\initial")))
            .is_none()
    );
    assert!(
        router
            .prompt_default_external_editor(19, &editors(), Some("b"))
            .is_none()
    );
    assert_eq!(factory.editors.borrow()[0], (editors(), Some("b".into())));
    assert_eq!(factory.picks.borrow()[0], (18, Some(r"C:\initial".into())));
    assert_eq!(factory.choices.borrow()[0].initial_selected_index, 1);
}

// Source-derived Add guards and product-name fallback; no dialog is prepared by this fake.
#[test]
fn add_missing_or_blank_picker_never_prompts_and_picked_path_supplies_default_name() {
    let factory = Rc::new(Factory::default());
    let port = port(factory.clone());
    for answer in [None, Some(" ".into()), Some("".into())] {
        *factory.picked.borrow_mut() = answer;
        assert!(port.prompt_add_external_editor(23, None).is_none());
    }
    assert!(factory.prompts.borrow().is_empty());
    *factory.picked.borrow_mut() = Some(r"C:\Missing\CaptureOne.exe".into());
    assert!(port.prompt_add_external_editor(23, None).is_none());
    let prompts = factory.prompts.borrow();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].2, "CaptureOne");
    assert!(!prompts[0].0.is_empty() && !prompts[0].1.is_empty());
}

// Source-derived Default selection rules and disposed factory neutrality.
#[test]
fn default_empty_skips_factory_unknown_id_selects_first_and_disposed_calls_do_nothing() {
    let factory = Rc::new(Factory::default());
    let port = port(factory.clone());
    assert!(port.prompt_default_external_editor(0, &[], None).is_none());
    assert!(factory.choices.borrow().is_empty());
    for id in [None, Some("unknown"), Some("a"), Some("b")] {
        port.prompt_default_external_editor(0, &editors(), id);
    }
    let choices = factory.choices.borrow();
    assert_eq!(
        choices.iter().map(|c| c.initial_selected_index).collect::<Vec<_>>(),
        [0, 0, 0, 1]
    );
    assert_eq!(choices[0].lines, ["Alpha", "Beta"]);
    drop(choices);
    port.dispose();
    assert!(port.show_external_editors(0, &editors(), None).is_none());
    assert!(port.prompt_add_external_editor(0, None).is_none());
    assert!(port.prompt_default_external_editor(0, &editors(), None).is_none());
    assert!(factory.editors.borrow().is_empty());
    assert!(factory.picks.borrow().is_empty());
    assert_eq!(factory.choices.borrow().len(), 4);
}

// Source-derived Settings gets a weak nested opener; retaining it must not retain the port.
#[test]
fn settings_receives_editor_opener_that_expires_with_the_port() {
    let factory = Rc::new(Factory::default());
    let native = port(factory.clone());
    assert!(!native.show_settings(0, SettingsTarget::ExternalEditor));
    assert_eq!(factory.old_settings.get(), 0);
    let opener = factory.opener.borrow().as_ref().unwrap().clone();
    assert!(opener(5, &editors(), Some("a")).is_none());
    assert_eq!(factory.editors.borrow().len(), 1);
    let weak = Rc::downgrade(&native);
    drop(native);
    assert!(weak.upgrade().is_none());
    assert!(opener(5, &editors(), Some("a")).is_none());
    assert_eq!(factory.editors.borrow().len(), 1);
}
