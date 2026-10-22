use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use pr_app::services::contracts::ZoomPromptService;
use pr_core::abstractions::{BatchReviewItem, DialogService, Log, SettingsTarget, SkippedEntry, SkippedKind};
use pr_l10n::{keys, localizer::Localizer};
use pr_platform_win::dialogs::dialog_service::{
    LazySecondaryPort, PortFactory, SecondaryWindowKind, SecondaryWindowPort, SecondaryWindowPortHost,
    Win32DialogService,
};
use pr_platform_win::dialogs::folder_dialog::{FolderPicker, ShellFolderDialog};
use pr_platform_win::dialogs::task_dialog::{TaskDialogKind, TaskDialogPort, common_buttons, main_icon};
use pr_platform_win::dialogs::zoom_prompt_service::Win32ZoomPromptService;
use pr_platform_win::win32::comctl::{TDCBF_NO_BUTTON, TDCBF_OK_BUTTON, TDCBF_YES_BUTTON};
use pr_platform_win::win32::messages::{IDCANCEL, IDNO, IDYES};
use pr_platform_win::win32::structs::{Hresult, Hwnd};

#[derive(Default)]
struct TaskCalls(RefCell<Vec<(Hwnd, String, String, TaskDialogKind)>>);
struct FakeTaskDialogs {
    result: Cell<i32>,
    calls: Rc<TaskCalls>,
}
impl TaskDialogPort for FakeTaskDialogs {
    fn show(&self, owner: Hwnd, title: &str, message: &str, kind: TaskDialogKind) -> i32 {
        self.calls
            .0
            .borrow_mut()
            .push((owner, title.to_owned(), message.to_owned(), kind));
        self.result.get()
    }
}

struct FolderRecord {
    call: RefCell<Option<(Hwnd, String, Option<String>)>>,
    answer: RefCell<Result<Option<String>, Hresult>>,
}
impl Default for FolderRecord {
    fn default() -> Self {
        Self {
            call: RefCell::new(None),
            answer: RefCell::new(Ok(None)),
        }
    }
}
struct FakeFolder(Rc<FolderRecord>);
impl ShellFolderDialog for FakeFolder {
    fn show(&self, owner: Hwnd, title: &str, initial: Option<&str>) -> Result<Option<String>, Hresult> {
        *self.0.call.borrow_mut() = Some((owner, title.to_owned(), initial.map(str::to_owned)));
        self.0.answer.borrow().clone()
    }
}

#[derive(Default)]
struct PortRecord {
    calls: RefCell<Vec<String>>,
    items: RefCell<Vec<BatchReviewItem>>,
    zoom: Cell<Option<i32>>,
    dispose_count: Cell<u32>,
    settings_result: Cell<bool>,
    batch_result: Cell<bool>,
}
struct FakePort(Rc<PortRecord>);
impl SecondaryWindowPort for FakePort {
    fn show_settings(&self, owner: Hwnd, target: SettingsTarget) -> bool {
        self.0.calls.borrow_mut().push(format!("settings:{owner}:{target:?}"));
        self.0.settings_result.get()
    }
    fn show_recovery(&self, owner: Hwnd) {
        self.0.calls.borrow_mut().push(format!("recovery:{owner}"));
    }
    fn show_diagnostics(&self, owner: Hwnd) {
        self.0.calls.borrow_mut().push(format!("diagnostics:{owner}"));
    }
    fn show_benchmark(&self, owner: Hwnd, folder: Option<&Path>) {
        self.0.calls.borrow_mut().push(format!(
            "benchmark:{owner}:{}",
            folder.map_or(String::new(), |p| p.to_string_lossy().into_owned())
        ));
    }
    fn show_skipped_files(&self, owner: Hwnd, entries: &[SkippedEntry]) {
        self.0
            .calls
            .borrow_mut()
            .push(format!("skipped:{owner}:{}", entries.len()));
    }
    fn show_batch_review(&self, owner: Hwnd, items: &[BatchReviewItem]) -> bool {
        self.0.calls.borrow_mut().push(format!("batch:{owner}:{}", items.len()));
        *self.0.items.borrow_mut() = items.to_vec();
        self.0.batch_result.get()
    }
    fn prompt_custom_zoom(&self, owner: Hwnd, percent: i32) -> Option<i32> {
        self.0.calls.borrow_mut().push(format!("zoom:{owner}:{percent}"));
        self.0.zoom.get()
    }
    fn dispose(&self) {
        self.0.dispose_count.set(self.0.dispose_count.get() + 1);
    }
}

