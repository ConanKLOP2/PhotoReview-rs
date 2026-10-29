#[allow(dead_code)] // The integration test includes W6-03b's source because module wiring belongs to the lead.
#[path = "../src/forward_pipe.rs"]
mod forward_pipe;
// The Windows production entrypoint and native backend are not exercised by the fake-only default test cases.
#[allow(dead_code)]
#[path = "../src/forward_server.rs"]
mod forward_server;
#[path = "../src/instance_keys.rs"]
mod instance_keys;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, mpsc};
use std::time::{Duration, Instant};

use forward_server::{ForwardPipe, ForwardPipeError, ForwardPipeFactory, InstanceForwardServer};
use pr_core::abstractions::Log;
use pr_core::instance::protocol::MAX_MESSAGE_BYTES;

const TEST_TIMEOUT: Duration = Duration::from_secs(3);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
struct RequestData {
    chunks: VecDeque<Vec<u8>>,
    chunk_offset: usize,
    client_closed: bool,
    bytes_read: usize,
    reply: Option<mpsc::Sender<Vec<u8>>>,
}

struct FakeRequest {
    data: Mutex<RequestData>,
    changed: Condvar,
}

impl FakeRequest {
    fn new(chunks: Vec<Vec<u8>>, client_closed: bool) -> (Arc<Self>, mpsc::Receiver<Vec<u8>>) {
        let (reply, receiver) = mpsc::channel();
        (
            Arc::new(Self {
                data: Mutex::new(RequestData {
                    chunks: chunks.into(),
                    client_closed,
                    reply: Some(reply),
                    ..RequestData::default()
                }),
                changed: Condvar::new(),
            }),
            receiver,
        )
    }
}

#[derive(Default)]
struct FactoryData {
    requests: VecDeque<Arc<FakeRequest>>,
    last_accepted: Option<Arc<FakeRequest>>,
    canceled: bool,
    create_failures: usize,
    create_count: usize,
    retry_delays: Vec<Duration>,
    accept_started: Option<mpsc::Sender<()>>,
}

#[derive(Default)]
struct FakeFactory {
    data: Mutex<FactoryData>,
    changed: Condvar,
}

impl FakeFactory {
    fn with_accept_signal() -> (Arc<Self>, mpsc::Receiver<()>) {
        let (sender, receiver) = mpsc::channel();
        let factory = Arc::new(Self {
            data: Mutex::new(FactoryData {
                accept_started: Some(sender),
                ..FactoryData::default()
            }),
            changed: Condvar::new(),
        });
        (factory, receiver)
    }

    fn submit(&self, bytes: Vec<u8>, client_closed: bool) -> mpsc::Receiver<Vec<u8>> {
        self.submit_chunks(vec![bytes], client_closed)
    }

    fn submit_chunks(&self, chunks: Vec<Vec<u8>>, client_closed: bool) -> mpsc::Receiver<Vec<u8>> {
        let (request, reply) = FakeRequest::new(chunks, client_closed);
        lock(&self.data).requests.push_back(request);
        self.changed.notify_one();
        reply
    }

    fn fail_next_creates(&self, count: usize) {
        lock(&self.data).create_failures = count;
    }

    fn retry_delays(&self) -> Vec<Duration> {
        lock(&self.data).retry_delays.clone()
    }

    fn counts(&self) -> (usize, usize) {
        let state = lock(&self.data);
        (state.create_count, state.requests.len())
    }

    fn last_accepted_bytes(&self) -> usize {
        let request = lock(&self.data).last_accepted.clone();
        request.map_or(0, |request| lock(&request.data).bytes_read)
    }
}

impl ForwardPipeFactory for FakeFactory {
    fn create(self: Arc<Self>, _pipe_name: &str) -> Result<Box<dyn ForwardPipe>, ForwardPipeError> {
        let mut state = lock(&self.data);
        state.create_count += 1;
        if state.canceled {
            return Err(ForwardPipeError::Canceled);
        }
        if state.create_failures != 0 {
            state.create_failures -= 1;
            return Err(ForwardPipeError::Failed("synthetic create failure".to_owned()));
        }
        drop(state);
        Ok(Box::new(FakePipe {
            factory: self,
            request: None,
        }))
    }

    fn cancel(&self) {
        lock(&self.data).canceled = true;
        self.changed.notify_all();
    }

    fn is_canceled(&self) -> bool {
        lock(&self.data).canceled
    }

    fn wait_retry(&self, delay: Duration) -> bool {
        let mut state = lock(&self.data);
        state.retry_delays.push(delay);
        !state.canceled
    }
}

struct FakePipe {
    factory: Arc<FakeFactory>,
    request: Option<Arc<FakeRequest>>,
}

