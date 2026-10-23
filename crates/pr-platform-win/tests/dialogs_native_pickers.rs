#![allow(clippy::too_many_lines)]

#[rustfmt::skip]
mod red_tests {

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use pr_platform_win::dialogs::file_dialog::{FileDialogFilter, NativeShellFileDialog, ShellFileDialog, OPEN_OPTIONS, SAVE_OPTIONS};
use pr_platform_win::dialogs::folder_dialog::{NativeShellFolderDialog, ShellFolderDialog, FOLDER_OPTIONS};
use pr_platform_win::win32::structs::Hwnd;
use pr_platform_win::win32::shell_native::{create_file_open_dialog, create_file_save_dialog, take_co_task_string};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::Shell::{IShellItem, SIGDN_FILESYSPATH};
use windows::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GetWindowTextW, IsWindowVisible, PostMessageA};

fn on_sta<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    thread::spawn(move || {
        // SAFETY: This test thread owns its COM apartment and drops COM values before uninitializing it.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).unwrap(); }
        let value = f();
        // SAFETY: All COM values created by the test body have been dropped.
        unsafe { CoUninitialize(); }
        value
    }).join().expect("STA test thread panicked")
}

fn close_visible_windows(thread_id: u32, stop: Arc<AtomicBool>) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !stop.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
        // SAFETY: The callback only checks visibility and posts WM_CLOSE; it retains no window memory.
        unsafe extern "system" fn close(hwnd: windows::Win32::Foundation::HWND, _: LPARAM) -> windows::core::BOOL {
            // SAFETY: hwnd is supplied by EnumThreadWindows and remains valid for this callback.
            if unsafe { IsWindowVisible(hwnd).as_bool() } {
                // SAFETY: Posting WM_CLOSE copies no Rust data into the window procedure.
                let _ = unsafe { PostMessageA(Some(hwnd), 0x0010, WPARAM(0), LPARAM(0)) };
            }
            windows::core::BOOL(1)
        }
        // SAFETY: EnumThreadWindows invokes the callback synchronously on this thread.
        unsafe { let _ = EnumThreadWindows(thread_id, Some(close), LPARAM(0)); }
        thread::sleep(Duration::from_millis(100));
    }
}

fn read_folder(dialog: &windows::Win32::UI::Shell::IFileDialog) -> String {
    // SAFETY: `dialog` is a live COM interface in the current STA apartment.
    let item: IShellItem = unsafe { dialog.GetFolder().unwrap() };
    // SAFETY: `item` is a live shell interface returned by GetFolder.
    let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH).unwrap() };
    // SAFETY: The shell allocated this PWSTR and take_co_task_string frees it.
    unsafe { take_co_task_string(name.0).unwrap() }
}

// Rust-only: the hand-written Debug impls of the native dialogs.
#[test]
fn debug_output_names_the_native_dialogs() {
    assert!(format!("{:?}", NativeShellFolderDialog::new()).starts_with("NativeShellFolderDialog"));
    assert!(format!("{:?}", NativeShellFileDialog::new()).starts_with("NativeShellFileDialog"));
    assert!(format!("{:?}", NativeShellFolderDialog::default()).starts_with("NativeShellFolderDialog"));
    assert!(format!("{:?}", NativeShellFileDialog::default()).starts_with("NativeShellFileDialog"));
}

// C#: NativeShellFolderDialogTests.Configure_SetsFolderPickingFlags_Title_AndInitialFolder
#[test]
#[ignore = "native: needs desktop"]
fn folder_configure_sets_the_folder_picking_flags_title_and_initial_folder() {
    on_sta(|| {
        let dialog = create_file_open_dialog().unwrap();
        NativeShellFolderDialog::configure(&dialog, "Choose folder", Some("C:\\" )).unwrap();
        // SAFETY: `dialog` is a live COM interface in the current STA apartment.
        let options = unsafe { dialog.GetOptions().unwrap() };
        assert_eq!(options.0 & FOLDER_OPTIONS.0, FOLDER_OPTIONS.0);
        assert_eq!(read_folder(&dialog), "C:\\");
    });
}

