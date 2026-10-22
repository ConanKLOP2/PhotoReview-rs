use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pr_app::services::contracts::ClipboardService;
use pr_platform_win::dialogs::clipboard::{
    ATTEMPTS, ClipboardApi, RETRY_DELAY_MILLISECONDS, Win32ClipboardService, block_byte_len, utf16_block,
};
use pr_platform_win::win32::structs::{Hwnd, Win32Error};

struct FakeState {
    open_failures: Cell<u32>,
    open_calls: Cell<u32>,
    close_calls: Cell<u32>,
    written: RefCell<Vec<String>>,
    set_result: RefCell<Result<bool, Win32Error>>,
    panic_on_set: Cell<bool>,
}

struct FakeClipboard(Rc<FakeState>);

impl ClipboardApi for FakeClipboard {
    fn open(&self, _owner: Hwnd) -> bool {
        self.0.open_calls.set(self.0.open_calls.get() + 1);
        if self.0.open_failures.get() > 0 {
            self.0.open_failures.set(self.0.open_failures.get() - 1);
            false
        } else {
            true
        }
    }

    fn close(&self) {
        self.0.close_calls.set(self.0.close_calls.get() + 1);
    }

    #[allow(clippy::clone_on_copy)] // Keep the red-test implementation unchanged for the red-test guard.
    fn set_unicode_text(&self, text: &str) -> Result<bool, Win32Error> {
        self.0.written.borrow_mut().push(text.to_owned());
        assert!(!self.0.panic_on_set.get(), "injected panic");
        self.0.set_result.borrow().clone()
    }
}

fn make_service(state: &Rc<FakeState>, owner: Hwnd, sleeps: Rc<RefCell<Vec<u32>>>) -> Win32ClipboardService {
    Win32ClipboardService::with_parts(
        Box::new(FakeClipboard(Rc::clone(state))),
        Box::new(move || owner),
        Box::new(move |ms| sleeps.borrow_mut().push(ms)),
    )
}

fn state(open_failures: u32, result: Result<bool, Win32Error>) -> Rc<FakeState> {
    Rc::new(FakeState {
        open_failures: Cell::new(open_failures),
        open_calls: Cell::new(0),
        close_calls: Cell::new(0),
        written: RefCell::new(Vec::new()),
        set_result: RefCell::new(result),
        panic_on_set: Cell::new(false),
    })
}

#[test]
fn win32_clipboard_service_is_the_shell_implementation_of_the_shared_clipboard_contract() {
    // C#: Win32ClipboardService_IsTheShellImplementationOfTheSharedClipboardContract
    fn assert_impl<T: ClipboardService>() {}
    assert_impl::<Win32ClipboardService>();
    let state = state(0, Ok(true));
    let service = make_service(&state, 1, Rc::new(RefCell::new(Vec::new())));
    let contract: &dyn ClipboardService = &service;
    assert!(contract.try_set_text("abc"));
}

#[test]
fn try_set_text_retries_while_the_clipboard_is_busy_then_succeeds() {
    // C#: TrySetText_RetriesWhileClipboardIsBusy_ThenSucceeds
    let state = state(2, Ok(true));
    let sleeps = Rc::new(RefCell::new(Vec::new()));
    assert!(make_service(&state, 1, Rc::clone(&sleeps)).try_set_text("abc"));
    assert_eq!(&*state.written.borrow(), &["abc"]);
    assert_eq!(state.open_calls.get(), 3);
    assert_eq!(&*sleeps.borrow(), &[20, 20]);
    assert_eq!(state.close_calls.get(), 1);
}

#[test]
fn try_set_text_when_always_busy_gives_up_after_five_attempts_without_panicking() {
    // C#: TrySetText_WhenAlwaysBusy_GivesUpAfterFiveAttemptsWithoutThrowing
    let state = state(5, Ok(true));
    let sleeps = Rc::new(RefCell::new(Vec::new()));
    assert!(!make_service(&state, 1, Rc::clone(&sleeps)).try_set_text("abc"));
    assert_eq!(state.open_calls.get(), 5);
    assert_eq!(&*sleeps.borrow(), &[20, 20, 20, 20]);
    assert_eq!(state.close_calls.get(), 0);
}

#[test]
fn try_set_text_when_set_fails_closes_the_clipboard_and_reports_false() {
    // C#: TrySetText_WhenSetFails_ClosesClipboardAndReportsFalse
    let state = state(0, Ok(false));
    assert!(!make_service(&state, 1, Rc::new(RefCell::new(Vec::new()))).try_set_text("abc"));
    assert_eq!(state.open_calls.get(), 1);
    assert_eq!(state.close_calls.get(), 1);
}

#[test]
fn try_set_text_when_set_errors_closes_the_clipboard_and_reports_false() {
    // C#: TrySetText_WhenSetThrowsExternal_ClosesClipboardAndReportsFalse
    let state = state(0, Err(Win32Error(1)));
    assert!(!make_service(&state, 1, Rc::new(RefCell::new(Vec::new()))).try_set_text("abc"));
    assert_eq!(state.close_calls.get(), 1);
}

