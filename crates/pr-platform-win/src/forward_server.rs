//! Port of `PhotoReview.Platform.Windows/InstanceForwardServer.cs`'s `InstanceForwardServer` class.
//!
//! The listener accepts one pipe at a time. `ForwardPipeFactory` and `ForwardPipe` isolate the protocol loop from
//! kernel I/O so its limits, replies, callback behavior, retries, and disposal can be tested with deterministic fakes.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use pr_core::abstractions::{Log, NullLog};
use pr_core::instance::protocol::{self, MAX_MESSAGE_BYTES};

const RETRY_DELAY: Duration = Duration::from_millis(500);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(5);
const DISPOSE_JOIN_TIMEOUT: Duration = Duration::from_secs(2);

type PathsCallback = dyn Fn(&[String]) + Send + Sync;
/// Predicate used to confirm every decoded path exists before invoking the callback.
pub type PathExists = dyn Fn(&str) -> bool + Send + Sync;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Error from a single named-pipe operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardPipeError {
    /// The server is shutting down and pending native I/O was canceled.
    Canceled,
    /// The request deadline elapsed while reading or writing.
    TimedOut,
    /// A native or fake pipe operation failed.
    Failed(String),
}

impl fmt::Display for ForwardPipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Canceled => f.write_str("operation canceled"),
            Self::TimedOut => f.write_str("named pipe request timed out"),
            Self::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ForwardPipeError {}

/// One server-side named-pipe instance.
pub trait ForwardPipe: Send {
    /// Waits for one client; returns only after the client connects or the factory is canceled.
    fn accept(&mut self) -> Result<(), ForwardPipeError>;
    /// Reads some bytes before `deadline`; zero means the client closed its write side.
    fn read(&mut self, buffer: &mut [u8], deadline: Instant) -> Result<usize, ForwardPipeError>;
    /// Writes the reply before the request's original deadline.
    fn write_all(&mut self, bytes: &[u8], deadline: Instant) -> Result<(), ForwardPipeError>;
}

/// Creates secure pipe instances and supplies cancellation-aware retry waits.
pub trait ForwardPipeFactory: Send + Sync + 'static {
    /// Creates a new server instance for `pipe_name`.
    fn create(self: Arc<Self>, pipe_name: &str) -> Result<Box<dyn ForwardPipe>, ForwardPipeError>;
    /// Wakes pending accept/read/write/retry operations and makes future operations return `Canceled`.
    fn cancel(&self);
    /// Reports whether the factory has been canceled.
    fn is_canceled(&self) -> bool;
    /// Waits for the retry delay; returns false when canceled first.
    fn wait_retry(&self, delay: Duration) -> bool;
}

struct ServerInner {
    pipe_name: String,
    callback: Arc<PathsCallback>,
    log: Arc<dyn Log>,
    path_exists: Arc<PathExists>,
    read_timeout: Duration,
    factory: Arc<dyn ForwardPipeFactory>,
    disposed: AtomicBool,
}

