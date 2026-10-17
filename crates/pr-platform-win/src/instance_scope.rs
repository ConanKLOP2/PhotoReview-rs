//! Port of `PhotoReview.Platform.Windows/InstanceScope.cs` (C# baseline d92d6aae).
//!
//! The scope keeps each named mutex paired with its forwarding listener and follows the folders being opened and shown.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use pr_core::abstractions::{Log, NullLog};
use pr_core::instance::forwarding::InstanceForwardClient;
use pr_core::instance::handoff;
use pr_core::instance::ownership::{FolderOpenDecision, FolderOwnership};
use pr_core::model::InstanceMode;
use pr_core::sync::{CancelToken, Canceled};

use crate::forward_client::ForwardPipeClient;
use crate::forward_server::InstanceForwardServer;
use crate::instance_keys::{self, DEFAULT_PREFIX};

type PathsCallback = dyn Fn(&[String]) + Send + Sync + 'static;
type ClientFactory = dyn Fn(&str) -> Result<Box<dyn InstanceForwardClient>, InstanceScopeError> + Send + Sync;
type ClaimFactory = dyn Fn(&str, &str, Arc<PathsCallback>, Arc<dyn Log>)
        -> Result<Option<Arc<dyn ScopeClaim>>, InstanceScopeError>
    + Send
    + Sync;

const RELEASE_WAIT_STEP: Duration = Duration::from_millis(10);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Result of attempting to claim the mutex that covers a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceClaimResult {
    /// This scope already owns the claim.
    AlreadyHeld,
    /// The mutex and corresponding listener were created by this scope.
    Acquired,
    /// Another process or incompatible named kernel object owns the name.
    OwnedElsewhere,
}

/// Construction or native setup error for [`InstanceScope`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceScopeError {
    message: String,
}

impl std::fmt::Display for InstanceScopeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InstanceScopeError {}

/// Owns the process's instance mutexes and matching forward pipes.
pub struct InstanceScope {
    mode: InstanceMode,
    prefix: String,
    callback: Arc<PathsCallback>,
    log: Arc<dyn Log>,
    client_factory: Arc<ClientFactory>,
    claim_factory: Arc<ClaimFactory>,
    forward_timeout: Duration,
    gate: Mutex<ScopeState>,
    release_state: Arc<(Mutex<usize>, Condvar)>,
}

#[derive(Default)]
struct ScopeState {
    claims: HashMap<String, Arc<dyn ScopeClaim>>,
    pending: HashMap<String, usize>,
    disposed: bool,
}

impl InstanceScope {
    /// Creates a scope with the production forwarding client and default object-name prefix.
    pub fn new(
        mode: InstanceMode,
        on_forwarded_paths: impl Fn(&[String]) + Send + Sync + 'static,
    ) -> Result<Self, InstanceScopeError> {
        Self::with_options(mode, on_forwarded_paths, None, DEFAULT_PREFIX, None, false)
    }

    /// Creates a scope with explicit options. Tests may supply a client factory and unique prefix.
    pub fn with_options(
        mode: InstanceMode,
        on_forwarded_paths: impl Fn(&[String]) + Send + Sync + 'static,
        log: Option<Arc<dyn Log>>,
        prefix: &str,
        forward_timeout: Option<Duration>,
        allow_server_foreground: bool,
    ) -> Result<Self, InstanceScopeError> {
        if prefix.trim().is_empty() {
            return Err(InstanceScopeError { message: "name prefix cannot be empty or whitespace".to_owned() });
        }
        let log = log.unwrap_or_else(|| Arc::new(NullLog));
        let factory: Arc<ClientFactory> = Arc::new(move |pipe_name| {
            let client = ForwardPipeClient::new(Some(pipe_name.to_owned()), allow_server_foreground)
                .map_err(|error| InstanceScopeError { message: error.to_string() })?;
            Ok(Box::new(client) as Box<dyn InstanceForwardClient>)
        });
        let claim_factory: Arc<ClaimFactory> = Arc::new(|mutex, pipe, callback, log| {
            Ok(HeldClaim::try_create(mutex, pipe, callback, log)?
                .map(|claim| Arc::new(claim) as Arc<dyn ScopeClaim>))
        });
        Self::with_factories(
            mode,
            on_forwarded_paths,
            prefix,
            forward_timeout.unwrap_or(handoff::DEFAULT_TIMEOUT),
            log,
            factory,
            claim_factory,
        )
    }

    fn with_factories(
        mode: InstanceMode,
        on_forwarded_paths: impl Fn(&[String]) + Send + Sync + 'static,
        prefix: &str,
        forward_timeout: Duration,
        log: Arc<dyn Log>,
        client_factory: Arc<ClientFactory>,
        claim_factory: Arc<ClaimFactory>,
    ) -> Result<Self, InstanceScopeError> {
        if prefix.trim().is_empty() {
            return Err(InstanceScopeError { message: "name prefix cannot be empty or whitespace".to_owned() });
        }
        Ok(Self {
            mode,
            prefix: prefix.to_owned(),
            callback: Arc::new(on_forwarded_paths),
            log,
            client_factory,
            claim_factory,
            forward_timeout,
            gate: Mutex::new(ScopeState::default()),
            release_state: Arc::new((Mutex::new(0), Condvar::new())),
        })
    }

    /// Mode configured for this scope.
    pub fn mode(&self) -> InstanceMode {
        self.mode
    }

    /// Computes the paired mutex and pipe names for a folder.
    pub fn keys_for(&self, folder: Option<&str>) -> Result<(String, String), InstanceScopeError> {
        let mutex = instance_keys::mutex_name_for(self.mode, folder, &self.prefix)
            .map_err(|message| InstanceScopeError { message: message.to_owned() })?;
        let pipe = crate::forward_pipe::current_pipe_name(&mutex, &self.prefix);
        Ok((mutex, pipe))
    }

    /// Startup helper: returns false only if another owner holds the corresponding lock.
    pub fn try_acquire(&self, folder: Option<&str>) -> Result<bool, InstanceScopeError> {
        Ok(self.claim(folder)? != InstanceClaimResult::OwnedElsewhere)
    }

