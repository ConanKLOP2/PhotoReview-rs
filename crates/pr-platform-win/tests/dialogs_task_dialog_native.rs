use pr_core::abstractions::DialogService;
use pr_platform_win::dialogs::dialog_service::{LazySecondaryPort, Win32DialogService};
use pr_platform_win::dialogs::folder_dialog::{FolderPicker, UnwiredShellFolderDialog};
use pr_platform_win::dialogs::task_dialog::{NativeTaskDialogPort, TaskDialogKind, TaskDialogPort};
use pr_platform_win::win32::activation_context::ActivationScope;
use pr_platform_win::win32::comctl::SHELL_MANIFEST;
use pr_platform_win::win32::messages::{IDCANCEL, WM_CLOSE};
use pr_platform_win::win32::structs::Hwnd;
use std::rc::Rc;

fn show_and_close() -> (i32, Option<String>) {
    let _activation = ActivationScope::from_manifest_text(SHELL_MANIFEST).expect("activate comctl32 v6");
    let port = Rc::new(NativeTaskDialogPort::with_on_created(Box::new(|hwnd: Hwnd| {
        // SAFETY: the callback receives the live dialog window and posts WM_CLOSE to that window.
        let _ = unsafe { pr_platform_win::win32::api::user32::post_message(hwnd, WM_CLOSE, 0, 0) };
    })));
    let picker = FolderPicker::new(Rc::new(UnwiredShellFolderDialog), Box::new(|| 0));
    let secondary = Rc::new(LazySecondaryPort::from_value(None));
    let service =
        Win32DialogService::with_task_dialogs(Box::new(|| 0), Box::new(NativePort(port.clone())), picker, secondary);
    let result = service.show_confirmation("PhotoReview", "Native task dialog test");
    (i32::from(result), port.last_failure())
}

struct NativePort(Rc<NativeTaskDialogPort>);
impl TaskDialogPort for NativePort {
    fn show(&self, owner: Hwnd, title: &str, message: &str, kind: TaskDialogKind) -> i32 {
        self.0.show(owner, title, message, kind)
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn task_dialog_closed_with_wm_close_returns_cancel_and_confirmation_is_false() {
    // C#: NativeTaskDialog_ClosedWithWmClose_ReturnsCancel_AndConfirmationIsFalse
    let (confirmed, failure) = show_and_close();
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(confirmed, 0);
    assert_eq!(IDCANCEL, 2);
}

#[test]
#[ignore = "native: needs desktop"]
fn task_dialog_information_and_error_close_with_cancel() {
    let _activation = ActivationScope::from_manifest_text(SHELL_MANIFEST).expect("activate comctl32 v6");
    for kind in [TaskDialogKind::Information, TaskDialogKind::Error] {
        let port = NativeTaskDialogPort::with_on_created(Box::new(|hwnd| {
            // SAFETY: the callback receives the live dialog window and posts WM_CLOSE to that window.
            let _ = unsafe { pr_platform_win::win32::api::user32::post_message(hwnd, WM_CLOSE, 0, 0) };
        }));
        assert_eq!(port.show(0, "PhotoReview", "Native icon test", kind), IDCANCEL);
        assert!(port.last_failure().is_none());
    }
}