// C#: NativeShellFolderDialogTests.Show_ReturnsNullWhenTheDialogWindowIsClosed_ReturnsOnlyAfterItWasShown
#[test]
#[ignore = "native: needs desktop"]
fn folder_show_returns_none_when_the_dialog_window_is_closed() {
    on_sta(|| {
        let stop = Arc::new(AtomicBool::new(false));
        let closer = stop.clone();
        // SAFETY: GetCurrentThreadId has no preconditions and returns the current STA thread id.
        let thread_id = unsafe { GetCurrentThreadId() };
        let _helper = thread::spawn(move || close_visible_windows(thread_id, closer));
        let picker = NativeShellFolderDialog::with_before_show(Box::new(|_| {}));
        let result = picker.show(Hwnd::default(), "Choose folder", None);
        stop.store(true, Ordering::Release);
        assert_eq!(result.unwrap(), None);
    });
}

// C#: NativeShellFileDialogTests.OpenDialog_Configure_SetsExistingFileFlags_FileTypeIndex_AndInitialFolder
#[test]
#[ignore = "native: needs desktop"]
fn open_configure_sets_existing_file_flags_file_type_index_and_initial_folder() {
    on_sta(|| {
        let dialog = create_file_open_dialog().unwrap();
        let filters = vec![FileDialogFilter { name: "JSON".into(), pattern: "*.json".into() }];
        let buffers = NativeShellFileDialog::configure(&dialog, "Open", &filters, Some("C:\\"), OPEN_OPTIONS, None, None).unwrap();
        // SAFETY: `dialog` is a live COM interface in the current STA apartment.
        let options = unsafe { dialog.GetOptions().unwrap() };
        // SAFETY: `dialog` is a live COM interface in the current STA apartment.
        let index = unsafe { dialog.GetFileTypeIndex().unwrap() };
        assert_eq!(options.0 & OPEN_OPTIONS.0, OPEN_OPTIONS.0);
        assert_eq!(index, 1);
        assert_eq!(read_folder(&dialog), "C:\\");
        assert!(format!("{buffers:?}").starts_with("FilterBuffers"));
        assert_eq!(buffers.specs().len(), 1);
        // SAFETY: the spec strings are NUL-terminated UTF-16 owned by `buffers`, which is alive here.
        let (spec_name, spec_pattern) = unsafe { (buffers.specs()[0].pszName.to_string().unwrap(), buffers.specs()[0].pszSpec.to_string().unwrap()) };
        assert_eq!((spec_name.as_str(), spec_pattern.as_str()), ("JSON", "*.json"));
        drop(buffers);
    });
}

// C#: NativeShellFileDialogTests.SaveDialog_Configure_SetsOverwritePrompt_FileName_AndKeepsTheFirstFilter
#[test]
#[ignore = "native: needs desktop"]
fn save_configure_sets_overwrite_prompt_file_name_and_keeps_the_first_filter() {
    on_sta(|| {
        let dialog = create_file_save_dialog().unwrap();
        let filters = vec![FileDialogFilter { name: "JSON".into(), pattern: "*.json".into() }];
        let buffers = NativeShellFileDialog::configure(&dialog, "Save", &filters, None, SAVE_OPTIONS, Some(".json"), Some("settings.json")).unwrap();
        // SAFETY: `dialog` is a live COM interface in the current STA apartment.
        let options = unsafe { dialog.GetOptions().unwrap() };
        // SAFETY: `dialog` is a live COM interface in the current STA apartment.
        let filename = unsafe { dialog.GetFileName().unwrap() };
        // SAFETY: The returned PWSTR is allocated by the shell and is freed by take_co_task_string.
        let filename = unsafe { take_co_task_string(filename.0).unwrap() };
        assert_eq!(options.0 & SAVE_OPTIONS.0, SAVE_OPTIONS.0);
        assert_eq!(filename, "settings.json");
        drop(buffers);
    });
}

