//! Port of `PhotoReview.Shell.Win32/Dialogs/Win32ClipboardService.cs` (116 lines, baseline `d92d6aae`). Unit W8-18c. Contract: `contracts/W8-18c.md`.
//! The WPF `WpfClipboardService` (23 lines) is retired.
//!
//! `pr_app::services::contracts::ClipboardService` for the Win32 shell (WP-19a): writes text with `OpenClipboard` / `SetClipboardData(CF_UNICODETEXT)`. When another
//! process holds the clipboard it retries 5 times, 20 ms apart (like the WPF `SetDataObject(copy: true)`); still busy is `false`, never a panic. UI thread only.
use pr_app::services::contracts::ClipboardService;

use crate::win32::{
    api::{kernel32, user32},
    messages::{CF_UNICODETEXT, GMEM_MOVEABLE},
    structs::{Hwnd, Win32Error},
};
use std::time::Duration;

/// C# `Win32ClipboardService.Attempts`.
pub const ATTEMPTS: u32 = 5;
/// C# `Win32ClipboardService.RetryDelayMilliseconds`.
pub const RETRY_DELAY_MILLISECONDS: u32 = 20;

/// The UTF-16 block `NativeClipboardApi::set_unicode_text` copies into the `HGLOBAL`: the code units of `text` (an embedded NUL is kept; a surrogate pair is two
/// units) followed by ONE terminating `0`. New in Rust so the UTF-16 rules (trap T3) are tested without touching the clipboard.
pub fn utf16_block(text: &str) -> Vec<u16> {
    let mut block: Vec<u16> = text.encode_utf16().collect();
    block.push(0);
    block
}

/// The byte size of the block for `unit_count` text units: `(unit_count + 1) * 2`, `None` on overflow (the C# `(text.Length + 1) * sizeof(char)` could wrap).
pub fn block_byte_len(unit_count: usize) -> Option<usize> {
    unit_count.checked_add(1)?.checked_mul(std::mem::size_of::<u16>())
}

/// C# `IClipboardApi`: the seam over the clipboard functions (a fake in the retry and clean-up tests).
pub trait ClipboardApi {
    /// `OpenClipboard(owner)`: `false` when another process holds the clipboard.
    fn open(&self, owner: Hwnd) -> bool;
    /// `CloseClipboard`.
    fn close(&self);
    /// Empties the clipboard and sets `CF_UNICODETEXT`. `Ok(false)` = the OS refused (no memory is leaked); `Err` = the C# `ExternalException` / `OutOfMemoryException` /
    /// `InvalidOperationException` that `TrySetText` turns into `false`.
    fn set_unicode_text(&self, text: &str) -> Result<bool, Win32Error>;
}

/// C# `Win32ClipboardService`.
pub struct Win32ClipboardService {
    api: Box<dyn ClipboardApi>,
    owner: Box<dyn Fn() -> Hwnd>,
    sleep: Box<dyn Fn(u32)>,
}

impl Win32ClipboardService {
    /// C# public ctor `(Func<nint> ownerProvider)`: `NativeClipboardApi` and `std::thread::sleep` of the given milliseconds.
    pub fn new(owner: Box<dyn Fn() -> Hwnd>) -> Self {
        Self::with_parts(
            Box::new(NativeClipboardApi),
            owner,
            Box::new(|ms| std::thread::sleep(Duration::from_millis(ms.into()))),
        )
    }

    /// C# internal ctor `(IClipboardApi api, Func<nint> ownerProvider, Action<int> sleep)`.
    pub fn with_parts(api: Box<dyn ClipboardApi>, owner: Box<dyn Fn() -> Hwnd>, sleep: Box<dyn Fn(u32)>) -> Self {
        Self { api, owner, sleep }
    }
}

impl ClipboardService for Win32ClipboardService {
    /// `true` when the text is on the clipboard; `false` when it stayed busy, there is no owner window (`owner() == 0`) or the write failed.
    fn try_set_text(&self, text: &str) -> bool {
        let owner = (self.owner)();
        if owner == 0 {
            return false;
        }

        for attempt in 0..ATTEMPTS {
            if attempt > 0 {
                (self.sleep)(RETRY_DELAY_MILLISECONDS);
            }
            if !self.api.open(owner) {
                continue;
            }

            struct CloseGuard<'a>(&'a dyn ClipboardApi);
            impl Drop for CloseGuard<'_> {
                fn drop(&mut self) {
                    self.0.close();
                }
            }
            let _close = CloseGuard(self.api.as_ref());
            return self.api.set_unicode_text(text).unwrap_or(false);
        }
        false
    }
}

/// C# `NativeClipboardApi`: the real functions.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeClipboardApi;

impl ClipboardApi for NativeClipboardApi {
    fn open(&self, owner: Hwnd) -> bool {
        user32::open_clipboard(owner).is_ok()
    }

    fn close(&self) {
        let _ = user32::close_clipboard();
    }

    fn set_unicode_text(&self, text: &str) -> Result<bool, Win32Error> {
        let block = utf16_block(text);
        let Some(bytes) = block_byte_len(block.len() - 1) else {
            return Ok(false);
        };
        let memory = match kernel32::global_alloc(GMEM_MOVEABLE, bytes) {
            Ok(memory) => memory,
            Err(_) => return Ok(false),
        };

        struct MemoryGuard {
            memory: isize,
            handed_over: bool,
        }
        impl Drop for MemoryGuard {
            fn drop(&mut self) {
                if !self.handed_over {
                    // SAFETY: memory was allocated here, is unlocked, and was not handed to the system.
                    unsafe { kernel32::global_free(self.memory) };
                }
            }
        }
        let mut guard = MemoryGuard {
            memory,
            handed_over: false,
        };

        let target = match kernel32::global_lock(memory) {
            Ok(target) => target,
            Err(_) => return Ok(false),
        };
        // SAFETY: target is GlobalLock's address for `bytes == block.len() * 2` bytes, is u16-aligned, `block` is live and non-overlapping, and the block remains locked until GlobalUnlock.
        unsafe { std::ptr::copy_nonoverlapping(block.as_ptr(), target.cast::<u16>(), block.len()) };
        let _ = kernel32::global_unlock(memory);

        if user32::empty_clipboard().is_err() {
            return Ok(false);
        }
        // SAFETY: memory is a GMEM_MOVEABLE handle containing NUL-terminated UTF-16 CF_UNICODETEXT; on success ownership passes to the system and the guard is disarmed.
        match unsafe { user32::set_clipboard_data(CF_UNICODETEXT, memory) } {
            Ok(_) => {
                guard.handed_over = true;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }
}

impl std::fmt::Debug for Win32ClipboardService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Win32ClipboardService").finish_non_exhaustive()
    }
}
