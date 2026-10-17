//! Port of `PhotoReview.Platform.Windows/WindowsLiveOperationRegistry.cs` (Q-R27 / D-13).
//! The named manual-reset event is shared with C# builds using the exact `Local\` name and SID-bound SHA-256 recipe.

use std::sync::Arc;

use pr_core::abstractions::{LiveMarker, LiveOperationRegistry, Log, NullLog};
use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, OpenEventW, OpenProcessToken, SYNCHRONIZATION_SYNCHRONIZE,
};
use windows::core::{PCWSTR, PWSTR};

const DEFAULT_PREFIX: &str = "PhotoReview";

/// Windows named-event implementation of the process-shared live-operation registry.
#[derive(Clone)]
pub struct WindowsLiveOperationRegistry {
    log: Arc<dyn Log>,
    prefix: String,
}

impl std::fmt::Debug for WindowsLiveOperationRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WindowsLiveOperationRegistry")
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

impl Default for WindowsLiveOperationRegistry {
    fn default() -> Self {
        Self::new(Arc::new(NullLog), DEFAULT_PREFIX)
    }
}

impl WindowsLiveOperationRegistry {
    /// Creates a registry. Tests should pass a unique prefix to avoid colliding with a running app.
    pub fn new(log: Arc<dyn Log>, prefix: impl Into<String>) -> Self {
        let prefix = prefix.into();
        assert!(!prefix.trim().is_empty(), "prefix cannot be empty or whitespace");
        Self { log, prefix }
    }

    fn name_for(&self, operation_id: &str) -> windows::core::Result<String> {
        let sid = current_user_sid()?;
        Ok(name_for_sid(&self.prefix, &sid, operation_id))
    }

    fn report(&self, operation_id: &str, operation: &str, error: &windows::core::Error) {
        self.log.error(
            &format!("Live-operation marker could not be {operation} for journal operation {operation_id}"),
            Some(error),
        );
    }
}

impl LiveOperationRegistry for WindowsLiveOperationRegistry {
    fn begin(&self, operation_id: &str) -> LiveMarker {
        assert!(!operation_id.is_empty(), "operation id must not be empty");
        let result = self.name_for(operation_id).and_then(|name| {
            let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            // SAFETY: `wide` is a live NUL-terminated UTF-16 name; a manual-reset, initially nonsignaled event matches C#.
            unsafe { CreateEventW(None, true, false, PCWSTR(wide.as_ptr())) }
        });
        match result {
            Ok(handle) => Box::new(EventMarker(handle)),
            Err(error) => {
                self.report(operation_id, "created", &error);
                Box::new(NoMarker)
            }
        }
    }

    fn is_live(&self, operation_id: &str) -> bool {
        if operation_id.is_empty() {
            return false;
        }
        let result = self.name_for(operation_id).and_then(|name| {
            let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            // SAFETY: `wide` is a live NUL-terminated UTF-16 object name; SYNCHRONIZE is sufficient to open and close.
            unsafe { OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, false, PCWSTR(wide.as_ptr())) }
        });
        match result {
            Ok(handle) => {
                // SAFETY: this handle was returned by OpenEventW and is closed exactly once here.
                let _ = unsafe { CloseHandle(handle) };
                true
            }
            Err(error) if win32_error_code(&error) == ERROR_ACCESS_DENIED.0 => true,
            Err(error) if win32_error_code(&error) == ERROR_FILE_NOT_FOUND.0 => false,
            Err(error) => {
                self.report(operation_id, "checked", &error);
                false
            }
        }
    }
}

struct EventMarker(HANDLE);

// SAFETY: a kernel event handle may be closed from any thread; Windows handles are process-wide, not thread-affine.
unsafe impl Send for EventMarker {}

