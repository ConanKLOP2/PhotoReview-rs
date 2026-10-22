#![allow(clippy::too_many_lines)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
use windows::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, IsWindowVisible, PostMessageA};

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
    let item: IShellItem = unsafe { dialog.GetFolder().unwrap() };
    let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH).unwrap() };
    // SAFETY: The shell allocated this PWSTR and take_co_task_string frees it.
    unsafe { take_co_task_string(name.0).unwrap() }
}

// C#: NativeShellFolderDialogTests.Configure_SetsFolderPickingFlags_Title_AndInitialFolder
#[test]
#[ignore = "native: needs desktop"]
fn folder_configure_sets_the_folder_picking_flags_title_and_initial_folder() {
    on_sta(|| {
        let dialog = create_file_open_dialog().unwrap();
        NativeShellFolderDialog::configure(&dialog, "Choose folder", Some("C:\\" )).unwrap();
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
        let options = unsafe { dialog.GetOptions().unwrap() };
        let index = unsafe { dialog.GetFileTypeIndex().unwrap() };
        assert_eq!(options.0 & OPEN_OPTIONS.0, OPEN_OPTIONS.0);
        assert_eq!(index, 1);
        assert_eq!(read_folder(&dialog), "C:\\");
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
        let options = unsafe { dialog.GetOptions().unwrap() };
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
