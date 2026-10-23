use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use pr_core::abstractions::{BatchReviewItem, NullLog, SettingsTarget, SkippedEntry};
use pr_core::settings::shortcuts::ReviewAction;
use pr_platform_win::dialogs::dialog_service::SecondaryWindowPort;
use pr_platform_win::dialogs::native::dialog::{DialogContent, NativeDialog};
use pr_platform_win::dialogs::secondary_window_port::{
    ActionProfilesOpener, PortOptions, PreparedDialog, SecondaryWindowDialogs, Win32SecondaryWindowPort,
};
use pr_platform_win::win32::structs::Hwnd;

struct MinimalContent;
impl DialogContent for MinimalContent {
    fn title(&self) -> String {
        "secondary port test".to_owned()
    }
    fn items(&self) -> Vec<pr_platform_win::dialogs::native::template::DialogItem> {
        Vec::new()
    }
    fn preferred_client_size(&self, _: &NativeDialog) -> (i32, i32) {
        (160, 80)
    }
    fn layout_controls(&self, _: &NativeDialog, _: i32, _: i32) {}
}

fn dialog() -> NativeDialog {
    NativeDialog::new(Rc::new(MinimalContent))
}

struct Factory {
    benchmark_calls: Cell<usize>,
    last_benchmark: RefCell<Option<NativeDialog>>,
    reuse_benchmark: bool,
}
impl Factory {
    fn new() -> Self {
        Self {
            benchmark_calls: Cell::new(0),
            last_benchmark: RefCell::new(None),
            reuse_benchmark: false,
        }
    }
}
impl SecondaryWindowDialogs for Factory {
    fn settings(&self, _: SettingsTarget, _: ActionProfilesOpener) -> Option<PreparedDialog<bool>> {
        Some(PreparedDialog::new(dialog(), Box::new(|id| id == 1)))
    }
    fn recovery(&self, _: Hwnd) -> Option<PreparedDialog<()>> {
        Some(PreparedDialog::new(dialog(), Box::new(|_| ())))
    }
    fn benchmark(&self, _: Option<&Path>) -> Option<NativeDialog> {
        self.benchmark_calls.set(self.benchmark_calls.get() + 1);
        let dialog = if self.reuse_benchmark {
            self.last_benchmark.borrow().as_ref().cloned().unwrap_or_else(dialog)
        } else {
            dialog()
        };
        *self.last_benchmark.borrow_mut() = Some(dialog.clone());
        Some(dialog)
    }
    fn action_profiles(&self, _: &[ReviewAction]) -> Option<PreparedDialog<Option<Vec<ReviewAction>>>> {
        let actions = vec![ReviewAction::new("custom action")];
        Some(PreparedDialog::new(
            dialog(),
            Box::new(move |id| (id == 1).then_some(actions)),
        ))
    }
    fn diagnostics(&self) -> Option<PreparedDialog<()>> {
        Some(PreparedDialog::new(dialog(), Box::new(|_| ())))
    }
    fn skipped_files(&self, _: &[SkippedEntry]) -> Option<PreparedDialog<()>> {
        Some(PreparedDialog::new(dialog(), Box::new(|_| ())))
    }
    fn batch_review(&self, _: &[BatchReviewItem]) -> Option<PreparedDialog<bool>> {
        Some(PreparedDialog::new(dialog(), Box::new(|id| id == 1)))
    }
    fn custom_zoom(&self, _: i32) -> Option<PreparedDialog<Option<i32>>> {
        Some(PreparedDialog::new(dialog(), Box::new(|id| (id == 1).then_some(42))))
    }
}

// C#: SecondaryWindowRoutingTests.Win32Port_ZoomPrompt_ReturnsTheValue_OrNullWhenCancelled
#[test]
#[ignore = "native: needs desktop"]
fn modal_result_ids_are_preserved_and_open_dialog_bookkeeping_is_removed() {
    for result in [1, 2] {
        let factory = Rc::new(Factory::new());
        let port_slot = Rc::new(RefCell::new(None::<Rc<Win32SecondaryWindowPort>>));
        let seen_count = Rc::new(Cell::new(0));
        let slot = Rc::clone(&port_slot);
        let seen = Rc::clone(&seen_count);
        let hook = Rc::new(move |dialog: &NativeDialog| {
            let port = slot
                .borrow()
                .as_ref()
                .cloned()
                .expect("port installed before modal call");
            seen.set(port.open_count());
            dialog.end_modal(result);
        });
        let port = Win32SecondaryWindowPort::new(
            factory,
            Arc::new(NullLog),
            PortOptions {
                ready: Some(hook),
                ..PortOptions::default()
            },
        );
        *port_slot.borrow_mut() = Some(Rc::clone(&port));
        assert_eq!(
            port.prompt_custom_zoom(0, 16),
            if result == 1 { Some(42) } else { None }
        );
        assert_eq!(seen_count.get(), 1);
        assert_eq!(port.open_count(), 0);
    }
}

