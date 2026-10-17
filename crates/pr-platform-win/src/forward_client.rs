//! Port of `PhotoReview.Platform.Windows/InstanceForwardClient.cs` (C# baseline d92d6aae).
//!
//! The transport is injectable so outcome classification can be checked without connecting to a real application
//! pipe. The Windows implementation uses overlapped I/O and checks caller cancellation while waiting.

use std::fmt;
use std::time::{Duration, Instant};

use pr_core::instance::forwarding::{ForwardError, ForwardOutcome, InstanceForwardClient};
use pr_core::instance::protocol::{self, MAX_PATHS};
use pr_core::sync::CancelToken;

const REPLY_LIMIT: usize = 16;
const CANCEL_POLL: Duration = Duration::from_millis(10);

/// Failure stage matters: before connecting is `NoInstance`; after a successful write it is `Unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardTransportError {
    /// No pipe was available when the connection was attempted.
    NoServer,
    /// A timeout elapsed during connect, write, or read.
    TimedOut,
    /// The connected pipe failed or closed unexpectedly.
    Failed,
    /// Writing stopped and indicates whether a partial frame may have reached the owner.
    WriteFailed { bytes_may_have_reached_server: bool },
}

/// Optional foreground request failures are deliberately non-fatal to forwarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForegroundPermissionError {
    /// user32.dll could not be loaded on this system.
    LibraryUnavailable,
    /// The optional API is missing from user32.dll.
    EntryPointUnavailable,
    /// The pipe or foreground request was invalid or denied.
    InvalidOperation,
}

/// Injectable pipe operations used by the client and deterministic contract tests.
pub trait ForwardClientTransport: Send + Sync {
    /// Establishes a connection by the deadline.
    fn connect(&self, pipe_name: &str, deadline: Instant, cancel: &CancelToken) -> Result<(), ForwardTransportError>;
    /// Confirms the connected server process belongs to the current user, failing closed on inspection errors.
    fn server_is_current_user(&self) -> bool;
    /// Best-effort foreground permission for the connected server process.
    fn allow_server_foreground(&self) -> Result<(), ForegroundPermissionError>;
    /// Writes the complete request before the deadline.
    fn write_all(&self, bytes: &[u8], deadline: Instant, cancel: &CancelToken) -> Result<(), ForwardTransportError>;
    /// Reads at most `limit` bytes through the first newline, before the deadline.
    fn read_reply(
        &self,
        limit: usize,
        deadline: Instant,
        cancel: &CancelToken,
    ) -> Result<Vec<u8>, ForwardTransportError>;
    /// Closes the connection so later calls open a fresh pipe instance.
    fn disconnect(&self);
}

/// Named-pipe client for the second-launch forwarding path.
pub struct ForwardPipeClient<T> {
    pipe_name: String,
    allow_server_foreground: bool,
    transport: T,
    operation: std::sync::Mutex<()>,
}

impl<T> fmt::Debug for ForwardPipeClient<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ForwardPipeClient")
            .field("pipe_name", &self.pipe_name)
            .field("allow_server_foreground", &self.allow_server_foreground)
            .finish_non_exhaustive()
    }
}

impl<T> ForwardPipeClient<T> {
    /// Creates a client; `None` models the C# null argument and is rejected immediately.
    pub fn with_transport(
        pipe_name: Option<String>,
        allow_server_foreground: bool,
        transport: T,
    ) -> Result<Self, ForwardError> {
        let Some(pipe_name) = pipe_name else {
            return Err(ForwardError::Failed {
                kind: "ArgumentNullException".to_owned(),
            });
        };
        Ok(Self {
            pipe_name,
            allow_server_foreground,
            transport,
            operation: std::sync::Mutex::new(()),
        })
    }
}

impl<T: ForwardClientTransport> ForwardPipeClient<T> {
    /// Sends the first `MAX_PATHS` entries, preserving order, and classifies uncertain delivery safely.
    pub fn send_with_timeout(
        &self,
        paths: &[String],
        timeout: Duration,
        cancel: &CancelToken,
    ) -> Result<ForwardOutcome, ForwardError> {
        let _operation = self.operation.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let outcome = self.send_locked(paths, timeout, cancel);
        self.transport.disconnect();
        outcome
    }

