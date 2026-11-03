//! Review-confirm tests of group G7 (`docs/reviews/G7-G15-2026-11-03.md`): what already holds in `Win32DialogService` / `LazySecondaryPort` and had no test.
//! C#: `Win32DialogService.PromptRename(request) => Port()?.PromptRename(_ownerProvider(), request)`; `Port()` only builds the port once the graph exists.
#![cfg(windows)]

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use pr_core::abstractions::{BatchReviewItem, DialogService, RenamePromptRequest, SettingsTarget, SkippedEntry};
use pr_platform_win::dialogs::dialog_service::{LazySecondaryPort, SecondaryWindowPort, Win32DialogService};
use pr_platform_win::dialogs::folder_dialog::{FolderPicker, ShellFolderDialog};
use pr_platform_win::dialogs::task_dialog::{TaskDialogKind, TaskDialogPort};
use pr_platform_win::win32::structs::{Hresult, Hwnd};

struct NoTasks;
impl TaskDialogPort for NoTasks {
    fn show(&self, _: Hwnd, _: &str, _: &str, _: TaskDialogKind) -> i32 {
        0
    }
}

struct NoFolder;
impl ShellFolderDialog for NoFolder {
    fn show(&self, _: Hwnd, _: &str, _: Option<&str>) -> Result<Option<String>, Hresult> {
        Ok(None)
    }
}

/// Records the rename calls; every other member is a no-op.
#[derive(Default)]
struct RenamePort {
    calls: RefCell<Vec<(Hwnd, String, String)>>,
}
impl SecondaryWindowPort for RenamePort {
    fn show_settings(&self, _: Hwnd, _: SettingsTarget) -> bool {
        false
    }
    fn show_recovery(&self, _: Hwnd) {}
    fn show_diagnostics(&self, _: Hwnd) {}
    fn show_benchmark(&self, _: Hwnd, _: Option<&Path>) {}
    fn show_skipped_files(&self, _: Hwnd, _: &[SkippedEntry]) {}
    fn show_batch_review(&self, _: Hwnd, _: &[BatchReviewItem]) -> bool {
        false
    }
    fn prompt_custom_zoom(&self, _: Hwnd, _: i32) -> Option<i32> {
        None
    }
    fn prompt_rename(&self, owner: Hwnd, request: &RenamePromptRequest) -> Option<String> {
        self.calls
            .borrow_mut()
            .push((owner, request.initial_name.clone(), request.extension.clone()));
        Some(format!("{}-renamed", request.initial_name))
    }
}

fn request(initial: &str, extension: &str) -> RenamePromptRequest {
    RenamePromptRequest {
        initial_name: initial.to_owned(),
        extension: extension.to_owned(),
        validate: Arc::new(|_, _| None),
    }
}

fn dialog_over(owner: Hwnd, secondary: Rc<LazySecondaryPort>) -> Win32DialogService {
    Win32DialogService::with_task_dialogs(
        Box::new(move || owner),
        Box::new(NoTasks),
        FolderPicker::new(Rc::new(NoFolder), Box::new(move || owner)),
        secondary,
    )
}

#[test]
fn prompt_rename_forwards_the_owner_and_the_request_and_returns_the_typed_name() {
    // C#: Win32DialogService.PromptRename => Port()?.PromptRename(_ownerProvider(), request)  (dialogs-base M1, already fixed; this test was missing)
    let port = Rc::new(RenamePort::default());
    let dialog = dialog_over(
        31,
        Rc::new(LazySecondaryPort::from_value(Some(
            port.clone() as Rc<dyn SecondaryWindowPort>
        ))),
    );

    let answer = dialog.prompt_rename(&request("IMG_0001", ".jpg"));

    assert_eq!(answer.as_deref(), Some("IMG_0001-renamed"));
    assert_eq!(
        *port.calls.borrow(),
        [(31, "IMG_0001".to_owned(), ".jpg".to_owned())],
        "the owner window and the request reach the port"
    );
}

#[test]
fn prompt_rename_without_a_port_is_a_silent_cancel() {
    // C#: Port() null -> `?.` -> null (cancel)
    let dialog = dialog_over(32, Rc::new(LazySecondaryPort::from_value(None)));
    assert_eq!(dialog.prompt_rename(&request("a", ".jpg")), None);
}

#[test]
fn lazy_port_waiting_for_readiness_answers_none_without_running_the_factory_and_builds_once_ready() {
    // New (startup review M3, already fixed in `new_when_ready`): a request before the graph is bound neither runs the factory nor locks the port at None.
    let ready = Rc::new(Cell::new(false));
    let built = Rc::new(Cell::new(0));
    let ready_for_gate = Rc::clone(&ready);
    let built_for_factory = Rc::clone(&built);
    let lazy = LazySecondaryPort::new_when_ready(
        Box::new(move || ready_for_gate.get()),
        Box::new(move || {
            built_for_factory.set(built_for_factory.get() + 1);
            Some(Rc::new(RenamePort::default()) as Rc<dyn SecondaryWindowPort>)
        }),
    );

    assert!(lazy.get().is_none());
    assert!(lazy.get().is_none());
    assert_eq!(built.get(), 0, "the factory must not run before the gate opens");
    assert!(!lazy.is_value_created(), "the early None is not kept");

    ready.set(true);
    assert!(lazy.get().is_some(), "after the gate opens the port is built");
    assert!(lazy.get().is_some());
    assert_eq!(built.get(), 1, "built exactly once");
    assert!(lazy.is_value_created());
}
