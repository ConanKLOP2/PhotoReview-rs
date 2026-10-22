//! Port of `PhotoReview.Shell.Win32/Dialogs/Win32ClipboardService.cs` (116 lines, baseline `d92d6aae`). Unit W8-18c. Contract: `contracts/W8-18c.md`.
//! The WPF `WpfClipboardService` (23 lines) is retired.
//!
//! `pr_app::services::contracts::ClipboardService` for the Win32 shell (WP-19a): writes text with `OpenClipboard` / `SetClipboardData(CF_UNICODETEXT)`. When another
//! process holds the clipboard it retries 5 times, 20 ms apart (like the WPF `SetDataObject(copy: true)`); still busy is `false`, never a panic. UI thread only.
#![allow(unused_variables, dead_code)] // stub: removed by the porter with the last `todo!()`

use pr_app::services::contracts::ClipboardService;

use crate::win32::structs::{Hwnd, Win32Error};

/// C# `Win32ClipboardService.Attempts`.
pub const ATTEMPTS: u32 = 5;
/// C# `Win32ClipboardService.RetryDelayMilliseconds`.
pub const RETRY_DELAY_MILLISECONDS: u32 = 20;

/// The UTF-16 block `NativeClipboardApi::set_unicode_text` copies into the `HGLOBAL`: the code units of `text` (an embedded NUL is kept; a surrogate pair is two
/// units) followed by ONE terminating `0`. New in Rust so the UTF-16 rules (trap T3) are tested without touching the clipboard.
pub fn utf16_block(text: &str) -> Vec<u16> {
    todo!("W8-18c")
}

/// The byte size of the block for `unit_count` text units: `(unit_count + 1) * 2`, `None` on overflow (the C# `(text.Length + 1) * sizeof(char)` could wrap).
pub fn block_byte_len(unit_count: usize) -> Option<usize> {
    todo!("W8-18c")
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
        todo!("W8-18c")
    }

    /// C# internal ctor `(IClipboardApi api, Func<nint> ownerProvider, Action<int> sleep)`.
    pub fn with_parts(api: Box<dyn ClipboardApi>, owner: Box<dyn Fn() -> Hwnd>, sleep: Box<dyn Fn(u32)>) -> Self {
        todo!("W8-18c")
    }
}

impl ClipboardService for Win32ClipboardService {
    /// `true` when the text is on the clipboard; `false` when it stayed busy, there is no owner window (`owner() == 0`) or the write failed.
    fn try_set_text(&self, text: &str) -> bool {
        todo!("W8-18c")
    }
}

/// C# `NativeClipboardApi`: the real functions.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeClipboardApi;

impl ClipboardApi for NativeClipboardApi {
    fn open(&self, owner: Hwnd) -> bool {
        todo!("W8-18c: user32::open_clipboard(owner).is_ok()")
    }

    fn close(&self) {
        todo!("W8-18c: user32::close_clipboard(), result ignored")
    }

    fn set_unicode_text(&self, text: &str) -> Result<bool, Win32Error> {
        todo!(
            "W8-18c: GlobalAlloc(GMEM_MOVEABLE, (utf16_len + 1) * 2), lock, copy + NUL, unlock, EmptyClipboard, SetClipboardData; free only when not handed over"
        )
    }
}

impl std::fmt::Debug for Win32ClipboardService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Win32ClipboardService").finish_non_exhaustive()
    }
}