impl ForwardPipe for FakePipe {
    fn accept(&mut self) -> Result<(), ForwardPipeError> {
        let mut state = lock(&self.factory.data);
        if let Some(sender) = state.accept_started.take() {
            let _ = sender.send(());
        }
        while state.requests.is_empty() && !state.canceled {
            state = self.factory.changed.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
        if state.canceled {
            return Err(ForwardPipeError::Canceled);
        }
        self.request = state.requests.pop_front();
        state.last_accepted = self.request.clone();
        Ok(())
    }

    fn read(&mut self, buffer: &mut [u8], deadline: Instant) -> Result<usize, ForwardPipeError> {
        let request = self.request.as_ref().expect("accept must select a fake request");
        let mut state = lock(&request.data);
        loop {
            while state.chunks.front().is_some_and(Vec::is_empty) {
                state.chunks.pop_front();
                state.chunk_offset = 0;
            }
            if let Some(chunk) = state.chunks.front() {
                let chunk_len = chunk.len();
                let offset = state.chunk_offset;
                let count = (chunk_len - offset).min(buffer.len());
                buffer[..count].copy_from_slice(&chunk[offset..offset + count]);
                state.chunk_offset += count;
                state.bytes_read += count;
                if state.chunk_offset == chunk_len {
                    state.chunks.pop_front();
                    state.chunk_offset = 0;
                }
                return Ok(count);
            }
            if state.client_closed {
                return Ok(0);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ForwardPipeError::TimedOut);
            }
            let (next, timeout) = request
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(PoisonError::into_inner);
            state = next;
            if timeout.timed_out() && state.chunks.is_empty() {
                return Err(ForwardPipeError::TimedOut);
            }
        }
    }

    fn write_all(&mut self, bytes: &[u8], deadline: Instant) -> Result<(), ForwardPipeError> {
        if Instant::now() >= deadline {
            return Err(ForwardPipeError::TimedOut);
        }
        let request = self.request.as_ref().expect("accept must select a fake request");
        let sender = lock(&request.data).reply.take();
        if let Some(sender) = sender {
            let _ = sender.send(bytes.to_vec());
        }
        Ok(())
    }
}

#[derive(Default)]
struct EventLog {
    events: Mutex<Vec<String>>,
    changed: Condvar,
}

impl EventLog {
    fn wait_for(&self, expected: &str) {
        let deadline = Instant::now() + TEST_TIMEOUT;
        let mut events = lock(&self.events);
        while !events.iter().any(|event| event.contains(expected)) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "timed out waiting for log event {expected:?}: {events:?}"
            );
            let (next, timeout) = self
                .changed
                .wait_timeout(events, remaining)
                .unwrap_or_else(PoisonError::into_inner);
            events = next;
            assert!(!timeout.timed_out() || events.iter().any(|event| event.contains(expected)));
        }
    }
}

impl Log for EventLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, message: &str) {
        lock(&self.events).push(message.to_owned());
        self.changed.notify_all();
    }
    fn warn(&self, message: &str) {
        lock(&self.events).push(message.to_owned());
        self.changed.notify_all();
    }
    fn error(&self, message: &str, error: Option<&(dyn std::error::Error + 'static)>) {
        let detail = error.map_or_else(String::new, |error| error.to_string());
        lock(&self.events).push(format!("{message} {detail}"));
        self.changed.notify_all();
    }
}

struct RunningServer {
    server: InstanceForwardServer,
    factory: Arc<FakeFactory>,
}

impl RunningServer {
    fn new(
        callback: impl Fn(&[String]) + Send + Sync + 'static,
        path_exists: impl Fn(&str) -> bool + Send + Sync + 'static,
        read_timeout: Duration,
    ) -> Self {
        let factory = Arc::new(FakeFactory::default());
        Self::with_factory(factory, callback, path_exists, read_timeout)
    }

    fn with_factory(
        factory: Arc<FakeFactory>,
        callback: impl Fn(&[String]) + Send + Sync + 'static,
        path_exists: impl Fn(&str) -> bool + Send + Sync + 'static,
        read_timeout: Duration,
    ) -> Self {
        let server = InstanceForwardServer::with_factory(
            "PhotoReview.Test.fake",
            callback,
            None,
            path_exists,
            Some(read_timeout),
            factory.clone(),
        );
        server.start().expect("fake server thread starts");
        Self { server, factory }
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.server.dispose();
    }
}

fn valid_message(path: &str) -> Vec<u8> {
    format!("PHOTOREVIEW-OPEN 1\n{path}\n\n").into_bytes()
}

