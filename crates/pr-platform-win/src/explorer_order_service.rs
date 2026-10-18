//! Port of the scheduling/lifecycle part of `PhotoReview.Platform.Windows/Explorer/ExplorerOrderService.cs`
//! (C# baseline d92d6aae). W6-01d supplies the synchronous native query through [`ExplorerQuery`].

use std::any::Any;
use std::error::Error;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use pr_core::abstractions::{ExplorerOrderProvider, Log, NullLog};
use pr_core::catalog::explorer_reason;
use pr_core::catalog::explorer_types::{ExplorerOrderStatus, ExplorerQueryProgress, ExplorerViewSnapshot};
use pr_core::catalog::snapshot_validator::{canonicalize_folder, same_path};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;

pub(crate) const DEFAULT_PREFETCH_MAX_AGE: Duration = Duration::from_secs(30);
const SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_millis(200);
// CancelToken is an atomic flag without a wait/notification primitive. The caller sleeps on the channel for this
// bounded interval between checks, so cancellation stays responsive without spinning or fixed-delay test asserts.
const CANCEL_CHECK_INTERVAL: Duration = Duration::from_millis(10);
const TIMER_MAX_MS: u128 = u32::MAX as u128 - 1;
const CLASS_NOT_REGISTERED: i32 = 0x8004_0154_u32 as i32;

/// Synchronous native view query executed only on the dedicated pump thread. The callback is created on that thread
/// and forwards progress to the waiting caller; the borrowed caller callback never crosses a thread boundary.
pub(crate) type ExplorerQuery = dyn for<'a> Fn(
        &str,
        Option<&'a (dyn Fn(ExplorerQueryProgress) + Sync)>,
        usize,
        &CancelToken,
    ) -> Result<ExplorerViewSnapshot, QueryError>
    + Send;

/// Failure crossing the narrow W6-01c/W6-01d seam. `shell_open` distinguishes the two shell activation outcomes
/// handled specially by the C# provider; errors from a selected window remain native-view outcomes in W6-01b.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueryError {
    pub(crate) type_name: String,
    pub(crate) message: String,
    pub(crate) hresult: Option<i32>,
    pub(crate) shell_open: bool,
}