    /// Creates a client for the pipe belonging to the owner of `folder`.
    pub fn create_client(&self, folder: Option<&str>) -> Result<Box<dyn InstanceForwardClient>, InstanceScopeError> {
        let (_, pipe) = self.keys_for(folder)?;
        (self.client_factory)(&pipe)
    }

    /// Claims the mutex and starts its matching listener if the name is free.
    pub fn claim(&self, folder: Option<&str>) -> Result<InstanceClaimResult, InstanceScopeError> {
        let (mutex, pipe) = self.keys_for(folder)?;
        self.claim_keys(&mutex, &pipe, false)
    }

    /// Whether this scope currently holds the mutex that covers `folder`.
    pub fn holds(&self, folder: Option<&str>) -> Result<bool, InstanceScopeError> {
        let (mutex, _) = self.keys_for(folder)?;
        Ok(lock(&self.gate).claims.contains_key(&mutex))
    }

    fn claim_keys(
        &self,
        mutex_name: &str,
        pipe_name: &str,
        mark_pending: bool,
    ) -> Result<InstanceClaimResult, InstanceScopeError> {
        let mut state = lock(&self.gate);
        if state.disposed && mark_pending {
            return Ok(InstanceClaimResult::AlreadyHeld);
        }
        if state.disposed {
            return Err(InstanceScopeError { message: "instance scope is disposed".to_owned() });
        }
        if !state.claims.contains_key(mutex_name) {
            let Some(claim) = (self.claim_factory)(
                mutex_name,
                pipe_name,
                Arc::clone(&self.callback),
                Arc::clone(&self.log),
            )? else {
                return Ok(InstanceClaimResult::OwnedElsewhere);
            };
            state.claims.insert(mutex_name.to_owned(), claim);
            if mark_pending {
                *state.pending.entry(mutex_name.to_owned()).or_default() += 1;
            }
            return Ok(InstanceClaimResult::Acquired);
        }
        if mark_pending {
            *state.pending.entry(mutex_name.to_owned()).or_default() += 1;
        }
        Ok(InstanceClaimResult::AlreadyHeld)
    }

    fn wait_for_releases(&self, cancel: &CancelToken) -> Result<(), Canceled> {
        let (count, changed) = &*self.release_state;
        let mut active = lock(count);
        while *active != 0 {
            cancel.check()?;
            let (next, _) = changed
                .wait_timeout(active, RELEASE_WAIT_STEP)
                .unwrap_or_else(PoisonError::into_inner);
            active = next;
        }
        cancel.check()
    }

    fn release_except(&self, keep_key: &str, finished_open: Option<&str>) {
        let released = {
            let mut state = lock(&self.gate);
            if state.disposed {
                return;
            }
            if let Some(key) = finished_open {
                if let Some(count) = state.pending.get_mut(key) {
                    if *count <= 1 {
                        state.pending.remove(key);
                    } else {
                        *count -= 1;
                    }
                }
            }
            let keys: Vec<String> = state
                .claims
                .keys()
                .filter(|key| key.as_str() != keep_key && !state.pending.contains_key(*key))
                .cloned()
                .collect();
            let released = keys.into_iter().filter_map(|key| state.claims.remove(&key)).collect::<Vec<_>>();
            if !released.is_empty() {
                // Publish an in-flight release before unlocking the claim map so a concurrent reopen cannot
                // mistake this scope's still-open mutex handle for a different process.
                let (active, _) = &*self.release_state;
                *lock(active) += 1;
            }
            released
        };
        if released.is_empty() {
            return;
        }

        let release_state = Arc::clone(&self.release_state);
        let logger = Arc::clone(&self.log);
        let spawn = std::thread::Builder::new().name("PhotoReviewRs-instance-release".to_owned()).spawn(move || {
            for claim in released {
                let name = claim.mutex_name().to_owned();
                if let Err(error) = claim.dispose() {
                    logger.error(&format!("Instance lock release failed: {name}"), Some(&error));
                }
            }
            let (active, changed) = &*release_state;
            let mut count = lock(active);
            *count = count.saturating_sub(1);
            changed.notify_all();
        });
        if let Err(error) = spawn {
            self.log.error("Could not start instance lock release worker", Some(&error));
            // The failed spawn drops its closure, which drops each claim; its destructor preserves server-before-mutex order.
            let (active, changed) = &*self.release_state;
            let mut count = lock(active);
            *count = count.saturating_sub(1);
            changed.notify_all();
        }
    }

    /// Stops pipe listeners while keeping their mutexes open.
    pub fn stop_listening(&self) {
        let claims = {
            let state = lock(&self.gate);
            state.claims.values().cloned().collect::<Vec<_>>()
        };
        for claim in claims {
            claim.stop_listening();
        }
    }

    /// Disposes every listener and mutex independently. Repeated calls are harmless.
    pub fn dispose(&self) {
        let claims = {
            let mut state = lock(&self.gate);
            if state.disposed {
                return;
            }
            state.disposed = true;
            state.pending.clear();
            state.claims.drain().map(|(_, claim)| claim).collect::<Vec<_>>()
        };
        for claim in claims {
            let name = claim.mutex_name().to_owned();
            if let Err(error) = claim.dispose() {
                self.log.warn(&format!("Instance claim dispose failed for {name}: {error}"));
            }
        }
    }
}