/// Owner side of the Q-R10 forwarding channel. `start` is idempotent; `dispose` cancels and joins its listener.
pub struct InstanceForwardServer {
    inner: Arc<ServerInner>,
    thread: Mutex<Option<JoinHandle<()>>>,
    finished: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl fmt::Debug for InstanceForwardServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InstanceForwardServer")
            .field("pipe_name", &self.inner.pipe_name)
            .field("disposed", &self.inner.disposed.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl InstanceForwardServer {
    /// Creates a server with injectable I/O, path-existence, and callback behavior for deterministic tests.
    pub fn with_factory(
        pipe_name: impl Into<String>,
        callback: impl Fn(&[String]) + Send + Sync + 'static,
        log: Option<Arc<dyn Log>>,
        path_exists: impl Fn(&str) -> bool + Send + Sync + 'static,
        read_timeout: Option<Duration>,
        factory: Arc<dyn ForwardPipeFactory>,
    ) -> Self {
        Self {
            inner: Arc::new(ServerInner {
                pipe_name: pipe_name.into(),
                callback: Arc::new(callback),
                log: log.unwrap_or_else(|| Arc::new(NullLog)),
                path_exists: Arc::new(path_exists),
                read_timeout: read_timeout.unwrap_or(DEFAULT_READ_TIMEOUT),
                factory,
                disposed: AtomicBool::new(false),
            }),
            thread: Mutex::new(None),
            finished: Mutex::new(None),
        }
    }

    /// Creates a production server using the W6-03b current-user-only pipe security descriptor.
    #[cfg(windows)]
    pub fn new(
        pipe_name: impl Into<String>,
        callback: impl Fn(&[String]) + Send + Sync + 'static,
        log: Option<Arc<dyn Log>>,
        path_exists: Option<Arc<PathExists>>,
        read_timeout: Option<Duration>,
    ) -> windows_core::Result<Self> {
        Ok(Self::with_factory(
            pipe_name,
            callback,
            log,
            move |path| {
                path_exists
                    .as_ref()
                    .map_or_else(|| std::fs::metadata(path).is_ok(), |exists| exists(path))
            },
            read_timeout,
            Arc::new(NativeForwardPipeFactory::new()?),
        ))
    }

    /// Starts the single-client accept loop. Calling this repeatedly does not create additional workers.
    pub fn start(&self) -> Result<(), ForwardPipeError> {
        if self.inner.disposed.load(Ordering::Acquire) {
            return Err(ForwardPipeError::Failed("server is disposed".to_owned()));
        }
        let mut thread_slot = lock(&self.thread);
        if self.inner.disposed.load(Ordering::Acquire) {
            return Err(ForwardPipeError::Failed("server is disposed".to_owned()));
        }
        if thread_slot.is_some() {
            return Ok(());
        }
        let inner = Arc::clone(&self.inner);
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let worker = thread::Builder::new()
            .name("PhotoReviewRs-forward-server".to_owned())
            .spawn(move || {
                run_server(inner);
                let _ = finished_tx.send(());
            })
            .map_err(|error| ForwardPipeError::Failed(format!("thread spawn failed: {error}")))?;
        *thread_slot = Some(worker);
        *lock(&self.finished) = Some(finished_rx);
        Ok(())
    }

    /// Cancels pending operations and joins the worker. Repeated calls are harmless.
    pub fn dispose(&self) {
        if !self.inner.disposed.swap(true, Ordering::AcqRel) {
            self.inner.factory.cancel();
        }
        let worker = lock(&self.thread).take();
        let Some(worker) = worker else { return };
        let finished = lock(&self.finished).take();
        let stopped = finished.is_some_and(|receiver| receiver.recv_timeout(DISPOSE_JOIN_TIMEOUT).is_ok());
        if stopped {
            let _ = worker.join();
        } else {
            self.inner.log.warn("Instance forward listener did not stop in time");
        }
    }
}

impl Drop for InstanceForwardServer {
    fn drop(&mut self) {
        self.dispose();
    }
}

#[derive(Debug)]
struct CallbackPanic;

impl fmt::Display for CallbackPanic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("instance forward callback panicked")
    }
}

impl std::error::Error for CallbackPanic {}

fn retry_delay(failures: u32) -> Duration {
    let shift = failures.saturating_sub(1).min(6);
    RETRY_DELAY.saturating_mul(1u32 << shift).min(MAX_RETRY_DELAY)
}

fn log_listener_failure(log: &dyn Log, failures: u32, error: &ForwardPipeError) {
    if failures.is_power_of_two() {
        log.warn(&format!(
            "Instance forward listener error: {error} (consecutive failures: {failures})"
        ));
    }
}

fn run_server(inner: Arc<ServerInner>) {
    let mut failures = 0u32;
    while !inner.factory.is_canceled() {
        let mut pipe = match Arc::clone(&inner.factory).create(&inner.pipe_name) {
            Ok(pipe) => pipe,
            Err(ForwardPipeError::Canceled) => break,
            Err(error) => {
                failures = failures.saturating_add(1);
                log_listener_failure(inner.log.as_ref(), failures, &error);
                if !inner.factory.wait_retry(retry_delay(failures)) {
                    break;
                }
                continue;
            }
        };
        match pipe.accept() {
            Ok(()) => failures = 0,
            Err(ForwardPipeError::Canceled) if inner.factory.is_canceled() => break,
            Err(error) => {
                failures = failures.saturating_add(1);
                log_listener_failure(inner.log.as_ref(), failures, &error);
                drop(pipe);
                if !inner.factory.wait_retry(retry_delay(failures)) {
                    break;
                }
                continue;
            }
        }
        handle_request(&inner, pipe.as_mut());
    }
}