impl Drop for EventMarker {
    fn drop(&mut self) {
        // SAFETY: this marker exclusively owns the handle returned by CreateEventW and closes it once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[derive(Debug)]
struct NoMarker;

fn win32_error_code(error: &windows::core::Error) -> u32 {
    (error.code().0 as u32) & 0xFFFF
}

fn name_for_sid(prefix: &str, sid: &str, operation_id: &str) -> String {
    let hash_input = format!("liveop|{sid}|{operation_id}");
    let digest = pr_core::session::store::sha256(hash_input.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02X}");
    }
    format!("Local\\{prefix}_LiveOp_{hex}")
}

fn current_user_sid() -> windows::core::Result<String> {
    let mut token = HANDLE::default();
    // SAFETY: GetCurrentProcess returns the current-process pseudo handle; token is a valid out parameter.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }?;
    let _token = OwnedHandle(token);

    let mut required = 0u32;
    // SAFETY: a null buffer with zero length is the documented size-query form of GetTokenInformation.
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut required) };
    if required < std::mem::size_of::<TOKEN_USER>() as u32 {
        return Err(windows::core::Error::from_thread());
    }
    let word_count = (required as usize).div_ceil(std::mem::size_of::<usize>());
    let mut buffer = vec![0usize; word_count];
    // SAFETY: the aligned buffer has at least `required` writable bytes for the token-user structure.
    unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            required,
            &mut required,
        )
    }?;
    // SAFETY: successful TokenUser query initialized the buffer with TOKEN_USER and its SID pointer.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut wide_sid = PWSTR::null();
    // SAFETY: `sid` is owned by the token information buffer, which remains alive through conversion.
    unsafe { ConvertSidToStringSidW(sid, &mut wide_sid) }?;
    if wide_sid.0.is_null() {
        return Err(windows::core::Error::from_thread());
    }
    let length = unsafe {
        // SAFETY: ConvertSidToStringSidW returns a NUL-terminated string allocated by LocalAlloc.
        let mut length = 0usize;
        while *wide_sid.0.add(length) != 0 {
            length += 1;
        }
        length
    };
    // SAFETY: the pointer references `length` initialized UTF-16 code units followed by NUL.
    let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(wide_sid.0, length) });
    // SAFETY: this string was allocated by ConvertSidToStringSidW and must be freed with LocalFree.
    let _ = unsafe { LocalFree(Some(HLOCAL(wide_sid.0.cast()))) };
    Ok(sid)
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this wrapper exclusively owns the process token handle and closes it once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "native: current-user token SID lookup"]
    fn name_for_uses_bounded_sid_bound_local_event_name() {
        let registry = WindowsLiveOperationRegistry::default();
        let name = registry.name_for(r"C:\photos\a.jpg").expect("current SID is available");
        assert!(name.starts_with("Local\\PhotoReview_LiveOp_"));
        assert_eq!(name.len(), "Local\\PhotoReview_LiveOp_".len() + 64);
        assert!(!name.contains('/'));
        assert_ne!(name, registry.name_for("another-id").expect("current SID is available"));
    }

    #[test]
    #[should_panic(expected = "operation id must not be empty")]
    fn begin_rejects_empty_id() {
        // C# twin: WindowsLiveOperationRegistry.Begin validates `ArgumentException.ThrowIfNullOrEmpty` at line 36.
        let _ = WindowsLiveOperationRegistry::default().begin("");
    }

    #[test]
    #[should_panic(expected = "prefix cannot be empty or whitespace")]
    fn new_rejects_whitespace_prefix() {
        // C# twin: WindowsLiveOperationRegistry constructor uses ArgumentException.ThrowIfNullOrWhiteSpace.
        let _ = WindowsLiveOperationRegistry::new(Arc::new(NullLog), " \t");
    }

    #[test]
    fn name_for_matches_csharp_sha256_recipe_exactly() {
        // C# twin: WindowsLiveOperationRegistry.NameFor; fixed SID/id make the interop name deterministic.
        assert_eq!(
            name_for_sid(
                "PhotoReviewTest",
                "S-1-5-21-123456789-123456789-123456789-1001",
                r"folder\photo:1"
            ),
            "Local\\PhotoReviewTest_LiveOp_EA6B7928352915CF0180637545478D7F230388AF1E3A86DA4C937FB64EB07455"
        );
    }

    #[test]
    #[ignore = "native: named kernel event interprocess visibility"]
    fn begin_is_visible_to_second_registry_until_last_marker_drops() {
        // C# twin: WindowsLiveOperationRegistryTests.Begin_VisibleToSecondRegistry_UntilDisposed and
        // Begin_SameIdTwice_LiveUntilLastHolderReleases.
        let prefix = format!("PhotoReviewRsTest{}", std::process::id());
        let first = WindowsLiveOperationRegistry::new(Arc::new(NullLog), &prefix);
        let second = WindowsLiveOperationRegistry::new(Arc::new(NullLog), &prefix);
        let id = "interprocess-test-operation";
        assert!(!second.is_live(id));
        let one = first.begin(id);
        let two = second.begin(id);
        assert!(first.is_live(id));
        drop(one);
        assert!(second.is_live(id));
        drop(two);
        assert!(!first.is_live(id));
    }
}