impl FolderOwnership for InstanceScope {
    fn before_open(
        &self,
        folder: &str,
        initial_path: Option<&str>,
        cancel: &CancelToken,
    ) -> Result<FolderOpenDecision, Canceled> {
        if folder.trim().is_empty() {
            return Ok(FolderOpenDecision::OwnedByOtherInstance);
        }
        let (mutex, pipe) = self.keys_for(Some(folder)).map_err(|_| Canceled)?;
        let mut claim = self.claim_keys(&mutex, &pipe, true).map_err(|_| Canceled)?;
        if claim == InstanceClaimResult::OwnedElsewhere {
            let (active, _) = &*self.release_state;
            if *lock(active) != 0 {
                self.wait_for_releases(cancel)?;
                claim = self.claim_keys(&mutex, &pipe, true).map_err(|_| Canceled)?;
            }
        }
        if claim != InstanceClaimResult::OwnedElsewhere {
            return Ok(FolderOpenDecision::Proceed);
        }

        let path = initial_path.unwrap_or(folder);
        if path.contains('\0') {
            self.log.warn("Folder switch refused: another instance owns the folder and the path is not forwardable");
            return Ok(FolderOpenDecision::OwnedByOtherInstance);
        }
        let target = match std::path::absolute(path) {
            Ok(path) => path.to_string_lossy().into_owned(),
            Err(_) => {
                self.log.warn("Folder switch refused: another instance owns the folder and the path is not forwardable");
                return Ok(FolderOpenDecision::OwnedByOtherInstance);
            }
        };
        let client = match (self.client_factory)(&pipe) {
            Ok(client) => client,
            Err(error) => {
                self.log.warn(&format!("Forwarding to the running instance failed: {error}"));
                return Ok(FolderOpenDecision::OwnedByOtherInstance);
            }
        };
        if handoff::try_forward(client.as_ref(), &[target], self.forward_timeout, Some(self.log.as_ref()), cancel)? {
            return Ok(FolderOpenDecision::ForwardedToOtherInstance);
        }
        if self.claim_keys(&mutex, &pipe, true).map_err(|_| Canceled)? != InstanceClaimResult::OwnedElsewhere {
            return Ok(FolderOpenDecision::Proceed);
        }
        self.log.warn("Folder switch refused: another instance owns the folder and did not answer");
        Ok(FolderOpenDecision::OwnedByOtherInstance)
    }

    fn on_folder_shown(&self, folder: &str) {
        if let Ok((mutex, _)) = self.keys_for(Some(folder)) {
            self.release_except(&mutex, None);
        }
    }

    fn after_open(&self, folder: &str, shown_folder: Option<&str>) {
        let Ok((finished, _)) = self.keys_for(Some(folder)) else { return };
        let keep = shown_folder.and_then(|folder| self.keys_for(Some(folder)).ok()).map_or_else(
            || self.keys_for(None).map(|(mutex, _)| mutex).unwrap_or_default(),
            |(mutex, _)| mutex,
        );
        self.release_except(&keep, Some(&finished));
    }
}

impl Drop for InstanceScope {
    fn drop(&mut self) {
        self.dispose();
    }
}

trait ScopeClaim: Send + Sync {
    fn mutex_name(&self) -> &str;
    fn stop_listening(&self);
    fn dispose(&self) -> Result<(), InstanceScopeError>;
}

struct HeldClaim {
    mutex_name: String,
    mutex: Mutex<OwnedKernelHandle>,
    server: Arc<InstanceForwardServer>,
}

impl HeldClaim {
    fn try_create(
        mutex_name: &str,
        pipe_name: &str,
        callback: Arc<PathsCallback>,
        log: Arc<dyn Log>,
    ) -> Result<Option<Self>, InstanceScopeError> {
        let Some(mutex) = OwnedKernelHandle::create_mutex(mutex_name)? else {
            return Ok(None);
        };
        let callback_for_server = Arc::clone(&callback);
        let server = match InstanceForwardServer::new(
            pipe_name,
            move |paths| callback_for_server(paths),
            Some(log),
            None,
            None,
        ) {
            Ok(server) => server,
            Err(error) => return Err(InstanceScopeError { message: error.to_string() }),
        };
        if let Err(error) = server.start() {
            server.dispose();
            return Err(InstanceScopeError { message: error.to_string() });
        }
        Ok(Some(Self { mutex_name: mutex_name.to_owned(), mutex: Mutex::new(mutex), server: Arc::new(server) }))
    }

    fn dispose_claim(&self) -> Result<(), InstanceScopeError> {
        // Stop and join the pipe listener before closing the mutex name, matching the C# ownership ordering.
        self.server.dispose();
        lock(&self.mutex).close()
    }
}

impl ScopeClaim for HeldClaim {
    fn mutex_name(&self) -> &str {
        &self.mutex_name
    }

    fn stop_listening(&self) {
        self.server.dispose();
    }

    fn dispose(&self) -> Result<(), InstanceScopeError> {
        self.dispose_claim()
    }
}

impl Drop for HeldClaim {
    fn drop(&mut self) {
        self.server.dispose();
        // OwnedKernelHandle closes its handle after the listener is stopped.
    }
}

#[cfg(windows)]
struct OwnedKernelHandle(windows::Win32::Foundation::HANDLE);

// SAFETY: a Win32 kernel handle is process-wide and may be closed from any thread; access to this owned wrapper is serialized by `HeldClaim`'s mutex.
#[cfg(windows)]
unsafe impl Send for OwnedKernelHandle {}

#[cfg(windows)]
impl OwnedKernelHandle {
    fn create_mutex(name: &str) -> Result<Option<Self>, InstanceScopeError> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Foundation::{
            CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_INVALID_HANDLE, GetLastError,
        };
        use windows::Win32::System::Threading::CreateMutexW;
        use windows::core::PCWSTR;

        let wide: Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect();
        // SAFETY: `wide` is NUL terminated and remains alive for the synchronous CreateMutexW call.
        let handle = match unsafe { CreateMutexW(None, false, PCWSTR(wide.as_ptr())) } {
            Ok(handle) => handle,
            Err(error) => {
                let code = GetLastError();
                if code == ERROR_ACCESS_DENIED || code == ERROR_INVALID_HANDLE {
                    return Ok(None);
                }
                return Err(InstanceScopeError { message: error.to_string() });
            }
        };
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // SAFETY: CreateMutexW returned this owned handle; it is closed exactly once here.
            let _ = unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(Self(handle)))
    }

    fn close(&mut self) -> Result<(), InstanceScopeError> {
        if self.0.0.is_null() {
            return Ok(());
        }
        use windows::Win32::Foundation::CloseHandle;
        // SAFETY: this wrapper owns the successful CreateMutexW handle and closes it once.
        let result = unsafe { CloseHandle(self.0) };
        self.0 = windows::Win32::Foundation::HANDLE::default();
        result.map_err(|error| InstanceScopeError { message: error.to_string() })
    }
}