fn handle_request(inner: &ServerInner, pipe: &mut dyn ForwardPipe) {
    let deadline = Instant::now()
        .checked_add(inner.read_timeout)
        .unwrap_or_else(Instant::now);
    let mut buffer = vec![0u8; MAX_MESSAGE_BYTES + 1];
    let mut length = 0usize;
    while length <= MAX_MESSAGE_BYTES && !protocol::is_complete(&buffer[..length]) {
        if Instant::now() >= deadline {
            inner.log.warn("Instance forward request timed out");
            return;
        }
        match pipe.read(&mut buffer[length..], deadline) {
            Ok(0) => break,
            Ok(read) => length += read,
            Err(ForwardPipeError::TimedOut) => {
                inner.log.warn("Instance forward request timed out");
                return;
            }
            Err(ForwardPipeError::Canceled) => return,
            Err(error) => {
                inner
                    .log
                    .warn(&format!("Instance forward request read failed: {error}"));
                return;
            }
        }
    }

    let paths = protocol::try_decode(&buffer[..length], |path| (inner.path_exists)(path));
    let accepted = if let Some(paths) = paths {
        match catch_unwind(AssertUnwindSafe(|| (inner.callback)(&paths))) {
            Ok(()) => true,
            Err(_) => {
                inner.log.error("Instance forward handler failed", Some(&CallbackPanic));
                false
            }
        }
    } else {
        inner
            .log
            .warn(&format!("Instance forward request rejected ({length} byte(s))"));
        false
    };
    let reply: &[u8] = if accepted { b"OK\n" } else { b"ERR\n" };
    if Instant::now() < deadline {
        let _ = pipe.write_all(reply, deadline);
    }
}