impl QueryError {
    pub(crate) fn new(type_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            type_name: type_name.into(),
            message: message.into(),
            hresult: None,
            shell_open: false,
        }
    }

    pub(crate) fn shell_open(hresult: i32) -> Self {
        Self {
            type_name: "COMException".into(),
            message: format!("shell activation failed (HRESULT 0x{:08X})", hresult as u32),
            hresult: Some(hresult),
            shell_open: true,
        }
    }

    pub(crate) fn canceled(message: impl Into<String>) -> Self {
        Self {
            type_name: "OperationCanceledException".into(),
            message: message.into(),
            hresult: None,
            shell_open: false,
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl Error for QueryError {}

pub(crate) trait MonotonicClock: Send + Sync {
    fn timestamp(&self) -> Duration;
}

#[derive(Clone)]
pub(crate) struct SystemMonotonicClock(Instant);
impl Default for SystemMonotonicClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}
impl MonotonicClock for SystemMonotonicClock {
    fn timestamp(&self) -> Duration {
        self.0.elapsed()
    }
}

enum PumpMessage {
    Query(QueryRequest),
    Shutdown,
}
struct QueryRequest {
    folder: String,
    batch_size: usize,
    token: CancelToken,
    timeout_expired: Arc<AtomicBool>,
    events: Sender<QueryEvent>,
    progress: bool,
    timer_stop: Option<Sender<()>>,
    cleanup: Arc<CleanupSignal>,
}
enum QueryEvent {
    Progress(ExplorerQueryProgress),
    Complete(ExplorerViewSnapshot),
}

struct CleanupSignal(Mutex<bool>, std::sync::Condvar);
impl CleanupSignal {
    fn new() -> Self {
        Self(Mutex::new(false), std::sync::Condvar::new())
    }
    fn complete(&self) {
        *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.1.notify_all();
    }
    #[cfg(test)]
    fn wait(&self, timeout: Duration) -> bool {
        let Ok(guard) = self.0.lock() else { return false };
        if *guard {
            return true;
        }
        self.1
            .wait_timeout_while(guard, timeout, |finished| !*finished)
            .map(|(guard, _)| *guard)
            .unwrap_or(false)
    }
}

struct StaThreadPump {
    sender: Mutex<Option<Sender<PumpMessage>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    exited: Mutex<Receiver<()>>,
    disposed: Arc<AtomicBool>,
    log: Arc<dyn Log>,
}

impl StaThreadPump {
    fn new(query: Box<ExplorerQuery>, log: Arc<dyn Log>) -> Result<Self, PumpStartError> {
        let (sender, receiver) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let (exited_tx, exited) = mpsc::channel();
        let thread_log = Arc::clone(&log);
        let thread = thread::Builder::new()
            .name("PhotoReview Explorer view".into())
            .spawn(move || {
                let initialized = initialize_sta();
                if let Err(hr) = initialized {
                    let _ = startup_tx.send(Err(hr));
                    return;
                }
                let _ = startup_tx.send(Ok(()));
                pump_loop(receiver, query, thread_log);
                uninitialize_sta();
                let _ = exited_tx.send(());
            })
            .map_err(PumpStartError::ThreadSpawn)?;
        match startup_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                sender: Mutex::new(Some(sender)),
                thread: Mutex::new(Some(thread)),
                exited: Mutex::new(exited),
                disposed: Arc::new(AtomicBool::new(false)),
                log,
            }),
            Ok(Err(hr)) => {
                let _ = thread.join();
                Err(PumpStartError::ComInitialization(hr))
            }
            Err(_) => {
                let _ = thread.join();
                Err(PumpStartError::StartupChannelClosed)
            }
        }
    }

    fn enqueue(&self, request: QueryRequest) -> Result<(), QueryRequest> {
        let sender = self.sender.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(sender) = sender.as_ref() else {
            return Err(request);
        };
        sender.send(PumpMessage::Query(request)).map_err(|error| match error.0 {
            PumpMessage::Query(request) => request,
            PumpMessage::Shutdown => unreachable!(),
        })
    }

    fn dispose(&self) {
        if self.disposed.swap(true, Ordering::AcqRel) {
            return;
        }
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(sender) = sender {
            let _ = sender.send(PumpMessage::Shutdown);
        }
        if self
            .exited
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .recv_timeout(SHUTDOWN_JOIN_TIMEOUT)
            .is_ok()
        {
            if let Some(thread) = self
                .thread
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                let _ = thread.join();
            }
        } else {
            self.log.error("Explorer STA pump thread did not exit within 200 ms; leaking queue instead of risking a stuck native call", None);
        }
    }
}

impl Drop for StaThreadPump {
    fn drop(&mut self) {
        self.dispose();
    }
}

fn pump_loop(receiver: Receiver<PumpMessage>, query: Box<ExplorerQuery>, log: Arc<dyn Log>) {
    while let Ok(message) = receiver.recv() {
        match message {
            PumpMessage::Shutdown => break,
            PumpMessage::Query(request) => execute_query(request, query.as_ref(), log.as_ref()),
        }
    }
}