fn receive_reply(receiver: mpsc::Receiver<Vec<u8>>) -> String {
    String::from_utf8(
        receiver
            .recv_timeout(TEST_TIMEOUT)
            .expect("server replies within the test bound"),
    )
    .expect("server reply is ASCII")
}

#[test]
fn server_malformed_oversized_and_invalid_utf8_are_refused_and_listener_survives() {
    // C# twin: InstanceForwardPipeTests.Server_MalformedAndOversizedInput_IsRefusedAndSurvives.
    let received = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let received_for_callback = Arc::clone(&received);
    let server = RunningServer::new(
        move |paths| lock(&received_for_callback).push(paths.to_vec()),
        |path| path == r"C:\ok.jpg",
        Duration::from_secs(1),
    );

    assert_eq!(
        receive_reply(server.factory.submit(b"not the protocol\n\n".to_vec(), true)),
        "ERR\n"
    );
    let oversized = server.factory.submit(vec![b'x'; MAX_MESSAGE_BYTES + 4096], true);
    assert_eq!(receive_reply(oversized), "ERR\n");
    assert_eq!(
        receive_reply(server.factory.submit(vec![0xFF, 0xFE, b'\n', b'\n'], true)),
        "ERR\n"
    );
    assert_eq!(
        receive_reply(server.factory.submit(valid_message(r"C:\ok.jpg"), true)),
        "OK\n"
    );
    assert_eq!(lock(&received).as_slice(), &[vec![r"C:\ok.jpg".to_owned()]]);
}

#[test]
fn server_oversized_read_is_bounded_to_max_message_plus_one() {
    // C# twin: InstanceForwardPipeTests.Server_MalformedAndOversizedInput_IsRefusedAndSurvives.
    let server = RunningServer::new(
        |_| panic!("oversized request reached callback"),
        |_| true,
        Duration::from_secs(1),
    );
    let oversized = server.factory.submit(vec![b'x'; MAX_MESSAGE_BYTES + 4096], true);
    assert_eq!(receive_reply(oversized), "ERR\n");
    assert_eq!(server.factory.last_accepted_bytes(), MAX_MESSAGE_BYTES + 1);
}

#[test]
fn server_stalled_client_times_out_and_does_not_block_next_request() {
    // C# twin: InstanceForwardPipeTests.Server_StalledClient_TimesOut.
    let (factory, accepting) = FakeFactory::with_accept_signal();
    let log = Arc::new(EventLog::default());
    let callback_count = Arc::new(AtomicUsize::new(0));
    let callback_counter = Arc::clone(&callback_count);
    let server = InstanceForwardServer::with_factory(
        "PhotoReview.Test.fake",
        move |_| {
            callback_counter.fetch_add(1, Ordering::SeqCst);
        },
        Some(log.clone()),
        |_| true,
        Some(Duration::from_millis(50)),
        factory.clone(),
    );
    server.start().expect("fake server thread starts");
    accepting.recv_timeout(TEST_TIMEOUT).expect("listener entered accept");
    let stalled_reply = factory.submit_chunks(Vec::new(), false);
    log.wait_for("request timed out");
    assert!(stalled_reply.try_recv().is_err(), "timeout must close without a reply");
    assert_eq!(receive_reply(factory.submit(valid_message(r"C:\ok.jpg"), true)), "OK\n");
    assert_eq!(callback_count.load(Ordering::SeqCst), 1);
    server.dispose();
}