#[test]
fn try_set_text_without_owner_window_returns_false_without_touching_the_clipboard() {
    // C#: TrySetText_WithoutOwnerWindow_ReturnsFalseWithoutTouchingClipboard
    let state = state(0, Ok(true));
    assert!(!make_service(&state, 0, Rc::new(RefCell::new(Vec::new()))).try_set_text("abc"));
    assert_eq!(state.open_calls.get(), 0);
    assert_eq!(state.close_calls.get(), 0);
}

#[test]
fn a_panic_inside_set_unicode_text_still_closes_the_clipboard() {
    let state = state(0, Ok(true));
    state.panic_on_set.set(true);
    let service = make_service(&state, 1, Rc::new(RefCell::new(Vec::new())));
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| service.try_set_text("abc"))).is_err());
    assert_eq!(state.close_calls.get(), 1);
}

#[test]
fn owner_provider_is_called_once_per_call() {
    let state = state(0, Ok(true));
    let calls = Rc::new(Cell::new(0));
    let calls_in_owner = Rc::clone(&calls);
    let service = Win32ClipboardService::with_parts(
        Box::new(FakeClipboard(Rc::clone(&state))),
        Box::new(move || {
            calls_in_owner.set(calls_in_owner.get() + 1);
            1
        }),
        Box::new(|_| {}),
    );
    assert!(service.try_set_text("abc"));
    assert_eq!(calls.get(), 1);
}

#[test]
fn utf16_block_counts_utf16_units_and_appends_one_terminator() {
    assert_eq!(utf16_block("A😀"), vec![65, 0xD83D, 0xDE00, 0]);
    assert_eq!(utf16_block(""), vec![0]);
}

#[test]
fn utf16_block_keeps_an_embedded_nul_and_still_appends_one_terminator() {
    assert_eq!(utf16_block("a\0b"), vec![97, 0, 98, 0]);
}

#[test]
fn block_byte_len_is_twice_the_units_plus_one_and_none_on_overflow() {
    assert_eq!(block_byte_len(0), Some(2));
    assert_eq!(block_byte_len(3), Some(8));
    assert_eq!(block_byte_len(usize::MAX), None);
    assert_eq!(block_byte_len(usize::MAX / 2), None);
}

#[test]
fn constants_are_five_attempts_and_twenty_milliseconds() {
    assert_eq!(ATTEMPTS, 5);
    assert_eq!(RETRY_DELAY_MILLISECONDS, 20);
}

#[cfg(windows)]
mod native {
    use pr_platform_win::dialogs::clipboard::NativeClipboardApi;
    use pr_platform_win::win32::api::{kernel32, user32};
    use pr_platform_win::win32::messages::CF_UNICODETEXT;

    use super::*;

    #[test]
    fn native_clipboard_open_with_invalid_owner_returns_false() {
        let api = NativeClipboardApi;
        assert!(!api.open(isize::MAX));
    }

    #[test]
    fn native_clipboard_set_without_open_clipboard_returns_false() {
        let api = NativeClipboardApi;
        assert_eq!(api.set_unicode_text("clipboard must remain untouched"), Ok(false));
    }

    struct Owner(Hwnd);

    impl Drop for Owner {
        fn drop(&mut self) {
            let _ = user32::destroy_window(self.0);
        }
    }

    fn read_text(owner: Hwnd) -> Option<String> {
        user32::open_clipboard(owner).ok()?;
        let text = (|| {
            let handle = user32::get_clipboard_data(CF_UNICODETEXT).ok()?;
            let pointer = kernel32::global_lock(handle).ok()?;
            let value = unsafe {
                // SAFETY: GetClipboardData returns CF_UNICODETEXT; GlobalLock keeps its NUL-terminated UTF-16 data valid.
                let pointer = pointer.cast::<u16>();
                let mut length = 0usize;
                while *pointer.add(length) != 0 {
                    length += 1;
                }
                String::from_utf16(std::slice::from_raw_parts(pointer, length)).ok()
            };
            let _ = kernel32::global_unlock(handle);
            value
        })();
        let _ = user32::close_clipboard();
        text
    }

    #[test]
    #[ignore = "native: needs desktop"]
    fn native_clipboard_set_text_can_be_read_back_and_old_text_is_restored() {
        // C#: NativeClipboard_SetText_CanBeReadBackAndOldTextIsRestored
        let hwnd = unsafe {
            // SAFETY: creates a simple STATIC owner window on this test thread; Owner destroys it on drop.
            user32::create_window_ex(0, "STATIC", "", 0, 0, 0, 0, 0, 0, 0, 0, 0)
        }
        .expect("create STATIC owner window");
        let owner = Owner(hwnd);
        let previous = read_text(owner.0);
        let native_api = NativeClipboardApi;
        assert!(native_api.open(owner.0));
        native_api.close();
        assert!(native_api.open(owner.0));
        native_api.close();

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let text = format!("PhotoReview WP-19a {}-{nanos} 😀", std::process::id());
        let service = Win32ClipboardService::new(Box::new(move || owner.0));
        assert!(service.try_set_text(&text));
        assert_eq!(read_text(owner.0).as_deref(), Some(text.as_str()));
        let boundary_text = "x";
        assert!(service.try_set_text(boundary_text));
        assert_eq!(read_text(owner.0).as_deref(), Some(boundary_text));
        if let Some(previous) = previous {
            assert!(service.try_set_text(&previous));
        }
    }
}