#[derive(Default)]
struct RecordingLog(std::sync::Mutex<Vec<(String, Option<String>)>>);
impl Log for RecordingLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, _: &str) {}
    fn warn(&self, _: &str) {}
    fn error(&self, message: &str, error: Option<&(dyn std::error::Error + 'static)>) {
        self.0
            .lock()
            .expect("log mutex")
            .push((message.to_owned(), error.map(ToString::to_string)));
    }
}

fn service(
    owner: Hwnd,
    task_result: i32,
    folder: Rc<FolderRecord>,
    secondary: Rc<LazySecondaryPort>,
) -> (Win32DialogService, Rc<TaskCalls>) {
    let calls = Rc::new(TaskCalls::default());
    let picker = FolderPicker::new(Rc::new(FakeFolder(folder)), Box::new(move || owner));
    let service = Win32DialogService::with_task_dialogs(
        Box::new(move || owner),
        Box::new(FakeTaskDialogs {
            result: Cell::new(task_result),
            calls: Rc::clone(&calls),
        }),
        picker,
        secondary,
    )
    .with_localizer(Box::new(Localizer::current));
    (service, calls)
}

#[test]
fn zoom_prompt_forwards_owner_and_current_percent_and_returns_the_chosen_value() {
    // C#: ZoomPrompt_ForwardsOwnerAndCurrentPercent_AndReturnsTheChosenValue
    let record = Rc::new(PortRecord {
        zoom: Cell::new(Some(250)),
        ..Default::default()
    });
    let secondary = Rc::new(LazySecondaryPort::from_value(Some(Rc::new(FakePort(Rc::clone(
        &record,
    ))))));
    let zoom = Win32ZoomPromptService::new(Box::new(|| 77), secondary);
    assert_eq!(zoom.prompt_custom_zoom(125), Some(250));
    assert_eq!(&*record.calls.borrow(), &["zoom:77:125"]);
}

#[test]
fn zoom_prompt_cancelled_returns_none() {
    // C#: ZoomPrompt_Cancelled_ReturnsNull
    let secondary = Rc::new(LazySecondaryPort::from_value(Some(Rc::new(FakePort(Rc::new(
        PortRecord::default(),
    ))))));
    assert_eq!(
        Win32ZoomPromptService::new(Box::new(|| 77), secondary).prompt_custom_zoom(125),
        None
    );
}

#[test]
fn zoom_prompt_bridge_missing_returns_none_without_throwing() {
    // C#: ZoomPrompt_BridgeMissing_ReturnsNullWithoutThrowing
    let secondary = Rc::new(LazySecondaryPort::from_value(None));
    assert_eq!(
        Win32ZoomPromptService::new(Box::new(|| 77), secondary).prompt_custom_zoom(125),
        None
    );
}

#[test]
fn zoom_prompt_does_not_load_the_bridge_until_first_use() {
    // C#: ZoomPrompt_DoesNotLoadTheBridge_UntilFirstUse
    let count = Rc::new(Cell::new(0));
    let c = Rc::clone(&count);
    let factory: PortFactory = Box::new(move || {
        c.set(c.get() + 1);
        Some(Rc::new(FakePort(Rc::new(PortRecord::default()))))
    });
    let lazy = Rc::new(LazySecondaryPort::new(factory));
    let zoom = Win32ZoomPromptService::new(Box::new(|| 1), lazy);
    assert_eq!(count.get(), 0);
    let _ = zoom.prompt_custom_zoom(100);
    let _ = zoom.prompt_custom_zoom(100);
    assert_eq!(count.get(), 1);
}

