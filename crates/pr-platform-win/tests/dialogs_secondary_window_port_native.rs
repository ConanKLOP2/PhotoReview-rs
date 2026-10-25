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
#[path = "common/ui_thread.rs"]
mod ui_thread;
thread_local! {
    static LOOP_PORT: RefCell<Option<Rc<Win32SecondaryWindowPort>>> = const {RefCell::new(None)};
    static QUEUED_RAN: Cell<bool> = const {Cell::new(false)};
}

struct CountingFilter(Cell<usize>);
impl pr_platform_win::host::message_loop::MessageFilter for CountingFilter {
    fn pre_filter_message(&self, _: &pr_platform_win::host::contracts::WindowMessage) -> bool {
        self.0.set(self.0.get() + 1);
        false
    }
}

// C#: NativeDialogLoopTests.DispatcherWorkAndMessageFilters_KeepRunningInsideTheModalLoop.
#[test]
#[ignore = "native: needs desktop"]
fn shell_loop_keeps_dispatcher_work_and_filters_running_inside_a_port_modal() {
    QUEUED_RAN.with(|ran| ran.set(false));
    let dispatcher = Rc::new(ui_thread::TestDispatcher::new());
    let post = dispatcher.poster();
    let loop_ = Rc::new(pr_platform_win::host::message_loop::MessageLoop::new(dispatcher).unwrap());
    let filter = Rc::new(CountingFilter(Cell::new(0)));
    loop_.add_message_filter(filter.clone());
    let ready = Rc::new(move |d: &NativeDialog| {
        // SAFETY: integer-only application/close messages sent to this test-owned modal window.
        unsafe {
            pr_platform_win::win32::api::user32::post_message(d.hwnd(), 0x8017, 0, 0).unwrap();
            pr_platform_win::win32::api::user32::post_message(d.hwnd(), 0x0010, 0, 0).unwrap();
        }
        post(Box::new(|| QUEUED_RAN.with(|ran| ran.set(true))));
    });
    let port = Win32SecondaryWindowPort::new(
        Rc::new(Factory::new()),
        Arc::new(NullLog),
        PortOptions {
            ready: Some(ready),
            message_loop: Some(loop_),
            ..Default::default()
        },
    );
    assert_eq!(port.prompt_custom_zoom(0, 30), None);
    assert!(filter.0.get() > 0);
    QUEUED_RAN.with(|ran| assert!(ran.get()));
    assert_eq!(port.open_count(), 0);
}

// C#: NativeDialogLoopTests.WmQuit_InsideTheModalLoop_ClosesTheDialogAndEndsTheOuterLoop_WithTheSameExitCode.
#[test]
#[ignore = "native: needs desktop"]
fn wm_quit_inside_a_port_modal_is_reposted_to_the_outer_shell_loop() {
    let dispatcher = Rc::new(ui_thread::TestDispatcher::new());
    let loop_ = Rc::new(pr_platform_win::host::message_loop::MessageLoop::new(dispatcher).unwrap());
    let port = Win32SecondaryWindowPort::new(
        Rc::new(Factory::new()),
        Arc::new(NullLog),
        PortOptions {
            ready: Some(Rc::new(|_| pr_platform_win::win32::api::user32::post_quit_message(37))),
            message_loop: Some(loop_.clone()),
            ..Default::default()
        },
    );
    assert_eq!(port.prompt_custom_zoom(0, 30), None);
    assert_eq!(port.open_count(), 0);
    assert_eq!(loop_.run().unwrap(), 37);
}