#[cfg(windows)]
impl Drop for OwnedKernelHandle {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(not(windows))]
struct OwnedKernelHandle;

#[cfg(not(windows))]
impl OwnedKernelHandle {
    fn create_mutex(_name: &str) -> Result<Option<Self>, InstanceScopeError> {
        Err(InstanceScopeError { message: "instance scope requires Windows".to_owned() })
    }

    fn close(&mut self) -> Result<(), InstanceScopeError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pr_core::instance::forwarding::{ForwardError, ForwardOutcome};
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::sync::mpsc::{self, Receiver, Sender};

    #[derive(Default)]
    struct FakeClaims {
        held: Mutex<HashSet<String>>,
        occupied_next: Mutex<HashMap<String, VecDeque<bool>>>,
        disposed: Mutex<Vec<String>>,
        disposed_changed: Condvar,
        stopped: Mutex<HashSet<String>>,
        events: Mutex<Vec<String>>,
        fail_dispose: Mutex<HashSet<String>>,
        release_barrier: Mutex<Option<Arc<ReleaseBarrier>>>,
        attempts: Mutex<HashMap<String, usize>>,
        attempts_changed: Condvar,
    }

    #[derive(Default)]
    struct ReleaseBarrier {
        started: Mutex<bool>,
        changed: Condvar,
        released: Mutex<bool>,
        release_changed: Condvar,
    }

    impl ReleaseBarrier {
        fn wait_started(&self) {
            let started = lock(&self.started);
            let (started, result) = self.changed.wait_timeout_while(started, Duration::from_secs(2), |value| !*value)
                .unwrap_or_else(PoisonError::into_inner);
            assert!(*started && !result.timed_out(), "release worker did not enter fake disposal");
        }

        fn release(&self) {
            *lock(&self.released) = true;
            self.release_changed.notify_all();
        }

        fn wait_release(&self) {
            let released = lock(&self.released);
            let (released, result) = self.release_changed.wait_timeout_while(released, Duration::from_secs(2), |value| !*value)
                .unwrap_or_else(PoisonError::into_inner);
            assert!(*released && !result.timed_out(), "test did not release fake disposal");
        }
    }

    #[derive(Default)]
    struct FakeClients {
        outcome: Mutex<Option<Result<ForwardOutcome, ForwardError>>>,
        sent: Mutex<Vec<Vec<String>>>,
    }

    #[derive(Default)]
    struct FakeLog(Mutex<Vec<String>>);

    impl Log for FakeLog {
        fn enabled(&self) -> bool { true }
        fn info(&self, message: &str) { lock(&self.0).push(format!("INFO:{message}")); }
        fn warn(&self, message: &str) { lock(&self.0).push(format!("WARN:{message}")); }
        fn error(&self, message: &str, error: Option<&(dyn std::error::Error + 'static)>) {
            let kind = error.map_or_else(String::new, |error| error.to_string());
            lock(&self.0).push(format!("ERROR:{message}:{kind}"));
        }
    }

    struct FakeClient(Arc<FakeClients>);

    impl InstanceForwardClient for FakeClient {
        fn send(&self, paths: &[String], _timeout: Duration, _cancel: &CancelToken) -> Result<ForwardOutcome, ForwardError> {
            lock(&self.0.sent).push(paths.to_vec());
            lock(&self.0.outcome).clone().unwrap_or(Ok(ForwardOutcome::NoInstance))
        }
    }

    struct FakeClaim {
        name: String,
        shared: Arc<FakeClaims>,
    }

    impl ScopeClaim for FakeClaim {
        fn mutex_name(&self) -> &str { &self.name }

        fn stop_listening(&self) {
            lock(&self.shared.stopped).insert(self.name.clone());
        }

        fn dispose(&self) -> Result<(), InstanceScopeError> {
            if let Some(barrier) = lock(&self.shared.release_barrier).clone() {
                *lock(&barrier.started) = true;
                barrier.changed.notify_all();
                barrier.wait_release();
            }
            lock(&self.shared.events).push(format!("server-stop:{}", self.name));
            lock(&self.shared.disposed).push(self.name.clone());
            self.shared.disposed_changed.notify_all();
            lock(&self.shared.events).push(format!("mutex-close:{}", self.name));
            lock(&self.shared.held).remove(&self.name);
            if lock(&self.shared.fail_dispose).contains(&self.name) {
                return Err(InstanceScopeError { message: "fake dispose failure".to_owned() });
            }
            Ok(())
        }
    }

    fn fake_scope(
        mode: InstanceMode,
        prefix: &str,
        shared: Arc<FakeClaims>,
        clients: Arc<FakeClients>,
    ) -> InstanceScope {
        fake_scope_with_log(mode, prefix, shared, clients, Arc::new(FakeLog::default())).0
    }

    fn fake_scope_with_log(
        mode: InstanceMode,
        prefix: &str,
        shared: Arc<FakeClaims>,
        clients: Arc<FakeClients>,
        log: Arc<FakeLog>,
    ) -> (InstanceScope, Arc<FakeLog>) {
        let client_factory: Arc<ClientFactory> = Arc::new({
            let clients = Arc::clone(&clients);
            move |_| Ok(Box::new(FakeClient(Arc::clone(&clients))) as Box<dyn InstanceForwardClient>)
        });
        let claim_factory: Arc<ClaimFactory> = Arc::new({
            let shared = Arc::clone(&shared);
            move |mutex_name, _pipe_name, _callback, _log| {
                *lock(&shared.attempts).entry(mutex_name.to_owned()).or_default() += 1;
                shared.attempts_changed.notify_all();
                let queued = lock(&shared.occupied_next).get_mut(mutex_name).and_then(VecDeque::pop_front);
                let mut held = lock(&shared.held);
                if queued.unwrap_or_else(|| held.contains(mutex_name)) {
                    return Ok(None);
                }
                held.insert(mutex_name.to_owned());
                Ok(Some(Arc::new(FakeClaim { name: mutex_name.to_owned(), shared: Arc::clone(&shared) }) as Arc<dyn ScopeClaim>))
            }
        });
        let scope = InstanceScope::with_factories(
            mode,
            |_| {},
            prefix,
            Duration::from_millis(50),
            log.clone(),
            client_factory,
            claim_factory,
        ).expect("fake scope construction succeeds");
        (scope, log)
    }