#[test]
fn show_confirmation_only_yes_is_true() {
    // C#: ShowConfirmation_OnlyYesIsTrue
    for (pressed, expected) in [(IDYES, true), (IDNO, false), (IDCANCEL, false)] {
        let folder = Rc::new(FolderRecord::default());
        let lazy = Rc::new(LazySecondaryPort::from_value(None));
        let (service, calls) = service(0x1234, pressed, folder, lazy);
        assert_eq!(service.show_confirmation("Tiêu đề", "Xoá?"), expected);
        assert_eq!(
            &*calls.0.borrow(),
            &[(0x1234, "Tiêu đề".into(), "Xoá?".into(), TaskDialogKind::Confirmation)]
        );
    }
}

#[test]
fn show_message_and_show_error_use_information_and_error_kinds() {
    // C#: ShowMessageAndShowError_UseInformationAndErrorKinds
    let (service, calls) = service(
        1,
        IDYES,
        Rc::new(FolderRecord::default()),
        Rc::new(LazySecondaryPort::from_value(None)),
    );
    service.show_message("T1", "M1");
    service.show_error("T2", "M2");
    assert_eq!(
        calls.0.borrow().iter().map(|x| x.3).collect::<Vec<_>>(),
        [TaskDialogKind::Information, TaskDialogKind::Error]
    );
}

#[test]
fn pick_folder_uses_catalog_title_and_drops_missing_initial_folder() {
    // C#: PickFolder_UsesCatalogTitle_AndDropsMissingInitialFolder
    let record = Rc::new(FolderRecord::default());
    *record.answer.borrow_mut() = Ok(Some("D:\\Photos".into()));
    let (service, _) = service(
        0x1234,
        IDYES,
        Rc::clone(&record),
        Rc::new(LazySecondaryPort::from_value(None)),
    );
    assert_eq!(
        service.pick_folder(Some(Path::new("Z:\\does-not-exist"))),
        Some(PathBuf::from("D:\\Photos"))
    );
    let call = record.call.borrow();
    let (_, title, initial) = call.as_ref().expect("folder dialog called");
    assert_eq!(title, &Localizer::current().get(keys::DIALOG_PICK_FOLDER_TITLE));
    assert_eq!(initial, &None);
}

#[test]
fn pick_folder_passes_existing_initial_folder_and_null_when_cancelled() {
    // C#: PickFolder_PassesExistingInitialFolder_AndNullWhenCancelled
    let path = std::env::temp_dir();
    let record = Rc::new(FolderRecord::default());
    let (service, _) = service(
        0x1234,
        IDYES,
        Rc::clone(&record),
        Rc::new(LazySecondaryPort::from_value(None)),
    );
    assert_eq!(service.pick_folder(Some(&path)), None);
    assert_eq!(
        record.call.borrow().as_ref().and_then(|x| x.2.as_deref()),
        path.to_str()
    );
}

#[test]
fn secondary_windows_are_delegated_with_owner_and_loaded_lazily() {
    // C#: SecondaryWindows_AreDelegatedWithOwner_AndLoadedLazily
    let record = Rc::new(PortRecord {
        settings_result: Cell::new(true),
        ..Default::default()
    });
    let n = Rc::new(Cell::new(0));
    let n2 = Rc::clone(&n);
    let r = Rc::clone(&record);
    let lazy = Rc::new(LazySecondaryPort::new(Box::new(move || {
        n2.set(n2.get() + 1);
        Some(Rc::new(FakePort(r)))
    })));
    let (service, _) = service(4660, IDYES, Rc::new(FolderRecord::default()), lazy);
    service.show_confirmation("t", "m");
    assert_eq!(n.get(), 0);
    assert!(service.show_settings_at(SettingsTarget::ExternalEditor));
    service.show_recovery();
    service.show_diagnostics();
    service.show_benchmark(Some(Path::new("C:\\x")));
    service.show_skipped_files(&[SkippedEntry {
        path: "a".into(),
        reason: "r".into(),
        kind: SkippedKind::FileUnreadable,
    }]);
    assert_eq!(n.get(), 1);
    assert_eq!(
        &*record.calls.borrow(),
        &[
            "settings:4660:ExternalEditor",
            "recovery:4660",
            "diagnostics:4660",
            "benchmark:4660:C:\\x",
            "skipped:4660:1"
        ]
    );
}