fn execute_query(request: QueryRequest, query: &ExplorerQuery, log: &dyn Log) {
    let QueryRequest {
        folder,
        batch_size,
        token,
        timeout_expired,
        events,
        progress,
        timer_stop,
        cleanup,
    } = request;
    let result = if token.is_canceled() {
        Ok(unavailable(
            &folder,
            ExplorerOrderStatus::Canceled,
            explorer_reason::CANCELED,
        ))
    } else {
        let progress_callback = |value| {
            let _ = events.send(QueryEvent::Progress(value));
        };
        let callback = progress.then_some(&progress_callback as &(dyn Fn(ExplorerQueryProgress) + Sync));
        match catch_unwind(AssertUnwindSafe(|| query(&folder, callback, batch_size, &token))) {
            Ok(result) => result,
            Err(payload) => Err(QueryError::new(
                panic_type_name(payload.as_ref()),
                panic_message(payload.as_ref()),
            )),
        }
    };
    let snapshot = match result {
        Ok(snapshot) => resolve_cancellation(snapshot, false, false, timeout_expired.load(Ordering::Acquire)),
        Err(error) if error.type_name == "OperationCanceledException" => resolve_cancellation(
            unavailable(&folder, ExplorerOrderStatus::Canceled, explorer_reason::CANCELED),
            false,
            false,
            timeout_expired.load(Ordering::Acquire),
        ),
        Err(error) if error.shell_open && error.hresult == Some(CLASS_NOT_REGISTERED) => unavailable(
            &folder,
            ExplorerOrderStatus::NativeViewUnavailable,
            explorer_reason::SHELL_UNAVAILABLE,
        ),
        Err(error) => {
            log.error("Explorer native view query failed", Some(&error));
            unavailable(
                &folder,
                ExplorerOrderStatus::Failed,
                &format!("{}|{}", explorer_reason::QUERY_FAILED, error.type_name),
            )
        }
    };
    let _ = events.send(QueryEvent::Complete(snapshot));
    if let Some(timer_stop) = timer_stop {
        let _ = timer_stop.send(());
    }
    cleanup.complete();
}

fn panic_type_name(_payload: &(dyn Any + Send)) -> &'static str {
    "PanicException"
}
fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|value| (*value).to_owned()))
        .unwrap_or_else(|| "query panicked".into())
}

#[derive(Debug)]
pub(crate) enum PumpStartError {
    ThreadSpawn(std::io::Error),
    ComInitialization(i32),
    StartupChannelClosed,
}
impl fmt::Display for PumpStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ThreadSpawn(error) => write!(f, "could not start Explorer STA thread: {error}"),
            Self::ComInitialization(hr) => write!(f, "CoInitializeEx failed (HRESULT 0x{:08X})", *hr as u32),
            Self::StartupChannelClosed => f.write_str("Explorer STA startup channel closed"),
        }
    }
}
impl Error for PumpStartError {}

#[cfg(windows)]
#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut std::ffi::c_void, coinit: u32) -> i32;
    fn CoUninitialize();
}

#[cfg(windows)]
fn initialize_sta() -> Result<(), i32> {
    // SAFETY: called once on the newly-created pump thread with the documented STA apartment flag and null reserved pointer.
    let hr = unsafe { CoInitializeEx(std::ptr::null_mut(), 0x2) };
    if hr < 0 { Err(hr) } else { Ok(()) }
}
#[cfg(not(windows))]
fn initialize_sta() -> Result<(), i32> {
    Ok(())
}
#[cfg(windows)]
fn uninitialize_sta() {
    // SAFETY: balances the successful CoInitializeEx call on this same pump thread.
    unsafe { CoUninitialize() }
}
#[cfg(not(windows))]
fn uninitialize_sta() {}

struct PrefetchedQuery {
    folder: String,
    events: Receiver<QueryEvent>,
    token: CancelToken,
    timeout_expired: Arc<AtomicBool>,
    started: Duration,
    cleanup: Arc<CleanupSignal>,
}

struct WaitContext<'a> {
    folder: &'a str,
    events: &'a Receiver<QueryEvent>,
    token: &'a CancelToken,
    timeout_expired: &'a AtomicBool,
    timeout: Duration,
    started_at: Instant,
    caller: &'a CancelToken,
    progress: Option<&'a (dyn Fn(ExplorerQueryProgress) + Sync)>,
}
impl PrefetchedQuery {
    fn cancel(&self) {
        self.token.cancel();
    }
}