// C#: NativeShellFileDialogTests.Show_ReturnsNullWhenTheUserClosesTheWindow(false)
#[test]
#[ignore = "native: needs desktop"]
fn open_show_returns_none_when_the_user_closes_the_window() { show_is_cancelled(false); }

// C#: NativeShellFileDialogTests.Show_ReturnsNullWhenTheUserClosesTheWindow(true)
#[test]
#[ignore = "native: needs desktop"]
fn save_show_returns_none_when_the_user_closes_the_window() { show_is_cancelled(true); }

fn show_is_cancelled(save: bool) {
    on_sta(move || {
        let stop = Arc::new(AtomicBool::new(false));
        let closer = stop.clone();
        // SAFETY: GetCurrentThreadId has no preconditions and returns the current STA thread id.
        let thread_id = unsafe { GetCurrentThreadId() };
        let _helper = thread::spawn(move || close_visible_windows(thread_id, closer));
        let dialog = NativeShellFileDialog::with_before_show(Box::new(|_| {}));
        let filters = [FileDialogFilter { name: "JSON".into(), pattern: "*.json".into() }];
        let result = if save { dialog.pick_save_file(Hwnd::default(), "Save", &filters, Some(".json"), Some("a.json"), None) }
            else { dialog.pick_open_file(Hwnd::default(), "Open", &filters, None) };
        stop.store(true, Ordering::Release);
        assert_eq!(result.unwrap(), None);
    });
}

/// Accepts the dialog (posts `WM_COMMAND`/`IDOK`) every 100 ms and records the caption of each visible dialog window it saw.
fn accept_visible_windows(thread_id: u32, stop: Arc<AtomicBool>, captions: Arc<Mutex<Vec<String>>>) {
    let captions: &Mutex<Vec<String>> = &captions;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !stop.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
        // SAFETY: The callback only reads the caption of a visible window and posts WM_COMMAND(IDOK); it retains no window memory.
        unsafe extern "system" fn accept(hwnd: windows::Win32::Foundation::HWND, sink: LPARAM) -> windows::core::BOOL {
            // SAFETY: hwnd is supplied by EnumThreadWindows and remains valid for this callback.
            if unsafe { IsWindowVisible(hwnd).as_bool() } {
                let mut text = [0u16; 256];
                // SAFETY: `text` is a live buffer of 256 UTF-16 units.
                let length = unsafe { GetWindowTextW(hwnd, &mut text) };
                let caption = String::from_utf16_lossy(&text[..usize::try_from(length).unwrap_or(0)]);
                // SAFETY: `sink` is the address of the caller's `Mutex<Vec<String>>`, which outlives the synchronous EnumThreadWindows call.
                unsafe { &*(sink.0 as *const Mutex<Vec<String>>) }.lock().unwrap().push(caption);
                // SAFETY: Posting WM_COMMAND copies no Rust data into the window procedure.
                let _ = unsafe { PostMessageA(Some(hwnd), 0x0111, WPARAM(1), LPARAM(0)) };
            }
            windows::core::BOOL(1)
        }
        // SAFETY: EnumThreadWindows invokes the callback synchronously on this thread.
        unsafe { let _ = EnumThreadWindows(thread_id, Some(accept), LPARAM(std::ptr::from_ref::<Mutex<Vec<String>>>(captions) as isize)); }
        thread::sleep(Duration::from_millis(100));
    }
    // The dialog did not accept (a regression): close it so the test fails instead of hanging.
    if !stop.load(Ordering::Acquire) {
        close_visible_windows(thread_id, stop);
    }
}