#[test]
fn show_batch_review_path_overload_maps_to_items_with_unknown_length() {
    // C#: ShowBatchReview_PathOverload_MapsToItemsWithUnknownLength
    let record = Rc::new(PortRecord {
        batch_result: Cell::new(true),
        ..Default::default()
    });
    let lazy = Rc::new(LazySecondaryPort::from_value(Some(Rc::new(FakePort(Rc::clone(
        &record,
    ))))));
    let (service, _) = service(1, IDYES, Rc::new(FolderRecord::default()), lazy);
    assert!(service.show_batch_review_paths(&[PathBuf::from("C:\\a.jpg"), PathBuf::from("C:\\b.jpg")]));
    let items = record.items.borrow();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].length, None);
    assert_eq!(items[0].path, PathBuf::from("C:\\a.jpg"));
}

#[test]
fn secondary_windows_when_bridge_unavailable_return_false_and_do_not_throw() {
    // C#: SecondaryWindows_WhenBridgeUnavailable_ReturnFalseAndDoNotThrow
    let (service, _) = service(
        1,
        IDYES,
        Rc::new(FolderRecord::default()),
        Rc::new(LazySecondaryPort::from_value(None)),
    );
    assert!(!service.show_settings());
    assert!(!service.show_batch_review(&[]));
    service.show_recovery();
    service.show_diagnostics();
    service.show_benchmark(None);
    service.show_skipped_files(&[]);
}

#[test]
fn common_buttons_and_main_icon_per_kind() {
    assert_eq!(
        common_buttons(TaskDialogKind::Confirmation),
        TDCBF_YES_BUTTON | TDCBF_NO_BUTTON
    );
    assert_eq!(common_buttons(TaskDialogKind::Information), TDCBF_OK_BUTTON);
    assert_eq!(common_buttons(TaskDialogKind::Error), TDCBF_OK_BUTTON);
    assert_eq!(main_icon(TaskDialogKind::Confirmation), 0);
    assert_eq!(main_icon(TaskDialogKind::Information), 0xFFFD);
    assert_eq!(main_icon(TaskDialogKind::Error), 0xFFFF);
}

#[test]
fn lazy_port_runs_factory_once_and_keeps_an_absent_answer() {
    let count = Rc::new(Cell::new(0));
    let c = Rc::clone(&count);
    let lazy = LazySecondaryPort::new(Box::new(move || {
        c.set(c.get() + 1);
        None
    }));
    assert!(lazy.get().is_none());
    assert!(lazy.get().is_none());
    assert_eq!(count.get(), 1);
    assert!(lazy.is_value_created());
}

#[test]
fn lazy_port_reentrant_get_returns_none() {
    let slot = Rc::new(RefCell::<Option<Rc<LazySecondaryPort>>>::new(None));
    let slot2 = Rc::clone(&slot);
    let lazy = Rc::new(LazySecondaryPort::new(Box::new(move || {
        assert!(slot2.borrow().as_ref().and_then(|p| p.get()).is_none());
        None
    })));
    *slot.borrow_mut() = Some(Rc::clone(&lazy));
    assert!(lazy.get().is_none());
    assert!(lazy.is_value_created());
}

#[test]
fn port_host_dispose_does_nothing_when_port_was_never_created() {
    let count = Rc::new(Cell::new(0));
    let c = Rc::clone(&count);
    let lazy = Rc::new(LazySecondaryPort::new(Box::new(move || {
        c.set(c.get() + 1);
        None
    })));
    SecondaryWindowPortHost::new(lazy).dispose();
    assert_eq!(count.get(), 0);
}