/// The injectable provider seam used by W6-01d. Calls are synchronous at this boundary; all native work runs on one STA.
pub(crate) struct ExplorerOrderService {
    log: Arc<dyn Log>,
    pump: StaThreadPump,
    clock: Arc<dyn MonotonicClock>,
    prefetch_max_age: Duration,
    prefetch: Mutex<Option<PrefetchedQuery>>,
    disposed: Arc<AtomicBool>,
    before_prefetch_store: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl ExplorerOrderService {
    pub(crate) fn with_query(query: Box<ExplorerQuery>, log: Option<Arc<dyn Log>>) -> Result<Self, PumpStartError> {
        Self::with_seams(
            query,
            log,
            Arc::new(SystemMonotonicClock::default()),
            DEFAULT_PREFETCH_MAX_AGE,
        )
    }

    pub(crate) fn with_seams(
        query: Box<ExplorerQuery>,
        log: Option<Arc<dyn Log>>,
        clock: Arc<dyn MonotonicClock>,
        prefetch_max_age: Duration,
    ) -> Result<Self, PumpStartError> {
        let log = log.unwrap_or_else(|| Arc::new(NullLog));
        let pump = StaThreadPump::new(query, Arc::clone(&log))?;
        Ok(Self {
            log,
            pump,
            clock,
            prefetch_max_age,
            prefetch: Mutex::new(None),
            disposed: Arc::new(AtomicBool::new(false)),
            before_prefetch_store: Mutex::new(None),
        })
    }

    fn take_prefetch(&self) -> Option<PrefetchedQuery> {
        self.prefetch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    #[cfg(test)]
    pub(crate) fn set_before_prefetch_store_hook(&self, hook: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self
            .before_prefetch_store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = hook;
    }

    #[cfg(test)]
    pub(crate) fn wait_prefetch_cleanup(&self, timeout: Duration) -> bool {
        self.prefetch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|prefetch| prefetch.cleanup.wait(timeout))
    }

    fn start_query(
        &self,
        folder: String,
        batch_size: usize,
        progress: bool,
        prefetch_timeout: Option<Duration>,
    ) -> (Receiver<QueryEvent>, CancelToken, Arc<AtomicBool>, Arc<CleanupSignal>) {
        let (events_tx, events_rx) = mpsc::channel();
        let token = CancelToken::new();
        let timeout_expired = Arc::new(AtomicBool::new(false));
        let cleanup = Arc::new(CleanupSignal::new());
        let timer_stop = if let Some(timeout) = prefetch_timeout {
            let (tx, rx) = mpsc::channel();
            if let Some(duration) = normalize_timeout(timeout) {
                let timer_token = token.clone();
                let timer_expired = Arc::clone(&timeout_expired);
                if duration.is_zero() {
                    timer_expired.store(true, Ordering::Release);
                    timer_token.cancel();
                    drop(rx);
                } else {
                    let _ = thread::Builder::new()
                        .name("PhotoReview Explorer prefetch timeout".into())
                        .spawn(move || {
                            if timer_stop_rx_timeout(&rx, duration) {
                                timer_expired.store(true, Ordering::Release);
                                timer_token.cancel();
                            }
                        });
                }
            } else {
                drop(rx);
            }
            Some(tx)
        } else {
            None
        };
        let request = QueryRequest {
            folder,
            batch_size,
            token: token.clone(),
            timeout_expired: Arc::clone(&timeout_expired),
            events: events_tx,
            progress,
            timer_stop,
            cleanup: Arc::clone(&cleanup),
        };
        if let Err(request) = self.pump.enqueue(request) {
            request.cleanup.complete();
            let _ = request.events.send(QueryEvent::Complete(unavailable(
                &request.folder,
                ExplorerOrderStatus::Canceled,
                explorer_reason::CANCELED,
            )));
        }
        (events_rx, token, timeout_expired, cleanup)
    }

    fn wait_for_query(&self, context: WaitContext<'_>) -> ExplorerViewSnapshot {
        let WaitContext {
            folder,
            events,
            token,
            timeout_expired,
            timeout,
            started_at,
            caller,
            progress,
        } = context;
        let deadline = normalize_timeout(timeout).and_then(|duration| started_at.checked_add(duration));
        loop {
            match events.try_recv() {
                Ok(QueryEvent::Progress(value)) => {
                    if let Some(progress) = progress {
                        progress(value);
                    }
                    continue;
                }
                Ok(QueryEvent::Complete(snapshot)) => {
                    return resolve_cancellation(
                        snapshot,
                        self.disposed.load(Ordering::Acquire),
                        caller.is_canceled(),
                        timeout_expired.load(Ordering::Acquire),
                    );
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return unavailable(
                        folder,
                        ExplorerOrderStatus::Failed,
                        &format!("{}|ChannelClosed", explorer_reason::QUERY_FAILED),
                    );
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if caller.is_canceled() {
                token.cancel();
                return unavailable(folder, ExplorerOrderStatus::Canceled, explorer_reason::CANCELED);
            }
            if self.disposed.load(Ordering::Acquire) {
                token.cancel();
                return unavailable(folder, ExplorerOrderStatus::Canceled, explorer_reason::CANCELED);
            }
            let remaining = match deadline {
                Some(deadline) => match deadline.checked_duration_since(Instant::now()) {
                    Some(remaining) if !remaining.is_zero() => remaining.min(CANCEL_CHECK_INTERVAL),
                    _ => {
                        timeout_expired.store(true, Ordering::Release);
                        token.cancel();
                        return unavailable(folder, ExplorerOrderStatus::TimedOut, explorer_reason::TIMEOUT);
                    }
                },
                None => CANCEL_CHECK_INTERVAL,
            };
            match events.recv_timeout(remaining) {
                Ok(QueryEvent::Progress(value)) => {
                    if let Some(progress) = progress {
                        progress(value);
                    }
                }
                Ok(QueryEvent::Complete(snapshot)) => {
                    return resolve_cancellation(
                        snapshot,
                        self.disposed.load(Ordering::Acquire),
                        caller.is_canceled(),
                        timeout_expired.load(Ordering::Acquire),
                    );
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return unavailable(
                        folder,
                        ExplorerOrderStatus::Failed,
                        &format!("{}|ChannelClosed", explorer_reason::QUERY_FAILED),
                    );
                }
            }
        }
    }

    fn invalid_folder(&self, folder: &str) -> ExplorerViewSnapshot {
        unavailable(
            folder,
            ExplorerOrderStatus::Failed,
            &format!("{}|ArgumentException", explorer_reason::QUERY_FAILED),
        )
    }

    pub(crate) fn dispose(&self) {
        if self.disposed.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(prefetch) = self.take_prefetch() {
            prefetch.cancel();
        }
        self.pump.dispose();
    }
}

impl ExplorerOrderProvider for ExplorerOrderService {
    fn try_get_snapshot_progressive(
        &self,
        folder: &Path,
        timeout: Duration,
        progress: Option<&(dyn Fn(ExplorerQueryProgress) + Sync)>,
        batch_size: usize,
        cancel: &CancelToken,
    ) -> ExplorerViewSnapshot {
        let folder_text = folder.to_string_lossy();
        let prefetch = self.take_prefetch();
        let Ok(canonical) = canonicalize_folder(&folder_text) else {
            if let Some(prefetch) = prefetch {
                prefetch.cancel();
            }
            return self.invalid_folder(&folder_text);
        };
        if let Some(prefetch) = prefetch {
            let equivalent = same_path(&prefetch.folder, &canonical).unwrap_or(false);
            let fresh = self.clock.timestamp().saturating_sub(prefetch.started) <= self.prefetch_max_age;
            if equivalent && fresh {
                let snapshot = self.wait_for_query(WaitContext {
                    folder: &prefetch.folder,
                    events: &prefetch.events,
                    token: &prefetch.token,
                    timeout_expired: &prefetch.timeout_expired,
                    timeout,
                    started_at: Instant::now(),
                    caller: cancel,
                    progress,
                });
                if cancel.is_canceled() || self.disposed.load(Ordering::Acquire) {
                    prefetch.cancel();
                }
                return snapshot;
            }
            prefetch.cancel();
        }
        if cancel.is_canceled() || self.disposed.load(Ordering::Acquire) {
            return unavailable(&canonical, ExplorerOrderStatus::Canceled, explorer_reason::CANCELED);
        }
        let started_at = Instant::now();
        let immediate_timeout = timeout.is_zero().then_some(Duration::ZERO);
        let (events, token, timeout_expired, _cleanup) = self.start_query(
            canonical.clone(),
            batch_size.clamp(1, 128),
            progress.is_some(),
            immediate_timeout,
        );
        self.wait_for_query(WaitContext {
            folder: &canonical,
            events: &events,
            token: &token,
            timeout_expired: &timeout_expired,
            timeout,
            started_at,
            caller: cancel,
            progress,
        })
    }

    fn prefetch(&self, folder: &Path, timeout: Duration) {
        if self.disposed.load(Ordering::Acquire) {
            return;
        }
        let folder_text = folder.to_string_lossy();
        let Ok(canonical) = canonicalize_folder(&folder_text) else {
            self.log.warn("Explorer prefetch skipped: folder is not a usable path");
            return;
        };
        let started = self.clock.timestamp();
        let (events, token, timeout_expired, cleanup) = self.start_query(canonical.clone(), 16, false, Some(timeout));
        if let Some(hook) = self
            .before_prefetch_store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            hook();
        }
        let mut guard = self.prefetch.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.disposed.load(Ordering::Acquire) {
            token.cancel();
            return;
        }
        let superseded = guard.replace(PrefetchedQuery {
            folder: canonical.clone(),
            events,
            token,
            timeout_expired,
            started,
            cleanup,
        });
        drop(guard);
        if let Some(superseded) = superseded {
            superseded.cancel();
        }
        self.log.info(&format!("Explorer prefetch-start: folder={canonical}"));
    }
}

impl Drop for ExplorerOrderService {
    fn drop(&mut self) {
        self.dispose();
    }
}

/// Maps a Canceled snapshot to TimedOut only when the provider's own timer fired; caller cancellation/disposal win.
pub(crate) fn resolve_cancellation(
    snapshot: ExplorerViewSnapshot,
    disposed: bool,
    caller_canceled: bool,
    timeout_expired: bool,
) -> ExplorerViewSnapshot {
    if snapshot.status == ExplorerOrderStatus::Canceled && timeout_expired && !caller_canceled && !disposed {
        unavailable(
            &snapshot.folder,
            ExplorerOrderStatus::TimedOut,
            explorer_reason::TIMEOUT,
        )
    } else {
        snapshot
    }
}

/// A Rust `Duration` cannot be negative or infinite. Zero means expired; values beyond C#'s CancelAfter limit map to no deadline.
pub(crate) fn normalize_timeout(timeout: Duration) -> Option<Duration> {
    if timeout.as_millis() > TIMER_MAX_MS {
        None
    } else {
        Some(timeout)
    }
}

fn timer_stop_rx_timeout(receiver: &Receiver<()>, timeout: Duration) -> bool {
    matches!(receiver.recv_timeout(timeout), Err(RecvTimeoutError::Timeout))
}

fn unavailable(folder: &str, status: ExplorerOrderStatus, reason: &str) -> ExplorerViewSnapshot {
    ExplorerViewSnapshot {
        folder: folder.to_owned(),
        ordered_paths: Vec::new(),
        sort_columns: Vec::new(),
        group_state: pr_core::catalog::explorer_types::ExplorerGroupState::Unknown,
        status,
        reason: Some(reason.to_owned()),
        captured_utc: DateTimeUtc::now(),
    }
}