    fn scope(mode: InstanceMode, prefix: &str) -> InstanceScope {
        InstanceScope::with_options(mode, |_| {}, None, prefix, None, false).expect("valid scope options")
    }

    #[test]
    fn keys_single_window_ignore_folder() {
        // C# twin: InstanceScopeTests.Keys_SingleWindow_IgnoreFolder.
        let scope = scope(InstanceMode::SingleWindow, "PhotoReviewRsScopeTest");
        assert_eq!(scope.keys_for(Some(r"C:\One")).unwrap(), scope.keys_for(Some(r"D:\Two")).unwrap());
    }

    #[test]
    fn keys_per_folder_follow_folder_and_keep_legacy_names() {
        // C# twin: InstanceScopeTests.Keys_PerFolder_FollowFolderAndKeepLegacyNames.
        let scope = scope(InstanceMode::PerFolder, "PhotoReviewRsScopeTest");
        let first = scope.keys_for(Some(r"C:\One")).unwrap();
        let second = scope.keys_for(Some(r"D:\Two")).unwrap();
        assert!(first.0.starts_with(r"Local\PhotoReviewRsScopeTest_"));
        assert!(first.1.starts_with("PhotoReviewRsScopeTest.Forward."));
        assert_eq!(first, scope.keys_for(Some(r"c:\one\")).unwrap());
        assert_ne!(first, second);
    }

    #[test]
    fn keys_prefix_changes_both_names() {
        // C# twin: InstanceScopeTests.Keys_Prefix_ChangesBothNames.
        for mode in [InstanceMode::SingleWindow, InstanceMode::PerFolder] {
            let first = scope(mode, "PhotoReviewRsScopeA").keys_for(Some(r"C:\One")).unwrap();
            let second = scope(mode, "PhotoReviewRsScopeB").keys_for(Some(r"C:\One")).unwrap();
            assert_ne!(first.0, second.0);
            assert_ne!(first.1, second.1);
        }
    }

    #[test]
    fn constructor_blank_prefix_is_rejected() {
        // C# twin: InstanceScopeGapTests.Constructor_BlankPrefixOrNullHandler_Throws.
        assert!(InstanceScope::with_options(InstanceMode::SingleWindow, |_| {}, None, " \t", None, false).is_err());
        // Rust's non-optional callback parameter makes the C# null-handler case unrepresentable at runtime.
    }

    #[test]
    fn public_constructor_uses_the_requested_mode_and_default_key_prefix() {
        // C# twin: InstanceScopeGapTests.PublicConstructor_UsesTheGivenModeAndPrefixAndTheRealForwardClient.
        let scope = InstanceScope::new(InstanceMode::PerFolder, |_| {}).unwrap();
        assert_eq!(scope.mode(), InstanceMode::PerFolder);
        let (mutex, pipe) = scope.keys_for(None).unwrap();
        assert!(mutex.starts_with("Local\\PhotoReview_"));
        assert!(pipe.starts_with("PhotoReview.Forward."));
    }

    #[test]
    fn claim_is_idempotent_and_try_acquire_reports_owner() {
        // C# twin: PerFolder_LaunchForShownFolder_IsRefused.
        let shared = Arc::new(FakeClaims::default());
        let clients = Arc::new(FakeClients::default());
        let scope = fake_scope(InstanceMode::PerFolder, "ScopeClaimIdempotent", Arc::clone(&shared), Arc::clone(&clients));
        assert_eq!(scope.claim(Some(r"C:\A")).unwrap(), InstanceClaimResult::Acquired);
        assert_eq!(scope.claim(Some(r"C:\A")).unwrap(), InstanceClaimResult::AlreadyHeld);
        assert!(scope.holds(Some(r"C:\A")).unwrap());
        let second = fake_scope(InstanceMode::PerFolder, "ScopeClaimIdempotent", shared, clients);
        assert!(!second.try_acquire(Some(r"C:\A")).unwrap());
    }

    #[test]
    fn launch_for_another_scopes_shown_folder_is_refused_but_other_folder_is_available() {
        // C# twin: InstanceScopeTests.PerFolder_LaunchForShownFolder_IsRefused.
        let shared = Arc::new(FakeClaims::default());
        let clients = Arc::new(FakeClients::default());
        let owner = fake_scope(InstanceMode::PerFolder, "ScopeShownFolder", Arc::clone(&shared), Arc::clone(&clients));
        let launcher = fake_scope(InstanceMode::PerFolder, "ScopeShownFolder", shared, clients);
        owner.claim(Some(r"C:\Shown")).unwrap();
        owner.on_folder_shown(r"C:\Shown");
        assert!(!launcher.try_acquire(Some(r"c:\shown\")).unwrap());
        assert!(launcher.try_acquire(Some(r"C:\Other")).unwrap());
        owner.dispose();
        launcher.dispose();
    }

    #[test]
    fn name_occupied_by_another_kernel_object_is_reported_as_owned_elsewhere() {
        // C# twin: InstanceScopeGapTests.Claim_NameHeldByANonMutexObject_IsReportedAsOwnedElsewhere.
        let shared = Arc::new(FakeClaims::default());
        let scope = fake_scope(InstanceMode::PerFolder, "ScopeNonMutexObject", Arc::clone(&shared), Arc::new(FakeClients::default()));
        let (name, _) = scope.keys_for(Some(r"C:\Squatted")).unwrap();
        lock(&shared.held).insert(name);
        assert_eq!(scope.claim(Some(r"C:\Squatted")).unwrap(), InstanceClaimResult::OwnedElsewhere);
        assert!(!scope.holds(Some(r"C:\Squatted")).unwrap());
    }

    #[test]
    fn before_open_proceeds_and_after_open_releases_only_finished_unshown_claims() {
        // C# twins: SingleWindow_FolderSwitch_KeepsAppLock; PendingOpens_AreCountedPerFolder_AndOnlyTheLastFinishReleasesTheClaim.
        let shared = Arc::new(FakeClaims::default());
        let clients = Arc::new(FakeClients::default());
        let scope = fake_scope(InstanceMode::PerFolder, "ScopePendingCount", Arc::clone(&shared), clients);
        let cancel = CancelToken::none();
        assert_eq!(scope.before_open(r"C:\A", None, &cancel).unwrap(), FolderOpenDecision::Proceed);
        assert_eq!(scope.before_open(r"C:\A", None, &cancel).unwrap(), FolderOpenDecision::Proceed);
        assert_eq!(scope.before_open(r"C:\B", None, &cancel).unwrap(), FolderOpenDecision::Proceed);
        scope.after_open(r"C:\B", Some(r"C:\B"));
        scope.after_open(r"C:\A", Some(r"C:\B"));
        assert!(scope.holds(Some(r"C:\A")).unwrap(), "one A open remains pending");
        scope.after_open(r"C:\A", Some(r"C:\B"));
        wait_for_disposed(&shared, 1);
        assert!(!scope.holds(Some(r"C:\A")).unwrap());
        assert!(scope.holds(Some(r"C:\B")).unwrap());
    }

    #[test]
    fn owner_accepting_forward_keeps_the_current_folder() {
        // C# twins: SingleWindow_SecondLaunchOtherFolder_Forwards; PerFolder_SwitchToFolderOwnedElsewhere_ForwardsAndKeepsCurrent.
        let shared = Arc::new(FakeClaims::default());
        let clients = Arc::new(FakeClients::default());
        *lock(&clients.outcome) = Some(Ok(ForwardOutcome::Delivered));
        let scope = fake_scope(InstanceMode::PerFolder, "ScopeForward", Arc::clone(&shared), Arc::clone(&clients));
        let owner = fake_scope(InstanceMode::PerFolder, "ScopeForward", Arc::clone(&shared), Arc::clone(&clients));
        owner.claim(Some(r"C:\B")).unwrap();
        scope.before_open(r"C:\A", None, &CancelToken::none()).unwrap();
        assert_eq!(scope.before_open(r"C:\B", Some(r"C:\B\photo.jpg"), &CancelToken::none()).unwrap(), FolderOpenDecision::ForwardedToOtherInstance);
        assert_eq!(lock(&clients.sent).len(), 1);
        assert!(scope.holds(Some(r"C:\A")).unwrap());
        owner.dispose();
        scope.dispose();
    }

    #[test]
    fn single_window_second_launch_forwards_other_folder_and_empty_activation() {
        // C# twins: SingleWindow_SecondLaunchOtherFolder_Forwards; SingleWindow_SecondLaunchNoPath_ForwardsActivation.
        let shared = Arc::new(FakeClaims::default());
        let clients = Arc::new(FakeClients::default());
        *lock(&clients.outcome) = Some(Ok(ForwardOutcome::Delivered));
        let owner = fake_scope(InstanceMode::SingleWindow, "ScopeSingleLaunch", Arc::clone(&shared), Arc::clone(&clients));
        let second = fake_scope(InstanceMode::SingleWindow, "ScopeSingleLaunch", Arc::clone(&shared), Arc::clone(&clients));
        assert!(owner.try_acquire(Some(r"C:\A")).unwrap());
        assert!(!second.try_acquire(Some(r"D:\B")).unwrap());
        let client = second.create_client(Some(r"D:\B")).unwrap();
        let target = vec![r"D:\B\photo.jpg".to_owned()];
        assert_eq!(client.send(&target, Duration::from_millis(50), &CancelToken::none()), Ok(ForwardOutcome::Delivered));
        let activation = Vec::<String>::new();
        assert_eq!(client.send(&activation, Duration::from_millis(50), &CancelToken::none()), Ok(ForwardOutcome::Delivered));
        assert_eq!(lock(&clients.sent).as_slice(), &[target, activation]);
        owner.dispose();
    }

    #[test]
    fn single_window_folder_switch_keeps_the_shared_application_claim() {
        // C# twin: SingleWindow_FolderSwitch_KeepsAppLock.
        let shared = Arc::new(FakeClaims::default());
        let scope = fake_scope(InstanceMode::SingleWindow, "ScopeSingleSwitch", Arc::clone(&shared), Arc::new(FakeClients::default()));
        let cancel = CancelToken::none();
        scope.try_acquire(Some(r"C:\A")).unwrap();
        assert_eq!(scope.before_open(r"D:\B", None, &cancel).unwrap(), FolderOpenDecision::Proceed);
        scope.on_folder_shown(r"D:\B");
        scope.after_open(r"D:\B", Some(r"D:\B"));
        assert!(scope.holds(Some(r"C:\A")).unwrap());
        let second = fake_scope(InstanceMode::SingleWindow, "ScopeSingleSwitch", shared, Arc::new(FakeClients::default()));
        assert!(!second.try_acquire(None).unwrap());
        scope.dispose();
    }

    #[test]
    fn owner_silent_reports_owned_elsewhere_after_one_final_claim_attempt() {
        // C# twin: InstanceScopeTests.PerFolder_OwnerSilent_ReportsOwnedByOther.
        let shared = Arc::new(FakeClaims::default());
        let clients = Arc::new(FakeClients::default());
        let scope = fake_scope(InstanceMode::PerFolder, "ScopeOwnerSilent", Arc::clone(&shared), Arc::clone(&clients));
        let owner = fake_scope(InstanceMode::PerFolder, "ScopeOwnerSilent", Arc::clone(&shared), Arc::clone(&clients));
        owner.claim(Some(r"C:\B")).unwrap();
        assert_eq!(scope.before_open(r"C:\B", Some(r"C:\B\photo.jpg"), &CancelToken::none()).unwrap(), FolderOpenDecision::OwnedByOtherInstance);
        assert_eq!(lock(&clients.sent).len(), 1);
        owner.dispose();
    }

    #[test]
    fn nul_forward_path_returns_owned_by_other_without_calling_client() {
        // C# twin: BeforeOpenAsync_FolderWithNulWhileOwnedElsewhere_ReturnsDecision.
        let shared = Arc::new(FakeClaims::default());
        let clients = Arc::new(FakeClients::default());
        let scope = fake_scope(InstanceMode::PerFolder, "ScopeNulPath", Arc::clone(&shared), Arc::clone(&clients));
        let owner = fake_scope(InstanceMode::PerFolder, "ScopeNulPath", shared, Arc::clone(&clients));
        let bad_folder = "C:\\bad\0folder";
        owner.claim(Some(bad_folder)).unwrap();
        assert_eq!(scope.before_open(bad_folder, None, &CancelToken::none()).unwrap(), FolderOpenDecision::OwnedByOtherInstance);
        assert!(lock(&clients.sent).is_empty());
        owner.dispose();
    }

    #[test]
    fn failed_forward_retries_claim_once_if_owner_released_during_handoff() {
        // C# twin: PerFolder_OwnerLeavesDuringForward_ClaimsAndProceeds.
        let shared = Arc::new(FakeClaims::default());
        let clients = Arc::new(FakeClients::default());
        let scope = fake_scope(InstanceMode::PerFolder, "ScopeFinalClaim", Arc::clone(&shared), Arc::clone(&clients));
        let (name, _) = scope.keys_for(Some(r"C:\B")).unwrap();
        lock(&shared.occupied_next).insert(name, VecDeque::from([true, false]));
        assert_eq!(scope.before_open(r"C:\B", Some(r"C:\B\photo.jpg"), &CancelToken::none()).unwrap(), FolderOpenDecision::Proceed);
        assert_eq!(lock(&clients.sent).len(), 1);
        assert!(scope.holds(Some(r"C:\B")).unwrap());
        scope.dispose();
    }

    #[test]
    fn shown_folder_change_preserves_pending_claim_until_after_open() {
        // C# twins: PerFolder_FolderSwitch_LockAndPipeFollowFolder; PerFolder_SupersededOpen_KeepsPendingClaim.
        let shared = Arc::new(FakeClaims::default());
        let scope = fake_scope(InstanceMode::PerFolder, "ScopeShownPending", Arc::clone(&shared), Arc::new(FakeClients::default()));
        let cancel = CancelToken::none();
        scope.before_open(r"C:\A", None, &cancel).unwrap();
        scope.before_open(r"C:\B", None, &cancel).unwrap();
        scope.on_folder_shown(r"C:\B");
        assert!(scope.holds(Some(r"C:\A")).unwrap());
        scope.after_open(r"C:\A", Some(r"C:\B"));
        wait_for_disposed(&shared, 1);
        assert!(!scope.holds(Some(r"C:\A")).unwrap());
        assert!(scope.holds(Some(r"C:\B")).unwrap());
    }

    #[test]
    fn folder_switch_releases_old_claim_and_keeps_new_folder_claim() {
        // C# twin: PerFolder_FolderSwitch_LockAndPipeFollowFolder.
        let shared = Arc::new(FakeClaims::default());
        let scope = fake_scope(InstanceMode::PerFolder, "ScopeFolderSwitch", Arc::clone(&shared), Arc::new(FakeClients::default()));
        scope.try_acquire(Some(r"C:\A")).unwrap();
        assert_eq!(scope.before_open(r"C:\B", None, &CancelToken::none()).unwrap(), FolderOpenDecision::Proceed);
        scope.on_folder_shown(r"C:\B");
        scope.after_open(r"C:\B", Some(r"C:\B"));
        wait_for_disposed(&shared, 1);
        assert!(!scope.holds(Some(r"C:\A")).unwrap());
        assert!(scope.holds(Some(r"C:\B")).unwrap());
    }

    #[test]
    fn failed_open_releases_target_claim_but_keeps_currently_shown_claim() {
        // C# twin: PerFolder_FailedOpen_ReleasesClaimKeepsShown.
        let shared = Arc::new(FakeClaims::default());
        let scope = fake_scope(InstanceMode::PerFolder, "ScopeFailedOpen", Arc::clone(&shared), Arc::new(FakeClients::default()));
        scope.claim(Some(r"C:\A")).unwrap();
        scope.on_folder_shown(r"C:\A");
        assert_eq!(scope.before_open(r"C:\B", None, &CancelToken::none()).unwrap(), FolderOpenDecision::Proceed);
        scope.after_open(r"C:\B", Some(r"C:\A")); // failed load still completes exactly one pending open
        wait_for_disposed(&shared, 1);
        assert!(scope.holds(Some(r"C:\A")).unwrap());
        assert!(!scope.holds(Some(r"C:\B")).unwrap());
    }

    #[test]
    fn superseded_open_does_not_release_a_different_pending_target() {
        // C# twin: PerFolder_SupersededOpen_KeepsPendingClaim.
        let shared = Arc::new(FakeClaims::default());
        let scope = fake_scope(InstanceMode::PerFolder, "ScopeSuperseded", Arc::clone(&shared), Arc::new(FakeClients::default()));
        scope.claim(Some(r"C:\A")).unwrap();
        scope.on_folder_shown(r"C:\A");
        scope.before_open(r"C:\B", None, &CancelToken::none()).unwrap();
        scope.before_open(r"C:\C", None, &CancelToken::none()).unwrap();
        scope.after_open(r"C:\B", Some(r"C:\A"));
        wait_for_disposed(&shared, 1);
        assert!(!scope.holds(Some(r"C:\B")).unwrap());
        assert!(scope.holds(Some(r"C:\C")).unwrap());
        scope.on_folder_shown(r"C:\C");
        scope.after_open(r"C:\C", Some(r"C:\C"));
        wait_for_disposed(&shared, 2);
        scope.dispose();
    }

    #[test]
    fn reopen_waits_for_our_pending_release_then_claims() {
        // C# twin: PerFolder_ReopenFolderWhoseReleaseIsPending_WaitsAndProceeds.
        let shared = Arc::new(FakeClaims::default());
        let clients = Arc::new(FakeClients::default());
        *lock(&clients.outcome) = Some(Ok(ForwardOutcome::Delivered));
        let barrier = Arc::new(ReleaseBarrier::default());
        *lock(&shared.release_barrier) = Some(Arc::clone(&barrier));
        let scope = Arc::new(fake_scope(InstanceMode::PerFolder, "ScopeReleaseWait", Arc::clone(&shared), Arc::clone(&clients)));
        let cancel = CancelToken::none();
        scope.before_open(r"C:\A", None, &cancel).unwrap();
        scope.on_folder_shown(r"C:\B");
        barrier.wait_started();

        let (started_tx, started_rx): (Sender<()>, Receiver<()>) = mpsc::channel();
        let reopen_scope = Arc::clone(&scope);
        let (mutex, _) = scope.keys_for(Some(r"C:\A")).unwrap();
        let reopen = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            reopen_scope.before_open(r"C:\A", None, &CancelToken::none()).unwrap()
        });
        started_rx.recv_timeout(Duration::from_secs(2)).expect("reopen worker started");
        wait_for_claim_attempts(&shared, &mutex, 2);
        barrier.release();
        assert_eq!(reopen.join().unwrap(), FolderOpenDecision::Proceed);
        assert!(lock(&clients.sent).is_empty(), "self-owned pending mutex must not be forwarded to");
        scope.dispose();
    }

    #[test]
    fn stop_listening_retains_mutex_claims() {
        // C# twin: InstanceScopeGapTests.StopListening_KeepsTheLockButStopsAnsweringTheForwardPipe.
        let shared = Arc::new(FakeClaims::default());
        let scope = fake_scope(InstanceMode::PerFolder, "ScopeStopListener", Arc::clone(&shared), Arc::new(FakeClients::default()));
        scope.claim(Some(r"C:\A")).unwrap();
        let (name, _) = scope.keys_for(Some(r"C:\A")).unwrap();
        scope.stop_listening();
        assert!(scope.holds(Some(r"C:\A")).unwrap());
        assert!(lock(&shared.held).contains(&name));
        assert!(lock(&shared.stopped).contains(&name));
        scope.dispose();
    }

    #[test]
    fn dispose_failure_does_not_prevent_other_claim_cleanup() {
        // C# twins: Dispose_OneClaimFailsToDispose_StillFreesTheOthersAndLogsIt; Release_ClaimFailsToDispose_IsLoggedAndTheOtherReleasesStillRun.
        let shared = Arc::new(FakeClaims::default());
        let (scope, log) = fake_scope_with_log(InstanceMode::PerFolder, "ScopeDisposeFailures", Arc::clone(&shared), Arc::new(FakeClients::default()), Arc::new(FakeLog::default()));
        scope.claim(Some(r"C:\A")).unwrap();
        scope.claim(Some(r"C:\B")).unwrap();
        let names = [scope.keys_for(Some(r"C:\A")).unwrap().0, scope.keys_for(Some(r"C:\B")).unwrap().0];
        let (bad, _) = scope.keys_for(Some(r"C:\A")).unwrap();
        lock(&shared.fail_dispose).insert(bad);
        scope.dispose();
        assert_eq!(lock(&shared.disposed).len(), 2);
        assert!(lock(&shared.held).is_empty());
        assert!(lock(&log.0).iter().any(|message| message.starts_with("WARN:Instance claim dispose failed for "));
        let events = lock(&shared.events);
        for name in &names {
            let server = events.iter().position(|event| event == &format!("server-stop:{name}")).unwrap();
            let mutex = events.iter().position(|event| event == &format!("mutex-close:{name}")).unwrap();
            assert!(server < mutex, "listener must stop before its mutex is closed");
        }
    }

    #[test]
    fn release_failure_does_not_prevent_sibling_release() {
        // C# twin: InstanceScopeGapTests.Release_ClaimFailsToDispose_IsLoggedAndTheOtherReleasesStillRun.
        let shared = Arc::new(FakeClaims::default());
        let (scope, log) = fake_scope_with_log(InstanceMode::PerFolder, "ScopeReleaseFailures", Arc::clone(&shared), Arc::new(FakeClients::default()), Arc::new(FakeLog::default()));
        scope.claim(Some(r"C:\A")).unwrap();
        scope.claim(Some(r"C:\B")).unwrap();
        let (bad, _) = scope.keys_for(Some(r"C:\A")).unwrap();
        lock(&shared.fail_dispose).insert(bad);
        scope.on_folder_shown(r"C:\C");
        wait_for_disposed(&shared, 2);
        assert!(lock(&shared.held).is_empty());
        assert!(lock(&log.0).iter().any(|message| message.starts_with("ERROR:Instance lock release failed: "));
    }

    #[test]
    fn disposed_scope_proceeds_without_acquiring_another_lock() {
        // C# twin: InstanceScopeGapTests.BeforeOpenAsync_AfterDispose_JustProceedsAndHoldsNothing.
        let shared = Arc::new(FakeClaims::default());
        let scope = fake_scope(InstanceMode::PerFolder, "ScopeDisposed", Arc::clone(&shared), Arc::new(FakeClients::default()));
        scope.dispose();
        assert_eq!(scope.before_open(r"C:\A", None, &CancelToken::none()).unwrap(), FolderOpenDecision::Proceed);
        assert!(lock(&shared.held).is_empty());
    }

    fn wait_for_disposed(shared: &FakeClaims, expected: usize) {
        let disposed = lock(&shared.disposed);
        let (disposed, result) = shared.disposed_changed
            .wait_timeout_while(disposed, Duration::from_secs(2), |items| items.len() < expected)
            .unwrap_or_else(PoisonError::into_inner);
        assert!(disposed.len() >= expected && !result.timed_out(), "release worker did not dispose the expected claims");
    }

    fn wait_for_claim_attempts(shared: &FakeClaims, name: &str, expected: usize) {
        let attempts = lock(&shared.attempts);
        let (attempts, result) = shared.attempts_changed
            .wait_timeout_while(attempts, Duration::from_secs(2), |items| items.get(name).copied().unwrap_or_default() < expected)
            .unwrap_or_else(PoisonError::into_inner);
        assert!(attempts.get(name).copied().unwrap_or_default() >= expected && !result.timed_out());
    }
}