// C#: NativeDialogLoopTests.ClosingThePort_FromAQueuedCallback_EndsTheDialogOpenedByTheLoop.
#[test]
#[ignore = "native: needs desktop"]
fn queued_callback_can_dispose_the_port_from_inside_its_modal_loop() {
    QUEUED_RAN.with(|ran| ran.set(false));
    let dispatcher = Rc::new(ui_thread::TestDispatcher::new());
    let post = dispatcher.poster();
    let loop_ = Rc::new(pr_platform_win::host::message_loop::MessageLoop::new(dispatcher).unwrap());
    let port = Win32SecondaryWindowPort::new(
        Rc::new(Factory::new()),
        Arc::new(NullLog),
        PortOptions {
            ready: Some(Rc::new(move |_| {
                post(Box::new(|| {
                    let port = LOOP_PORT.with(|slot| slot.borrow().as_ref().cloned().unwrap());
                    assert_eq!(port.open_count(), 1);
                    port.dispose();
                    QUEUED_RAN.with(|ran| ran.set(true));
                }))
            })),
            message_loop: Some(loop_),
            ..Default::default()
        },
    );
    LOOP_PORT.with(|slot| *slot.borrow_mut() = Some(port.clone()));
    assert_eq!(port.prompt_custom_zoom(0, 30), None);
    assert_eq!(port.open_count(), 0);
    QUEUED_RAN.with(|ran| assert!(ran.get()));
    LOOP_PORT.with(|slot| slot.borrow_mut().take());
}

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
    closed_log: Option<Rc<RefCell<Vec<&'static str>>>>,
}
impl Factory {
    fn new() -> Self {
        Self {
            benchmark_calls: Cell::new(0),
            last_benchmark: RefCell::new(None),
            reuse_benchmark: false,
            closed_log: None,
        }
    }
    fn tracked(&self, name: &'static str) -> NativeDialog {
        match &self.closed_log {
            Some(log) => NativeDialog::new(Rc::new(TrackedContent { name, log: log.clone() })),
            None => dialog(),
        }
    }
}
struct TrackedContent {
    name: &'static str,
    log: Rc<RefCell<Vec<&'static str>>>,
}
impl DialogContent for TrackedContent {
    fn title(&self) -> String {
        self.name.to_owned()
    }
    fn items(&self) -> Vec<pr_platform_win::dialogs::native::template::DialogItem> {
        Vec::new()
    }
    fn preferred_client_size(&self, _: &NativeDialog) -> (i32, i32) {
        (160, 80)
    }
    fn layout_controls(&self, _: &NativeDialog, _: i32, _: i32) {}
    fn on_closed(&self, _: &NativeDialog) {
        self.log.borrow_mut().push(self.name);
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
            self.tracked("actions"),
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
        Some(PreparedDialog::new(
            self.tracked("zoom"),
            Box::new(|id| (id == 1).then_some(42)),
        ))
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
        closed_log: None,
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

// C#: Win32Port_Dispose_ClosesAnOpenDialog_AndLaterCommandsDoNothing (nested modal case).
#[test]
#[ignore = "native: needs desktop"]
fn dispose_from_the_inner_modal_closes_inner_before_outer_and_rejects_later_commands() {
    let closed = Rc::new(RefCell::new(Vec::new()));
    let factory = Rc::new(Factory {
        closed_log: Some(closed.clone()),
        ..Factory::new()
    });
    let slot = Rc::new(RefCell::new(None::<Rc<Win32SecondaryWindowPort>>));
    let count = Rc::new(Cell::new(0));
    let outer = Rc::new(Cell::new(0));
    let hook_slot = slot.clone();
    let hook_count = count.clone();
    let hook_outer = outer.clone();
    let hook = Rc::new(move |dialog: &NativeDialog| {
        let port = hook_slot.borrow().as_ref().unwrap().clone();
        hook_count.set(hook_count.get() + 1);
        if hook_count.get() == 1 {
            hook_outer.set(dialog.hwnd());
            assert_eq!(port.open_count(), 1);
            assert!(port.show_action_profiles(dialog.hwnd(), &[]).is_none());
        } else {
            assert_eq!(port.open_count(), 2);
            assert_eq!(
                pr_platform_win::win32::dialog_native::user32::get_window(dialog.hwnd(), 4),
                hook_outer.get()
            );
            assert!(!pr_platform_win::win32::dialog_native::user32::is_window_enabled(
                hook_outer.get()
            ));
            port.dispose();
            // C# Dispose only requests Close; modal registrations stay until their Run/finally blocks unwind.
            assert_eq!(port.open_count(), 2);
        }
    });
    let port = Win32SecondaryWindowPort::new(
        factory,
        Arc::new(NullLog),
        PortOptions {
            ready: Some(hook),
            ..Default::default()
        },
    );
    *slot.borrow_mut() = Some(port.clone());
    assert_eq!(port.prompt_custom_zoom(0, 20), None);
    assert_eq!(&*closed.borrow(), &["actions", "zoom"]);
    assert_eq!(port.open_count(), 0);
    assert_eq!(port.prompt_custom_zoom(0, 20), None);
    port.show_diagnostics(0);
    port.dispose();
    assert_eq!(count.get(), 2);
    assert_eq!(&*closed.borrow(), &["actions", "zoom"]);
    slot.borrow_mut().take();
}

#[test]
#[ignore = "native: needs desktop"]
fn ready_callback_panic_unwinds_after_cleanup_and_removes_the_open_registration() {
    let port = Win32SecondaryWindowPort::new(
        Rc::new(Factory::new()),
        Arc::new(NullLog),
        PortOptions {
            ready: Some(Rc::new(|_| panic!("simulated ready failure"))),
            ..Default::default()
        },
    );
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| port.prompt_custom_zoom(0, 20)));
    assert!(panic.is_err());
    assert_eq!(port.open_count(), 0);
    port.dispose();
}