#[test]
fn port_host_dispose_disposes_a_created_port_once() {
    let record = Rc::new(PortRecord::default());
    let lazy = Rc::new(LazySecondaryPort::from_value(Some(Rc::new(FakePort(Rc::clone(
        &record,
    ))))));
    let host = SecondaryWindowPortHost::new(lazy);
    host.dispose();
    assert_eq!(record.dispose_count.get(), 1);
}

#[test]
fn default_port_members_support_everything_and_have_no_action_profiles() {
    let port = FakePort(Rc::new(PortRecord::default()));
    assert!(port.supports(SecondaryWindowKind::Settings));
    assert!(port.show_action_profiles(1, &[]).is_none());
}

#[test]
fn pick_folder_failure_is_logged_and_answers_none() {
    let record = Rc::new(FolderRecord::default());
    *record.answer.borrow_mut() = Err(Hresult(0x80004005u32 as i32));
    let log = Arc::new(RecordingLog::default());
    let (service, _) = service(
        1,
        IDYES,
        Rc::clone(&record),
        Rc::new(LazySecondaryPort::from_value(None)),
    );
    let service = service.with_log(log.clone());
    assert_eq!(service.pick_folder(None), None);
    assert_eq!(log.0.lock().expect("log mutex")[0].0, "Folder dialog failed");
}

#[test]
fn pick_folder_with_a_path_that_is_not_unicode_has_no_initial_folder() {
    let record = Rc::new(FolderRecord::default());
    let (service, _) = service(
        1,
        IDYES,
        Rc::clone(&record),
        Rc::new(LazySecondaryPort::from_value(None)),
    );
    let invalid = PathBuf::from(OsString::from_wide(&[0xD800]));
    let _ = service.pick_folder(Some(&invalid));
    assert_eq!(record.call.borrow().as_ref().and_then(|x| x.2.as_deref()), None);
}

#[test]
fn owner_is_read_at_call_time_by_the_service_and_the_zoom_service() {
    let owner = Rc::new(Cell::new(1));
    let o = Rc::clone(&owner);
    let record = Rc::new(PortRecord::default());
    let lazy = Rc::new(LazySecondaryPort::from_value(Some(Rc::new(FakePort(Rc::clone(
        &record,
    ))))));
    let zoom = Win32ZoomPromptService::new(
        Box::new(move || {
            let x = o.get();
            o.set(x + 1);
            x
        }),
        Rc::clone(&lazy),
    );
    let _ = zoom.prompt_custom_zoom(10);
    let next = Rc::clone(&owner);
    let calls = Rc::new(TaskCalls::default());
    let tc = Rc::clone(&calls);
    let folder = Rc::new(FolderRecord::default());
    let picker = FolderPicker::new(Rc::new(FakeFolder(folder)), Box::new(|| 0));
    let service = Win32DialogService::with_task_dialogs(
        Box::new(move || {
            let x = next.get();
            next.set(x + 1);
            x
        }),
        Box::new(FakeTaskDialogs {
            result: Cell::new(IDYES),
            calls: tc,
        }),
        picker,
        lazy,
    )
    .with_localizer(Box::new(Localizer::current));
    assert!(service.show_confirmation("t", "m"));
    service.show_recovery();
    assert_eq!(calls.0.borrow()[0].0, 2);
    assert_eq!(&*record.calls.borrow(), &["zoom:1:10", "recovery:3"]);
}

#[test]
fn lazy_port_factory_returning_none_is_not_run_again() {
    let count = Rc::new(Cell::new(0));
    let c = Rc::clone(&count);
    let lazy = LazySecondaryPort::new(Box::new(move || {
        c.set(c.get() + 1);
        None
    }));
    assert!(lazy.get().is_none());
    assert!(lazy.get().is_none());
    assert_eq!(count.get(), 1);
}

#[test]
fn folder_picker_policy_golden_title_matches_catalog() {
    let title = Localizer::current().get(keys::DIALOG_PICK_FOLDER_TITLE);
    assert!(!title.is_empty());
}