#[cfg(windows)]
mod native_pipe {
    use super::*;
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_IO_PENDING, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows::Win32::Security::SECURITY_ATTRIBUTES;
    use windows::Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile};
    use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, NAMED_PIPE_MODE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE,
    };
    use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects, WaitForSingleObject};
    use windows::core::HSTRING;

    use crate::forward_pipe::current_security_descriptor;

    fn server_pipe_mode() -> NAMED_PIPE_MODE {
        PIPE_TYPE_BYTE | PIPE_REJECT_REMOTE_CLIENTS
    }

    #[cfg(test)]
    #[allow(clippy::items_after_test_module)] // Native factory below shares this private module with the mode helper.
    mod tests {
        use super::*;

        #[test]
        fn server_pipe_mode_rejects_remote_clients() {
            assert_ne!(server_pipe_mode().0 & PIPE_REJECT_REMOTE_CLIENTS.0, 0);
        }
    }

    pub(super) struct NativeForwardPipeFactory {
        stop_event: HANDLE,
        canceled: AtomicBool,
        active_pipe: Mutex<HANDLE>,
    }

    // SAFETY: HANDLE values refer to kernel event/pipe objects; shared operations are synchronized by Windows and the mutex.
    unsafe impl Send for NativeForwardPipeFactory {}
    // SAFETY: `active_pipe` serializes replacement/close, and cancellation only signals the stable event and live pipe.
    unsafe impl Sync for NativeForwardPipeFactory {}

    impl NativeForwardPipeFactory {
        pub(super) fn new() -> windows_core::Result<Self> {
            // SAFETY: null attributes create an event with default security; the event is owned and closed in Drop.
            let stop_event = unsafe { CreateEventW(None, true, false, None)? };
            Ok(Self {
                stop_event,
                canceled: AtomicBool::new(false),
                active_pipe: Mutex::new(HANDLE::default()),
            })
        }

        fn wait_operation(
            &self,
            pipe: HANDLE,
            overlapped: &OVERLAPPED,
            event: HANDLE,
            deadline: Option<Instant>,
        ) -> Result<u32, ForwardPipeError> {
            let handles = [self.stop_event, event];
            let timeout = deadline.map_or(u32::MAX, |deadline| {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    0
                } else {
                    remaining.as_millis().clamp(1, u32::MAX as u128) as u32
                }
            });
            // SAFETY: both handles are live auto/manual-reset events; `overlapped` belongs to `pipe` and remains live.
            let result = unsafe { WaitForMultipleObjects(&handles, false, timeout) };
            if result == WAIT_OBJECT_0 {
                // SAFETY: this cancels only the operation using the live handle and stack-owned OVERLAPPED.
                let _ = unsafe { CancelIoEx(pipe, Some(overlapped)) };
                let mut transferred = 0;
                // SAFETY: waiting here drains the canceled operation before its OVERLAPPED/buffer leave scope.
                let _ = unsafe { GetOverlappedResult(pipe, overlapped, &mut transferred, true) };
                return Err(ForwardPipeError::Canceled);
            }
            if result.0 == WAIT_TIMEOUT.0 {
                // SAFETY: this cancels only the operation using the live handle and stack-owned OVERLAPPED.
                let _ = unsafe { CancelIoEx(pipe, Some(overlapped)) };
                let mut transferred = 0;
                // SAFETY: waiting here drains the timed-out operation before its OVERLAPPED/buffer leave scope.
                let _ = unsafe { GetOverlappedResult(pipe, overlapped, &mut transferred, true) };
                return Err(ForwardPipeError::TimedOut);
            }
            let mut transferred = 0;
            // SAFETY: the operation event was signaled; handle and OVERLAPPED remain live through result retrieval.
            unsafe { GetOverlappedResult(pipe, overlapped, &mut transferred, false) }
                .map_err(|error| ForwardPipeError::Failed(error.to_string()))?;
            Ok(transferred)
        }

        fn operation_event(&self) -> Result<HANDLE, ForwardPipeError> {
            // SAFETY: null attributes create a private, owned event; Drop closes it after the pending operation drains.
            unsafe { CreateEventW(None, true, false, None) }
                .map_err(|error| ForwardPipeError::Failed(error.to_string()))
        }

        fn activate(&self, pipe: HANDLE) {
            *lock(&self.active_pipe) = pipe;
        }

        fn deactivate(&self, pipe: HANDLE) {
            let mut active = lock(&self.active_pipe);
            if *active == pipe {
                *active = HANDLE::default();
            }
        }
    }

    impl Drop for NativeForwardPipeFactory {
        fn drop(&mut self) {
            // SAFETY: this factory owns the stop event and closes it after the listener worker is joined.
            let _ = unsafe { CloseHandle(self.stop_event) };
        }
    }

    impl ForwardPipeFactory for NativeForwardPipeFactory {
        fn create(self: Arc<Self>, pipe_name: &str) -> Result<Box<dyn ForwardPipe>, ForwardPipeError> {
            if self.is_canceled() {
                return Err(ForwardPipeError::Canceled);
            }
            let security =
                current_security_descriptor().map_err(|error| ForwardPipeError::Failed(error.to_string()))?;
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: security.as_ptr().0,
                bInheritHandle: false.into(),
            };
            let name = HSTRING::from(format!(r"\\.\pipe\{pipe_name}"));
            // SAFETY: name and attributes are live for the synchronous call; the descriptor is held until pipe creation returns.
            let handle = unsafe {
                CreateNamedPipeW(
                    &name,
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                    server_pipe_mode(),
                    1,
                    0,
                    0,
                    0,
                    Some(&attributes),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(ForwardPipeError::Failed(windows_core::Error::from_thread().to_string()));
            }
            self.activate(handle);
            let pipe: Box<dyn ForwardPipe> = Box::new(NativeForwardPipe {
                factory: Arc::clone(&self),
                handle,
            });
            if self.is_canceled() {
                drop(pipe);
                return Err(ForwardPipeError::Canceled);
            }
            Ok(pipe)
        }

        fn cancel(&self) {
            if self.canceled.swap(true, Ordering::AcqRel) {
                return;
            }
            // SAFETY: the event is a live owned stop event and SetEvent is safe to call concurrently.
            let _ = unsafe { SetEvent(self.stop_event) };
            let active_guard = lock(&self.active_pipe);
            let active = *active_guard;
            if !active.is_invalid() {
                // SAFETY: holding active_pipe prevents its owner from deactivating and closing this handle until cancel returns.
                let _ = unsafe { CancelIoEx(active, None) };
            }
        }

        fn is_canceled(&self) -> bool {
            self.canceled.load(Ordering::Acquire)
        }

        fn wait_retry(&self, delay: Duration) -> bool {
            let milliseconds = delay.as_millis().min(u32::MAX as u128) as u32;
            // SAFETY: stop_event is live and owned; the bounded wait wakes immediately on cancellation.
            let result = unsafe { WaitForSingleObject(self.stop_event, milliseconds) };
            result.0 == WAIT_TIMEOUT.0 && !self.is_canceled()
        }
    }

    struct NativeForwardPipe {
        factory: Arc<NativeForwardPipeFactory>,
        handle: HANDLE,
    }

    // SAFETY: a pipe instance is moved to and used by only the listener worker; the raw handle has one owner.
    unsafe impl Send for NativeForwardPipe {}

    impl NativeForwardPipe {
        fn begin_io(&self) -> Result<(HANDLE, OVERLAPPED), ForwardPipeError> {
            // SAFETY: the event is owned by the returned tuple and remains live until the overlapped operation drains.
            let event = self.factory.operation_event()?;
            let overlapped = OVERLAPPED {
                hEvent: event,
                ..OVERLAPPED::default()
            };
            Ok((event, overlapped))
        }

        fn await_io(
            &self,
            event: HANDLE,
            overlapped: &OVERLAPPED,
            deadline: Option<Instant>,
        ) -> Result<u32, ForwardPipeError> {
            self.factory.wait_operation(self.handle, overlapped, event, deadline)
        }
    }

    impl Drop for NativeForwardPipe {
        fn drop(&mut self) {
            self.factory.deactivate(self.handle);
            // SAFETY: this object owns the server pipe handle; disconnect may fail if no client connected and is ignored.
            let _ = unsafe { DisconnectNamedPipe(self.handle) };
            // SAFETY: the owned pipe handle is closed once after pending operations were drained/canceled.
            let _ = unsafe { CloseHandle(self.handle) };
        }
    }

    impl ForwardPipe for NativeForwardPipe {
        fn accept(&mut self) -> Result<(), ForwardPipeError> {
            let (event, mut overlapped) = self.begin_io()?;
            // SAFETY: pipe is opened for overlapped I/O and the event/OVERLAPPED live until completion is retrieved.
            let connect = unsafe { ConnectNamedPipe(self.handle, Some(&mut overlapped)) };
            let result = match connect {
                Ok(()) => Ok(()),
                Err(error) if error.code().0 == hresult_from_win32(ERROR_PIPE_CONNECTED.0) => Ok(()),
                Err(error) if error.code().0 == hresult_from_win32(ERROR_IO_PENDING.0) => {
                    self.await_io(event, &overlapped, None).map(|_| ())
                }
                Err(error) => Err(ForwardPipeError::Failed(error.to_string())),
            };
            // SAFETY: event was created by begin_io and pending work has completed or was drained on cancellation.
            let _ = unsafe { CloseHandle(event) };
            result
        }

        fn read(&mut self, buffer: &mut [u8], deadline: Instant) -> Result<usize, ForwardPipeError> {
            if Instant::now() >= deadline {
                return Err(ForwardPipeError::TimedOut);
            }
            if buffer.is_empty() {
                return Ok(0);
            }
            let (event, mut overlapped) = self.begin_io()?;
            let mut immediate = 0u32;
            // SAFETY: pipe is overlapped, and buffer plus OVERLAPPED remain valid until completion is retrieved.
            let read = unsafe { ReadFile(self.handle, Some(buffer), Some(&mut immediate), Some(&mut overlapped)) };
            let result = match read {
                Ok(()) => Ok(immediate as usize),
                Err(error) if error.code().0 == hresult_from_win32(ERROR_IO_PENDING.0) => self
                    .await_io(event, &overlapped, Some(deadline))
                    .map(|read| read as usize),
                Err(error) if error.code().0 == hresult_from_win32(ERROR_BROKEN_PIPE.0) => Ok(0),
                Err(error) => Err(ForwardPipeError::Failed(error.to_string())),
            };
            // SAFETY: event was created by begin_io and pending work has completed or was drained on cancellation.
            let _ = unsafe { CloseHandle(event) };
            result
        }

        fn write_all(&mut self, mut bytes: &[u8], deadline: Instant) -> Result<(), ForwardPipeError> {
            while !bytes.is_empty() {
                if Instant::now() >= deadline {
                    return Err(ForwardPipeError::TimedOut);
                }
                let (event, mut overlapped) = self.begin_io()?;
                let mut immediate = 0u32;
                // SAFETY: pipe is overlapped and bytes/OVERLAPPED remain valid until completion is retrieved.
                let write = unsafe { WriteFile(self.handle, Some(bytes), Some(&mut immediate), Some(&mut overlapped)) };
                let written = match write {
                    Ok(()) => Ok(immediate as usize),
                    Err(error) if error.code().0 == hresult_from_win32(ERROR_IO_PENDING.0) => self
                        .await_io(event, &overlapped, Some(deadline))
                        .map(|count| count as usize),
                    Err(error) => Err(ForwardPipeError::Failed(error.to_string())),
                };
                // SAFETY: event was created by begin_io and pending work has completed or was drained on cancellation.
                let _ = unsafe { CloseHandle(event) };
                let written = written?;
                if written == 0 {
                    return Err(ForwardPipeError::Failed("named pipe write made no progress".to_owned()));
                }
                bytes = &bytes[written..];
            }
            Ok(())
        }
    }

    fn hresult_from_win32(code: u32) -> i32 {
        if code == 0 {
            0
        } else {
            ((code & 0xFFFF) | 0x8007_0000) as i32
        }
    }

    use windows::Win32::Foundation::{ERROR_BROKEN_PIPE, ERROR_PIPE_CONNECTED};

    pub(super) use NativeForwardPipeFactory as Factory;
}

#[cfg(windows)]
use native_pipe::Factory as NativeForwardPipeFactory;