    fn send_locked(
        &self,
        paths: &[String],
        timeout: Duration,
        cancel: &CancelToken,
    ) -> Result<ForwardOutcome, ForwardError> {
        let selected = &paths[..paths.len().min(MAX_PATHS)];
        let request = match protocol::encode(selected) {
            Ok(request) => request,
            Err(_) => return Ok(ForwardOutcome::Rejected),
        };
        if cancel.is_canceled() {
            return Err(ForwardError::Canceled);
        }
        let deadline = Instant::now().checked_add(timeout).unwrap_or_else(Instant::now);
        if Instant::now() >= deadline {
            return Ok(ForwardOutcome::NoInstance);
        }
        match self.transport.connect(&self.pipe_name, deadline, cancel) {
            Ok(()) => {}
            Err(_) if cancel.is_canceled() => return Err(ForwardError::Canceled),
            Err(_) => return Ok(ForwardOutcome::NoInstance),
        }
        if !self.transport.server_is_current_user() {
            return Ok(ForwardOutcome::NoInstance);
        }
        if self.allow_server_foreground {
            let _ = self.transport.allow_server_foreground();
        }
        match self.transport.write_all(&request, deadline, cancel) {
            Ok(()) => {}
            Err(_) if cancel.is_canceled() => return Err(ForwardError::Canceled),
            Err(ForwardTransportError::WriteFailed {
                bytes_may_have_reached_server: true,
            }) => {
                return Ok(ForwardOutcome::Unknown);
            }
            Err(ForwardTransportError::WriteFailed {
                bytes_may_have_reached_server: false,
            })
            | Err(ForwardTransportError::Failed)
            | Err(ForwardTransportError::NoServer)
            | Err(ForwardTransportError::TimedOut) => return Ok(ForwardOutcome::NoInstance),
        }
        let reply = match self.transport.read_reply(REPLY_LIMIT, deadline, cancel) {
            Ok(reply) => reply,
            Err(_) if cancel.is_canceled() => return Err(ForwardError::Canceled),
            Err(_) => return Ok(ForwardOutcome::Unknown),
        };
        if cancel.is_canceled() {
            return Err(ForwardError::Canceled);
        }
        let first_line = reply.split(|byte| *byte == b'\n').next().unwrap_or_default();
        Ok(if reply.is_empty() {
            ForwardOutcome::Unknown
        } else if first_line == b"OK" {
            ForwardOutcome::Delivered
        } else {
            ForwardOutcome::Rejected
        })
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::Mutex;

    use windows::Win32::Foundation::{
        CloseHandle, FreeLibrary, GENERIC_READ, GENERIC_WRITE, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows::Win32::Security::{GetLengthSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, OPEN_EXISTING, ReadFile, WriteFile,
    };
    use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::System::Pipes::{GetNamedPipeServerProcessId, WaitNamedPipeW};
    use windows::Win32::System::Threading::{
        CreateEventW, GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
        WaitForMultipleObjects,
    };
    use windows::core::PCWSTR;

    /// Concrete overlapped named-pipe transport. Each send owns one handle, protected for cancellation-safe I/O.
    #[derive(Default)]
    pub struct WindowsForwardTransport {
        pipe: Mutex<Option<HANDLE>>,
    }

    fn sid_bytes(token: HANDLE) -> Option<Vec<u8>> {
        let mut required = 0u32;
        // SAFETY: the first call requests the required length; null buffer and zero size are the documented probe.
        let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut required) };
        if required < std::mem::size_of::<TOKEN_USER>() as u32 {
            return None;
        }
        let word_count = (required as usize).div_ceil(std::mem::size_of::<u64>());
        let mut storage = vec![0u64; word_count];
        // SAFETY: storage is aligned, writable for `required` bytes, and the token remains open for this call.
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(storage.as_mut_ptr().cast()),
                (storage.len() * std::mem::size_of::<u64>()) as u32,
                &mut required,
            )
        }
        .ok()?;
        // SAFETY: GetTokenInformation filled storage with a complete TOKEN_USER structure.
        let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
        // SAFETY: `user.User.Sid` points to the valid SID returned by GetTokenInformation.
        let length = unsafe { GetLengthSid(user.User.Sid) } as usize;
        if length == 0 {
            return None;
        }
        // SAFETY: the returned SID is inside `storage`; GetLengthSid provides its readable byte length.
        Some(unsafe { std::slice::from_raw_parts(user.User.Sid.0.cast::<u8>(), length) }.to_vec())
    }

    fn process_user_sid(process: HANDLE) -> Option<Vec<u8>> {
        let mut token = HANDLE::default();
        // SAFETY: `process` is live and `token` is a valid output slot.
        unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.ok()?;
        let sid = sid_bytes(token);
        // SAFETY: OpenProcessToken returned the owned token handle, which is closed once here.
        let _ = unsafe { CloseHandle(token) };
        sid
    }

    // SAFETY: kernel handles are process-wide values; moving this owner between threads does not alter handle validity.
    unsafe impl Send for WindowsForwardTransport {}
    // SAFETY: all access to the owned handle is serialized by `pipe`, including I/O and close.
    unsafe impl Sync for WindowsForwardTransport {}

    impl WindowsForwardTransport {
        fn wide(value: &str) -> Vec<u16> {
            OsStr::new(value).encode_wide().chain(Some(0)).collect()
        }

        fn wait_io(
            &self,
            handle: HANDLE,
            event: HANDLE,
            deadline: Instant,
            cancel: &CancelToken,
            overlapped: &OVERLAPPED,
        ) -> Result<u32, ForwardTransportError> {
            loop {
                if cancel.is_canceled() {
                    // SAFETY: cancels only this operation; handle and OVERLAPPED stay live until the result is drained.
                    let _ = unsafe { CancelIoEx(handle, Some(overlapped as *const OVERLAPPED)) };
                    let mut ignored = 0;
                    // SAFETY: drain the canceled operation before its buffer and OVERLAPPED leave scope.
                    let _ = unsafe { GetOverlappedResult(handle, overlapped, &mut ignored, true) };
                    return Err(ForwardTransportError::Failed);
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    // SAFETY: cancels only this operation; handle and OVERLAPPED stay live until the result is drained.
                    let _ = unsafe { CancelIoEx(handle, Some(overlapped as *const OVERLAPPED)) };
                    let mut ignored = 0;
                    // SAFETY: drain the canceled operation before its buffer and OVERLAPPED leave scope.
                    let _ = unsafe { GetOverlappedResult(handle, overlapped, &mut ignored, true) };
                    return Err(ForwardTransportError::TimedOut);
                }
                let ms = remaining.min(CANCEL_POLL).as_millis().max(1) as u32;
                // SAFETY: the event is owned by this operation and OVERLAPPED remains live until this wait completes.
                let wait = unsafe { WaitForMultipleObjects(&[event], false, ms) };
                if wait == WAIT_OBJECT_0 {
                    let mut transferred = 0;
                    // SAFETY: the operation event is signaled and the handle/OVERLAPPED remain valid.
                    return unsafe { GetOverlappedResult(handle, overlapped, &mut transferred, false) }
                        .map(|_| transferred)
                        .map_err(|_| ForwardTransportError::Failed);
                }
                if wait == WAIT_TIMEOUT {
                    continue;
                }
                // SAFETY: stop a pending operation before its event, buffer, and OVERLAPPED go out of scope.
                let _ = unsafe { CancelIoEx(handle, Some(overlapped as *const OVERLAPPED)) };
                let mut ignored = 0;
                // SAFETY: drain the canceled operation; all referenced storage remains live for this call.
                let _ = unsafe { GetOverlappedResult(handle, overlapped, &mut ignored, true) };
                return Err(ForwardTransportError::Failed);
            }
        }

        fn transfer(
            &self,
            bytes: &mut [u8],
            write: bool,
            deadline: Instant,
            cancel: &CancelToken,
        ) -> Result<usize, ForwardTransportError> {
            if cancel.is_canceled() {
                return Err(ForwardTransportError::Failed);
            }
            if Instant::now() >= deadline {
                return Err(ForwardTransportError::TimedOut);
            }
            let guard = self.pipe.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(handle) = *guard else {
                return Err(ForwardTransportError::Failed);
            };
            // SAFETY: creates a private event owned until the overlapped operation finishes.
            let event = unsafe { CreateEventW(None, true, false, None) }.map_err(|_| ForwardTransportError::Failed)?;
            let mut overlapped = OVERLAPPED {
                hEvent: event,
                ..OVERLAPPED::default()
            };
            let mut transferred = 0;
            // SAFETY: handle is live; the buffer and OVERLAPPED remain valid until completion is observed.
            let result = unsafe {
                if write {
                    WriteFile(handle, Some(bytes), Some(&mut transferred), Some(&mut overlapped))
                } else {
                    ReadFile(handle, Some(bytes), Some(&mut transferred), Some(&mut overlapped))
                }
            };
            let count = match result {
                Ok(()) => Ok(transferred),
                Err(error) if error.code().0 as u32 == 0x8007_03E5 => {
                    match self.wait_io(handle, event, deadline, cancel, &overlapped) {
                        Err(ForwardTransportError::TimedOut) if write => Err(ForwardTransportError::WriteFailed {
                            bytes_may_have_reached_server: true,
                        }),
                        result => result,
                    }
                }
                Err(_) => Err(ForwardTransportError::Failed),
            };
            // SAFETY: event is owned by this call; a pending operation must be drained before closing (wait_io does so).
            let _ = unsafe { CloseHandle(event) };
            count.map(|count| count as usize)
        }
    }

    impl ForwardClientTransport for WindowsForwardTransport {
        fn connect(
            &self,
            pipe_name: &str,
            deadline: Instant,
            cancel: &CancelToken,
        ) -> Result<(), ForwardTransportError> {
            if cancel.is_canceled() {
                return Err(ForwardTransportError::Failed);
            }
            let name = Self::wide(&format!(r"\\.\pipe\{pipe_name}"));
            loop {
                // SAFETY: name is NUL-terminated and the returned handle is owned by this transport.
                let handle = unsafe {
                    CreateFileW(
                        PCWSTR(name.as_ptr()),
                        GENERIC_READ.0 | GENERIC_WRITE.0,
                        FILE_SHARE_NONE,
                        None,
                        OPEN_EXISTING,
                        FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED,
                        None,
                    )
                };
                match handle {
                    Ok(handle) => {
                        *self.pipe.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handle);
                        return Ok(());
                    }
                    Err(error) if error.code().0 as u32 == 0x8007_0002 || error.code().0 as u32 == 0x8007_00E7 => {
                        if cancel.is_canceled() {
                            return Err(ForwardTransportError::Failed);
                        }
                        if Instant::now() >= deadline {
                            return Err(ForwardTransportError::NoServer);
                        }
                        let wait = deadline.saturating_duration_since(Instant::now()).min(CANCEL_POLL);
                        let wait_name = PCWSTR(name.as_ptr());
                        // SAFETY: name remains live; pipe-busy waits are bounded and missing-pipe retries are throttled.
                        let _ = unsafe { WaitNamedPipeW(wait_name, wait.as_millis().max(1) as u32) };
                        if error.code().0 as u32 == 0x8007_0002 {
                            std::thread::sleep(wait);
                        }
                    }
                    Err(_) => return Err(ForwardTransportError::NoServer),
                }
            }
        }
        fn server_is_current_user(&self) -> bool {
            let guard = self.pipe.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(handle) = *guard else { return false };
            let mut pid = 0;
            // SAFETY: handle is the connected pipe owned by this client; pid is a valid output slot.
            if unsafe { GetNamedPipeServerProcessId(handle, &mut pid) }.is_err() {
                return false;
            }
            // SAFETY: opens only the process reported by the connected pipe for limited token inspection.
            let Ok(server) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
                return false;
            };
            let server_sid = process_user_sid(server);
            // SAFETY: OpenProcess returned an owned process handle, which is closed once here.
            let _ = unsafe { CloseHandle(server) };
            // SAFETY: GetCurrentProcess returns the process pseudo-handle and does not transfer ownership.
            let current_sid = process_user_sid(unsafe { GetCurrentProcess() });
            matches!((server_sid, current_sid), (Some(server), Some(current)) if server == current)
        }
        fn allow_server_foreground(&self) -> Result<(), ForegroundPermissionError> {
            let guard = self.pipe.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(handle) = *guard else {
                return Err(ForegroundPermissionError::InvalidOperation);
            };
            let mut pid = 0;
            // SAFETY: handle is the connected pipe owned by this client; pid is a valid output slot.
            unsafe { GetNamedPipeServerProcessId(handle, &mut pid) }
                .map_err(|_| ForegroundPermissionError::InvalidOperation)?;
            if pid == 0 {
                return Err(ForegroundPermissionError::InvalidOperation);
            }
            let library = Self::wide("user32.dll");
            // SAFETY: `library` is a live NUL-terminated UTF-16 string.
            let module = unsafe { LoadLibraryW(PCWSTR(library.as_ptr())) }
                .map_err(|_| ForegroundPermissionError::LibraryUnavailable)?;
            // SAFETY: module is a live user32 module and the byte string is NUL-terminated.
            let Some(procedure) = (unsafe { GetProcAddress(module, windows::core::s!("AllowSetForegroundWindow")) })
            else {
                // SAFETY: `module` is the owned reference returned by LoadLibraryW and is released on lookup failure.
                let _ = unsafe { FreeLibrary(module) };
                return Err(ForegroundPermissionError::EntryPointUnavailable);
            };
            type AllowForeground = unsafe extern "system" fn(u32) -> windows::Win32::Foundation::BOOL;
            // SAFETY: the named Windows API has the declared system ABI and one u32 PID parameter.
            let allow: AllowForeground =
                unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, AllowForeground>(procedure) };
            // SAFETY: calling the documented API with the server PID is valid; a false result is best-effort denial.
            let allowed = unsafe { allow(pid) };
            // SAFETY: this call owns the reference returned by LoadLibraryW and frees it once.
            let _ = unsafe { FreeLibrary(module) };
            if allowed.as_bool() {
                Ok(())
            } else {
                Err(ForegroundPermissionError::InvalidOperation)
            }
        }
        fn write_all(
            &self,
            bytes: &[u8],
            deadline: Instant,
            cancel: &CancelToken,
        ) -> Result<(), ForwardTransportError> {
            let mut offset = 0;
            while offset < bytes.len() {
                let mut chunk = bytes[offset..].to_vec();
                let written = match self.transfer(&mut chunk, true, deadline, cancel) {
                    Ok(written) => written,
                    Err(ForwardTransportError::TimedOut) if offset == 0 => return Err(ForwardTransportError::TimedOut),
                    // Once a prior chunk was written, or an overlapped write timed out, some bytes may have arrived.
                    Err(ForwardTransportError::TimedOut | ForwardTransportError::WriteFailed { .. }) => {
                        return Err(ForwardTransportError::WriteFailed {
                            bytes_may_have_reached_server: true,
                        });
                    }
                    Err(_) if offset == 0 => {
                        return Err(ForwardTransportError::WriteFailed {
                            bytes_may_have_reached_server: false,
                        });
                    }
                    Err(_) => {
                        return Err(ForwardTransportError::WriteFailed {
                            bytes_may_have_reached_server: true,
                        });
                    }
                };
                if written == 0 {
                    return Err(ForwardTransportError::Failed);
                }
                offset += written;
            }
            Ok(())
        }
        fn read_reply(
            &self,
            limit: usize,
            deadline: Instant,
            cancel: &CancelToken,
        ) -> Result<Vec<u8>, ForwardTransportError> {
            let mut reply = Vec::with_capacity(limit);
            while reply.len() < limit && !reply.contains(&b'\n') {
                let mut buffer = [0u8; 1];
                match self.transfer(&mut buffer, false, deadline, cancel) {
                    Ok(0) => break,
                    Ok(_) => reply.push(buffer[0]),
                    Err(error) => return Err(error),
                }
            }
            Ok(reply)
        }
        fn disconnect(&self) {
            let mut pipe = self.pipe.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(handle) = pipe.take() {
                // SAFETY: this transport owns the client pipe handle and closes it exactly once after each send.
                let _ = unsafe { CloseHandle(handle) };
            }
        }
    }

    impl Drop for WindowsForwardTransport {
        fn drop(&mut self) {
            if let Some(handle) = self
                .pipe
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                // SAFETY: this transport owns the connected client handle and closes it exactly once.
                let _ = unsafe { CloseHandle(handle) };
            }
        }
    }
}

#[cfg(windows)]
pub use native::WindowsForwardTransport;

#[cfg(windows)]
impl ForwardPipeClient<WindowsForwardTransport> {
    /// Creates a production client over a Windows named pipe.
    pub fn new(pipe_name: Option<String>, allow_server_foreground: bool) -> Result<Self, ForwardError> {
        Self::with_transport(pipe_name, allow_server_foreground, WindowsForwardTransport::default())
    }
}

#[cfg(windows)]
impl InstanceForwardClient for ForwardPipeClient<WindowsForwardTransport> {
    fn send(&self, paths: &[String], timeout: Duration, cancel: &CancelToken) -> Result<ForwardOutcome, ForwardError> {
        self.send_with_timeout(paths, timeout, cancel)
    }
}