#[test]
fn server_path_existence_failure_replies_err_without_callback() {
    // C# twin: InstanceForwardPipeTests.Forward_NonExistentPath_IsRejected.
    let count = Arc::new(AtomicUsize::new(0));
    let count_for_callback = Arc::clone(&count);
    let server = RunningServer::new(
        move |_| {
            count_for_callback.fetch_add(1, Ordering::SeqCst);
        },
        |_| false,
        Duration::from_secs(1),
    );
    assert_eq!(
        receive_reply(server.factory.submit(valid_message(r"C:\missing.jpg"), true)),
        "ERR\n"
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn server_callback_failure_rejects_one_request_and_keeps_listening() {
    // C# twin: InstanceForwardPipeTests.Server_MalformedAndOversizedInput_IsRefusedAndSurvives.
    let log = Arc::new(EventLog::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_callback = Arc::clone(&calls);
    let factory = Arc::new(FakeFactory::default());
    let server = InstanceForwardServer::with_factory(
        "PhotoReview.Test.fake",
        move |_| {
            if calls_for_callback.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("synthetic handler failure");
            }
        },
        Some(log.clone()),
        |_| true,
        Some(Duration::from_secs(1)),
        factory.clone(),
    );
    server.start().expect("fake server thread starts");
    assert_eq!(
        receive_reply(factory.submit(valid_message(r"C:\first.jpg"), true)),
        "ERR\n"
    );
    log.wait_for("handler failed");
    assert_eq!(
        receive_reply(factory.submit(valid_message(r"C:\second.jpg"), true)),
        "OK\n"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    server.dispose();
}

#[test]
fn server_retries_creation_failure_after_base_delay_and_then_accepts() {
    // C# twin: InstanceForwardPipeTests.Server_MalformedAndOversizedInput_IsRefusedAndSurvives (listener recovers).
    let factory = Arc::new(FakeFactory::default());
    factory.fail_next_creates(1);
    let server = RunningServer::with_factory(factory.clone(), |_| {}, |_| true, Duration::from_secs(1));
    assert_eq!(
        receive_reply(factory.submit(valid_message(r"C:\retry.jpg"), true)),
        "OK\n"
    );
    assert_eq!(factory.retry_delays().first(), Some(&Duration::from_millis(500)));
    server.server.dispose();
}

#[test]
fn dispose_stops_waiting_accept_and_is_idempotent() {
    // C# twin: InstanceForwardPipeTests.Dispose_StopsListening.
    let (factory, accepting) = FakeFactory::with_accept_signal();
    let server =
        InstanceForwardServer::with_factory("PhotoReview.Test.fake", |_| {}, None, |_| true, None, factory.clone());
    server.start().expect("fake server thread starts");
    accepting.recv_timeout(TEST_TIMEOUT).expect("listener entered accept");
    server.dispose();
    server.dispose();
    assert!(matches!(server.start(), Err(ForwardPipeError::Failed(_))));
    let _ignored = factory.submit(valid_message(r"C:\after-dispose.jpg"), true);
    assert_eq!(
        factory.counts().1,
        1,
        "disposed listener must not accept another request"
    );
}

#[test]
fn server_handles_fragmented_frame_and_incomplete_closed_frame() {
    // C# twin: InstanceForwardPipeTests.Server_MalformedAndOversizedInput_IsRefusedAndSurvives.
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_callback = Arc::clone(&calls);
    let server = RunningServer::new(
        move |_| {
            calls_for_callback.fetch_add(1, Ordering::SeqCst);
        },
        |_| true,
        Duration::from_secs(1),
    );
    assert_eq!(
        receive_reply(server.factory.submit_chunks(
            vec![b"PHOTOREVIEW-".to_vec(), b"OPEN 1\nC:\\part.jpg\n\n".to_vec()],
            true
        )),
        "OK\n"
    );
    assert_eq!(
        receive_reply(
            server
                .factory
                .submit(b"PHOTOREVIEW-OPEN 1\nC:\\partial.jpg\n".to_vec(), true)
        ),
        "ERR\n"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[cfg(windows)]
mod native {
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::forward_server::InstanceForwardServer;
    use windows::Win32::System::Pipes::WaitNamedPipeW;
    use windows::core::HSTRING;

    static NEXT_PIPE: AtomicU64 = AtomicU64::new(1);

    #[test]
    #[ignore = "native: starts and disposes a uniquely named test pipe; WaitNamedPipe is bounded to 5 seconds"]
    fn dispose_stops_native_accept_loop() {
        // C# twin: InstanceForwardPipeTests.Dispose_StopsListening.
        let name = format!(
            "PhotoReview.Test.{}.{}",
            std::process::id(),
            NEXT_PIPE.fetch_add(1, Ordering::Relaxed)
        );
        let server = InstanceForwardServer::new(name.clone(), |_| {}, None, None, None)
            .expect("create server with current-user-only security");
        server.start().expect("start listener");
        let full_name = HSTRING::from(format!(r"\\.\pipe\{name}"));
        // `start` returns before the listener thread has created the pipe, and WaitNamedPipe fails at once while the
        // name does not exist yet, so readiness is awaited with a bounded loop (5 s deadline, no fixed sleep).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut pipe_ready = false;
        while !pipe_ready && std::time::Instant::now() < deadline {
            // SAFETY: the test owns this unique server pipe name; each OS wait is bounded to 100 ms.
            pipe_ready = unsafe { WaitNamedPipeW(&full_name, 100) }.as_bool();
            if !pipe_ready {
                std::thread::yield_now();
            }
        }
        assert!(pipe_ready, "server pipe was not created");
        server.dispose();
        // C# Dispose_StopsListening: after dispose no instance of the pipe remains.
        // SAFETY: same unique name; the wait is bounded to 100 ms.
        let still_there = unsafe { WaitNamedPipeW(&full_name, 100) }.as_bool();
        assert!(!still_there, "pipe name must be freed by dispose");
        assert!(server.start().is_err(), "disposed listener must reject restart");
    }
}
