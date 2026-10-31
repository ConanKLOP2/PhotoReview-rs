//! ST-COM-RECYCLE C2: the `IFileOperationProgressSink` that tells a recycle from a permanent delete.
//!
//! The sink is driven through its COM interface by hand (no Shell delete, no Recycle Bin, no file is touched); the
//! `IShellItem`s are only parsed from an existing temp folder. Whether the real Shell fills `psiNewlyCreated` for a
//! recycle is NOT proven here; that is the attended `#[ignore]`d native test in `recycle_bin_facade.rs`.
#![cfg(windows)]

// These source modules are path-loaded like the other recycle tests.
#[allow(dead_code)]
#[path = "../src/recycle_bin.rs"]
mod recycle_bin;
#[allow(dead_code)]
#[path = "../src/recycle_bin_capacity.rs"]
mod recycle_bin_capacity;
#[allow(dead_code)]
#[path = "../src/recycle_bin_settings.rs"]
mod recycle_bin_settings;
#[allow(dead_code)]
#[path = "../src/recycle_eligibility.rs"]
mod recycle_eligibility;

use pr_test_support::TempRoot;
use recycle_bin::ShellRecycleOutcome::{DeletedPermanently, Failed, MovedToRecycleBin};
use recycle_bin::{classify_post_delete, delete_sink, merge_outcome};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Shell::{IFileOperationProgressSink, IShellItem, SHCreateItemFromParsingName};
use windows::core::{HRESULT, PCWSTR};

const E_ACCESSDENIED: i32 = 0x8007_0005u32 as i32;

struct Com;

impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: balances the successful CoInitializeEx of `Com::init` on this thread.
        unsafe { CoUninitialize() };
    }
}

impl Com {
    fn init() -> Self {
        // SAFETY: each test runs on its own thread without a prior apartment.
        let status = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        assert!(status.is_ok(), "{status:?}");
        Self
    }
}

fn folder_item(root: &TempRoot) -> IShellItem {
    let text = root.path().to_str().unwrap().to_owned();
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: wide is a live NUL-terminated string of an existing folder; the item owns its reference.
    unsafe { SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(wide.as_ptr()), None) }.unwrap()
}

fn post_delete(sink: &IFileOperationProgressSink, item: &IShellItem, hr: i32, created: Option<&IShellItem>) {
    // SAFETY: sink and items are live COM interfaces; the sink only reads the pointers' nullness.
    unsafe { sink.PostDeleteItem(0, item, HRESULT(hr), created) }.unwrap();
}

#[test]
fn classify_post_delete_maps_the_three_shell_reports() {
    assert_eq!(classify_post_delete(0, true), MovedToRecycleBin);
    assert_eq!(classify_post_delete(0, false), DeletedPermanently);
    assert_eq!(classify_post_delete(E_ACCESSDENIED, true), Failed(E_ACCESSDENIED));
    assert_eq!(classify_post_delete(E_ACCESSDENIED, false), Failed(E_ACCESSDENIED));
    // A positive (success) HRESULT such as COPYENGINE_S_* still counts as success.
    assert_eq!(classify_post_delete(0x0027_0001, true), MovedToRecycleBin);
    assert_eq!(classify_post_delete(0x0027_0001, false), DeletedPermanently);
}

#[test]
fn merge_outcome_keeps_the_worst_report() {
    assert_eq!(merge_outcome(None, MovedToRecycleBin), MovedToRecycleBin);
    assert_eq!(
        merge_outcome(Some(MovedToRecycleBin), DeletedPermanently),
        DeletedPermanently
    );
    assert_eq!(
        merge_outcome(Some(DeletedPermanently), MovedToRecycleBin),
        DeletedPermanently
    );
    assert_eq!(
        merge_outcome(Some(Failed(E_ACCESSDENIED)), MovedToRecycleBin),
        Failed(E_ACCESSDENIED)
    );
    assert_eq!(
        merge_outcome(Some(MovedToRecycleBin), MovedToRecycleBin),
        MovedToRecycleBin
    );
}

#[test]
fn sink_without_any_callback_leaves_the_slot_empty() {
    let (_sink, slot) = delete_sink::new_sink();
    assert_eq!(slot.get(), None);
}

#[test]
fn sink_post_delete_with_a_new_item_reports_moved_to_recycle_bin() {
    let _com = Com::init();
    let root = TempRoot::new("sink-moved");
    let item = folder_item(&root);
    let (sink, slot) = delete_sink::new_sink();
    post_delete(&sink, &item, 0, Some(&item));
    assert_eq!(slot.get(), Some(MovedToRecycleBin));
}

#[test]
fn sink_post_delete_without_a_new_item_reports_deleted_permanently() {
    let _com = Com::init();
    let root = TempRoot::new("sink-permanent");
    let item = folder_item(&root);
    let (sink, slot) = delete_sink::new_sink();
    post_delete(&sink, &item, 0, None);
    assert_eq!(slot.get(), Some(DeletedPermanently));
}

#[test]
fn sink_post_delete_with_a_failed_hresult_reports_failed() {
    let _com = Com::init();
    let root = TempRoot::new("sink-failed");
    let item = folder_item(&root);
    let (sink, slot) = delete_sink::new_sink();
    post_delete(&sink, &item, E_ACCESSDENIED, None);
    assert_eq!(slot.get(), Some(Failed(E_ACCESSDENIED)));
}

#[test]
fn sink_keeps_the_permanent_report_when_called_twice() {
    let _com = Com::init();
    let root = TempRoot::new("sink-twice");
    let item = folder_item(&root);
    let (sink, slot) = delete_sink::new_sink();
    post_delete(&sink, &item, 0, None);
    post_delete(&sink, &item, 0, Some(&item));
    assert_eq!(slot.get(), Some(DeletedPermanently));
}

#[test]
fn sink_other_callbacks_succeed_and_do_not_record() {
    let _com = Com::init();
    let root = TempRoot::new("sink-others");
    let item = folder_item(&root);
    let (sink, slot) = delete_sink::new_sink();
    // SAFETY: sink and item are live COM interfaces; every call is a no-op returning S_OK.
    unsafe {
        sink.StartOperations().unwrap();
        sink.PreDeleteItem(0, &item).unwrap();
        sink.UpdateProgress(1, 0).unwrap();
        sink.ResetTimer().unwrap();
        sink.PauseTimer().unwrap();
        sink.ResumeTimer().unwrap();
        sink.FinishOperations(HRESULT(0)).unwrap();
    }
    assert_eq!(slot.get(), None);
}