// C#: SettingsPortTests.ShowSettings_OpensTheDialog_AndReturnsTrueAfterSave
#[test]
#[ignore = "native: needs desktop"]
fn settings_returns_finish_value_for_ok_and_cancel() {
    for result in [1, 2] {
        let factory = Rc::new(Factory::new());
        let hook = Rc::new(move |dialog: &NativeDialog| dialog.end_modal(result));
        let port = Win32SecondaryWindowPort::new(
            factory,
            Arc::new(NullLog),
            PortOptions {
                ready: Some(hook),
                settings_enabled: Some(Box::new(|| true)),
                ..PortOptions::default()
            },
        );
        assert_eq!(port.show_settings(0, SettingsTarget::Default), result == 1);
    }
}

// C#: SecondaryWindowRoutingTests.Win32Port_BatchReview_IsTrueOnlyForConfirm_AndSkippedAndDiagnosticsOpenOwnedModalWindows
#[test]
#[ignore = "native: needs desktop"]
fn batch_review_is_true_only_for_ok_and_action_profiles_cancel_is_none() {
    for result in [1, 2] {
        let hook = Rc::new(move |dialog: &NativeDialog| dialog.end_modal(result));
        let port = Win32SecondaryWindowPort::new(
            Rc::new(Factory::new()),
            Arc::new(NullLog),
            PortOptions {
                ready: Some(hook),
                ..PortOptions::default()
            },
        );
        assert_eq!(port.show_batch_review(0, &[]), result == 1);
        assert_eq!(port.open_count(), 0);
    }

    let cancel = Rc::new(|dialog: &NativeDialog| dialog.end_modal(2));
    let port = Win32SecondaryWindowPort::new(
        Rc::new(Factory::new()),
        Arc::new(NullLog),
        PortOptions {
            ready: Some(cancel),
            ..PortOptions::default()
        },
    );
    assert_eq!(port.show_action_profiles(0, &[]), None);
    assert_eq!(port.open_count(), 0);
}

// C#: BenchmarkDialogTests.Port_ActionProfiles_ReturnsTheEditedList_OnApply_AndNullOnCancel
#[test]
#[ignore = "native: needs desktop"]
fn action_profiles_apply_returns_the_exact_edited_actions() {
    let hook = Rc::new(|dialog: &NativeDialog| dialog.end_modal(1));
    let port = Win32SecondaryWindowPort::new(
        Rc::new(Factory::new()),
        Arc::new(NullLog),
        PortOptions {
            ready: Some(hook),
            ..PortOptions::default()
        },
    );
    assert_eq!(
        port.show_action_profiles(0, &[]),
        Some(vec![ReviewAction::new("custom action")]),
    );
}

// C#: BenchmarkDialogTests.Port_OpensOneBenchmarkWindow_SecondOpenBringsItForward_ReopensAfterClose
#[test]
#[ignore = "native: needs desktop"]
fn benchmark_is_a_single_instance_and_closed_callback_clears_bookkeeping() {
    let factory = Rc::new(Factory::new());
    let port = Win32SecondaryWindowPort::new(
        Rc::clone(&factory) as Rc<dyn SecondaryWindowDialogs>,
        Arc::new(NullLog),
        PortOptions::default(),
    );
    assert!(!port.is_benchmark_open());
    assert_eq!(port.open_count(), 0);
    port.show_benchmark(0, None);
    assert!(port.is_benchmark_open());
    assert_eq!(port.open_count(), 1);
    port.show_benchmark(0, None);
    assert_eq!(factory.benchmark_calls.get(), 1);
    assert_eq!(port.open_count(), 1);
    factory
        .last_benchmark
        .borrow()
        .as_ref()
        .expect("opened benchmark")
        .close();
    assert!(!port.is_benchmark_open());
    assert_eq!(port.open_count(), 0);
    port.show_benchmark(0, None);
    assert_eq!(factory.benchmark_calls.get(), 2);
    assert_eq!(port.open_count(), 1);
    port.dispose();
    assert_eq!(port.open_count(), 0);
}

// C#: SecondaryWindowRoutingTests.Win32Port_Dispose_ClosesAnOpenDialog_AndLaterCommandsDoNothing
#[test]
#[ignore = "native: needs desktop"]
fn benchmark_show_error_removes_its_registration_and_instance() {
    let factory = Rc::new(Factory {
        benchmark_calls: Cell::new(0),
        last_benchmark: RefCell::new(None),
        reuse_benchmark: true,
    });
    let port = Win32SecondaryWindowPort::new(
        Rc::clone(&factory) as Rc<dyn SecondaryWindowDialogs>,
        Arc::new(NullLog),
        PortOptions::default(),
    );
    port.show_benchmark(0, None);
    factory
        .last_benchmark
        .borrow()
        .as_ref()
        .expect("first benchmark dialog")
        .close();
    assert_eq!(port.open_count(), 0);
    port.show_benchmark(0, None); // reused NativeDialog returns AlreadyShown
    assert_eq!(factory.benchmark_calls.get(), 2);
    assert_eq!(port.open_count(), 0);
    assert!(!port.is_benchmark_open());
}