/// Runs `show` on an STA thread while a helper thread accepts the dialog; returns the result and the captions seen.
fn show_accepted<T: Send + 'static>(show: impl FnOnce() -> T + Send + 'static) -> (T, Vec<String>) {
    on_sta(move || {
        let stop = Arc::new(AtomicBool::new(false));
        let captions = Arc::new(Mutex::new(Vec::new()));
        // SAFETY: GetCurrentThreadId has no preconditions and returns the current STA thread id.
        let thread_id = unsafe { GetCurrentThreadId() };
        let (helper_stop, helper_captions) = (stop.clone(), captions.clone());
        let helper = thread::spawn(move || accept_visible_windows(thread_id, helper_stop, helper_captions));
        let result = show();
        stop.store(true, Ordering::Release);
        helper.join().expect("accept helper");
        let seen = captions.lock().unwrap().clone();
        (result, seen)
    })
}

fn same_path(actual: &str, expected: &std::path::Path) -> bool {
    actual.eq_ignore_ascii_case(&expected.to_string_lossy())
}

fn set_file_name(dialog: &windows::Win32::UI::Shell::IFileDialog, name: &str) {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `dialog` is live on its STA thread and `wide` is NUL-terminated UTF-16 that outlives the call.
    unsafe { dialog.SetFileName(windows::core::PCWSTR(wide.as_ptr())).unwrap(); }
}

// C#: NativeShellFileDialogTests.OpenDialog_Show_ReturnsTheChosenFilePath (accepted by a helper thread)
#[test]
#[ignore = "native: needs desktop"]
fn open_show_returns_the_chosen_file_path_and_sets_the_title() {
    let root = pr_test_support::TempRoot::new("dialogs-open-accept");
    let file = root.path().join("settings.json");
    std::fs::write(&file, b"{}").unwrap();
    let name = file.to_string_lossy().into_owned();
    let (result, captions) = show_accepted(move || {
        let dialog = NativeShellFileDialog::with_before_show(Box::new(move |d| set_file_name(d, &name)));
        let filters = [FileDialogFilter { name: "JSON".into(), pattern: "*.json".into() }];
        dialog.pick_open_file(Hwnd::default(), "Open settings", &filters, None)
    });
    let picked = result.unwrap().expect("a path");
    assert!(same_path(&picked, &file), "{picked}");
    assert!(captions.iter().any(|c| c == "Open settings"), "{captions:?}");
}

// C#: NativeShellFileDialogTests.SaveDialog_Show_ReturnsTheChosenPath_WithTheDefaultExtension (accepted by a helper thread)
#[test]
#[ignore = "native: needs desktop"]
fn save_show_returns_the_chosen_path_with_the_default_extension_and_sets_the_title() {
    let root = pr_test_support::TempRoot::new("dialogs-save-accept");
    let stem = root.path().join("settings");
    let name = stem.to_string_lossy().into_owned();
    let (result, captions) = show_accepted(move || {
        let dialog = NativeShellFileDialog::new();
        let filters = [FileDialogFilter { name: "JSON".into(), pattern: "*.json".into() }];
        dialog.pick_save_file(Hwnd::default(), "Save settings", &filters, Some(".json"), Some(&name), None)
    });
    let picked = result.unwrap().expect("a path");
    assert!(same_path(&picked, &root.path().join("settings.json")), "{picked}");
    assert!(captions.iter().any(|c| c == "Save settings"), "{captions:?}");
}

// C#: NativeShellFolderDialogTests.Show_ReturnsTheChosenFolder (accepted by a helper thread)
#[test]
#[ignore = "native: needs desktop"]
fn folder_show_returns_the_chosen_folder_and_sets_the_title() {
    let root = pr_test_support::TempRoot::new("dialogs-folder-accept");
    let folder = root.path().join("photos");
    std::fs::create_dir_all(&folder).unwrap();
    let name = folder.to_string_lossy().into_owned();
    let (result, captions) = show_accepted(move || {
        let dialog = NativeShellFolderDialog::with_before_show(Box::new(move |d| set_file_name(d, &name)));
        dialog.show(Hwnd::default(), "Choose photos", None)
    });
    let picked = result.unwrap().expect("a path");
    assert!(same_path(&picked, &folder), "{picked}");
    assert!(captions.iter().any(|c| c == "Choose photos"), "{captions:?}");
}
}
