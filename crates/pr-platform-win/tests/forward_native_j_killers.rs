//! Native killers for the survivors of mutation lane J (`forward_pipe.rs`, `forward_server.rs`, `forward_client.rs`).
//! Evidence: docs/reviews/PLATFORM-NATIVE-J-MUT-2026-11-21.md. Every test uses a uniquely named pipe of its own.
//! The few `sleep` calls make a deliberately slow or late client (test fixture), they are never used to wait for a result.
#![cfg(windows)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use pr_core::abstractions::Log;
use pr_core::instance::forwarding::ForwardOutcome;
use pr_core::instance::protocol;
use pr_core::sync::CancelToken;
use pr_platform_win::forward_client::{
    ForwardClientTransport, ForwardPipeClient, ForwardTransportError, WindowsForwardTransport,
    current_user_sid_for_tests,
};
use pr_platform_win::forward_server::{InstanceForwardServer, native_factory_for_tests};

static NEXT_PIPE: AtomicU64 = AtomicU64::new(1);

fn unique_pipe_name(tag: &str) -> String {
    format!(
        "PhotoReviewRs.test.nativej.{}.{tag}.{}",
        std::process::id(),
        NEXT_PIPE.fetch_add(1, Ordering::Relaxed)
    )
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
struct EventLog {
    events: Mutex<Vec<String>>,
    changed: Condvar,
}

impl EventLog {
    fn push(&self, text: String) {
        lock(&self.events).push(text);
        self.changed.notify_all();
    }

    fn wait_for(&self, expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut events = lock(&self.events);
        while !events.iter().any(|event| event.contains(expected)) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "timed out waiting for {expected:?}: {events:?}");
            events = self
                .changed
                .wait_timeout(events, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn any(&self, needle: &str) -> bool {
        lock(&self.events).iter().any(|event| event.contains(needle))
    }
}

impl Log for EventLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, message: &str) {
        self.push(message.to_owned());
    }
    fn warn(&self, message: &str) {
        self.push(message.to_owned());
    }
    fn error(&self, message: &str, error: Option<&(dyn std::error::Error + 'static)>) {
        self.push(format!(
            "{message} {}",
            error.map_or_else(String::new, |e| e.to_string())
        ));
    }
}

fn start_server(name: &str, log: Option<Arc<dyn Log>>) -> InstanceForwardServer {
    let server = InstanceForwardServer::new(name.to_owned(), |_| {}, log, None, Some(Duration::from_secs(2)))
        .expect("native server");
    server.start().expect("listener starts");
    server
}

fn connect(name: &str) -> WindowsForwardTransport {
    let transport = WindowsForwardTransport::default();
    transport
        .connect(name, Instant::now() + Duration::from_secs(10), &CancelToken::none())
        .expect("client connects to the native server");
    transport
}

fn empty_request() -> Vec<u8> {
    protocol::encode(&[]).expect("an empty path list encodes")
}

fn send_promptly(name: &str) -> ForwardOutcome {
    let client = ForwardPipeClient::new(Some(name.to_owned()), false).expect("client");
    client
        .send_with_timeout(&[], Duration::from_millis(600), &CancelToken::none())
        .expect("send returns an outcome")
}

#[test]
#[ignore = "native: starts a uniquely named test pipe server"]
fn a_slow_client_still_receives_the_reply_the_server_wrote() {
    // Mutants forward_server.rs:611 `written == 0` -> `!=`, :621 `wait_reply_delivered -> Ok(())`: the server would drop (disconnect) its pipe
    // right after writing, which discards the unread `OK` (ST-WIN32 L2). A client that reads late makes that race deterministic.
    let name = unique_pipe_name("slow_reader");
    let server = start_server(&name, None);
    let transport = connect(&name);
    let deadline = Instant::now() + Duration::from_secs(10);
    transport
        .write_all(&empty_request(), deadline, &CancelToken::none())
        .expect("request written");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        transport.read_reply(16, deadline, &CancelToken::none()),
        Ok(b"OK\n".to_vec())
    );
    transport.disconnect();
    server.dispose();
}

#[test]
#[ignore = "native: starts a uniquely named test pipe server"]
fn the_listener_takes_the_next_client_promptly_after_a_delivered_request() {
    // Mutant forward_server.rs:623 `read == 0` -> `!=` in `wait_reply_delivered`: the loop would not end at the client's hang-up and would
    // spin until its one second delivery bound, so the next client (0.6 s budget) would find no free pipe instance.
    let name = unique_pipe_name("prompt_next");
    let server = start_server(&name, None);
    assert_eq!(send_promptly(&name), ForwardOutcome::Delivered);
    assert_eq!(send_promptly(&name), ForwardOutcome::Delivered);
    server.dispose();
}

#[test]
#[ignore = "native: starts a uniquely named test pipe server"]
fn a_client_that_vanishes_after_sending_does_not_stall_the_listener() {
    // Mutant forward_server.rs:603 `write_all` guard -> true: a failed reply write would be awaited as if pending until the 2 s request deadline.
    let name = unique_pipe_name("vanished");
    let server = start_server(&name, None);
    let transport = connect(&name);
    transport
        .write_all(
            &empty_request(),
            Instant::now() + Duration::from_secs(10),
            &CancelToken::none(),
        )
        .expect("request written");
    transport.disconnect();
    assert_eq!(send_promptly(&name), ForwardOutcome::Delivered);
    server.dispose();
}

#[test]
#[ignore = "native: starts a uniquely named test pipe server"]
fn a_client_that_hangs_up_mid_request_is_a_rejected_request_not_a_read_failure() {
    // Mutants forward_server.rs:584 (`read`, hang-up seen at once) and :414 (`wait_operation`, hang-up seen by a pending read): a broken pipe
    // ends the read like end of input (ST-WIN32 L5), so the partial frame is rejected instead of logged as a failed read.
    let name = unique_pipe_name("hangup");
    let log = Arc::new(EventLog::default());
    let server = start_server(&name, Some(log.clone()));
    // Hang-up right after the write: the server's next read usually fails at once with a broken pipe (repeated: the order is not controlled).
    for length in 1..=20usize {
        let transport = connect(&name);
        transport
            .write_all(
                &vec![b'x'; length],
                Instant::now() + Duration::from_secs(10),
                &CancelToken::none(),
            )
            .expect("fragment written");
        transport.disconnect();
        log.wait_for(&format!("rejected ({length} byte(s))"));
    }
    // Hang-up while the server's read is already pending.
    let transport = connect(&name);
    transport
        .write_all(
            &[b'y'; 30],
            Instant::now() + Duration::from_secs(10),
            &CancelToken::none(),
        )
        .expect("fragment written");
    std::thread::sleep(Duration::from_millis(300));
    transport.disconnect();
    log.wait_for("rejected (30 byte(s))");
    assert!(!log.any("read failed"), "{:?}", lock(&log.events));
    server.dispose();
}

#[test]
#[ignore = "native: creates and cancels one event-backed native pipe factory"]
fn native_factory_cancel_wakes_the_retry_wait_and_reports_canceled() {
    // Mutants forward_server.rs:500 `is_canceled -> false`, :504 `wait_retry -> true/false`, :507 (`==` -> `!=`, delete `!`), :493 `delete !` is
    // argued unobservable (P-458); :507 `&&` -> `||` differs only inside the cancel race window (P-457).
    let factory = native_factory_for_tests().expect("native factory");
    assert!(!factory.is_canceled());
    let started = Instant::now();
    assert!(
        factory.wait_retry(Duration::from_millis(30)),
        "an uncancelled wait times out and says retry"
    );
    assert!(started.elapsed() >= Duration::from_millis(25));
    factory.cancel();
    assert!(factory.is_canceled());
    let started = Instant::now();
    assert!(
        !factory.wait_retry(Duration::from_secs(30)),
        "a cancelled wait says stop"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "cancel must wake the wait at once"
    );
    factory.cancel();
    assert!(factory.is_canceled());
}

#[test]
#[ignore = "native: uses an unconnected transport only"]
fn write_with_an_already_expired_deadline_times_out_before_any_byte_is_written() {
    // Mutants forward_client.rs:475:61 (`offset == 0` -> false) and :475:68 (`==` -> `!=`): at offset 0 a timeout is `TimedOut`, not the
    // "bytes may have reached the server" failure.
    let transport = WindowsForwardTransport::default();
    assert_eq!(
        transport.write_all(b"x", Instant::now(), &CancelToken::none()),
        Err(ForwardTransportError::TimedOut)
    );
    // Not connected, deadline in the future: nothing was written, so the failure says no bytes reached the server.
    assert_eq!(
        transport.write_all(b"x", Instant::now() + Duration::from_secs(5), &CancelToken::none()),
        Err(ForwardTransportError::WriteFailed {
            bytes_may_have_reached_server: false
        })
    );
}

#[test]
#[ignore = "native: starts a uniquely named test pipe server"]
fn foreground_permission_for_a_same_process_server_is_granted() {
    // Mutant forward_client.rs:436:20 `pid == 0` -> `!=`: every real server pid would be refused as invalid.
    let name = unique_pipe_name("foreground");
    let server = start_server(&name, None);
    let transport = connect(&name);
    assert_eq!(transport.allow_server_foreground(), Ok(()));
    transport.disconnect();
    server.dispose();
}

#[test]
#[ignore = "native: connects read-only to well-known system pipes; fails when none accepts this client"]
fn a_system_owned_pipe_server_is_not_recognised_as_the_current_user() {
    // Mutants forward_client.rs:243 `process_user_sid -> Some(vec![..])`: a server whose token cannot be read would compare equal to ours
    // (the same-process test below kills them too, through the seam P-459).
    // Services such as `srvsvc` run as SYSTEM or NETWORK SERVICE, so opening their token fails for a normal user (None, fails closed).
    let mut checked = 0;
    for name in [
        "srvsvc",
        "wkssvc",
        "ntsvcs",
        "atsvc",
        "eventlog",
        "lsass",
        "InitShutdown",
    ] {
        let transport = WindowsForwardTransport::default();
        if transport
            .connect(name, Instant::now() + Duration::from_millis(300), &CancelToken::none())
            .is_err()
        {
            continue;
        }
        assert!(!transport.server_is_current_user(), "system pipe {name}");
        transport.disconnect();
        checked += 1;
    }
    assert!(checked > 0, "no well-known system pipe accepted this client");
}

#[test]
#[ignore = "native: starts a uniquely named test pipe server"]
fn a_client_that_hangs_up_before_sending_anything_is_a_rejected_empty_request() {
    // Mutants forward_server.rs:584:31 `read` guard -> false and :584:46 `==` -> `!=`: a connect-and-close client makes the server's first
    // `ReadFile` fail at once with a broken pipe, which must end the read like end of input (ST-WIN32 L5), not log a failed read.
    let name = unique_pipe_name("silent");
    let log = Arc::new(EventLog::default());
    let server = start_server(&name, Some(log.clone()));
    for _ in 0..10 {
        let transport = connect(&name);
        transport.disconnect();
        log.wait_for("rejected (0 byte(s))");
        lock(&log.events).clear();
    }
    assert!(!log.any("read failed"), "{:?}", lock(&log.events));
    server.dispose();
}

#[test]
#[ignore = "native: starts a uniquely named test pipe server"]
fn the_server_keeps_its_pipe_until_the_client_hangs_up_and_a_busy_pipe_makes_the_next_client_wait() {
    // Mutants forward_server.rs:611:28 (`written == 0` -> `!=`, the reply write is reported as failed) and :621:13 (`wait_reply_delivered`
    // -> `Ok(())`): the server would release its single pipe instance right after the reply instead of holding it until the client hangs
    // up (bounded by `REPLY_DELIVERY_TIMEOUT`, 1 s). forward_client.rs:400:50 (`ERROR_PIPE_BUSY` check `==` -> `!=`): a busy pipe would be
    // reported as `NoServer` at once instead of being waited for.
    let name = unique_pipe_name("busy");
    let server = start_server(&name, None);
    let first = connect(&name);
    let deadline = Instant::now() + Duration::from_secs(10);
    first
        .write_all(&empty_request(), deadline, &CancelToken::none())
        .expect("request written");
    assert_eq!(
        first.read_reply(16, deadline, &CancelToken::none()),
        Ok(b"OK
"
        .to_vec())
    );
    // `first` stays connected, so the only pipe instance is busy until the server's delivery bound ends it.
    let started = Instant::now();
    let second = WindowsForwardTransport::default();
    second
        .connect(&name, Instant::now() + Duration::from_secs(10), &CancelToken::none())
        .expect("the busy pipe is waited for, then the next instance accepts the client");
    assert!(
        started.elapsed() >= Duration::from_millis(200),
        "the pipe instance was not held after the reply: {:?}",
        started.elapsed()
    );
    second.disconnect();
    first.disconnect();
    server.dispose();
}

#[test]
#[ignore = "native: reads the current process token"]
fn the_current_process_user_sid_is_a_well_formed_sid() {
    // Mutants forward_client.rs:212:9 `sid_bytes -> Some(vec![])/Some(vec![0])/Some(vec![1])` and :243:9 `process_user_sid -> Some(..)`:
    // a real SID is revision 1, a sub-authority count n (1..=15) and 8 + 4 * n bytes (S-1-5-21-... has at least 12 bytes, usually 28).
    let sid = current_user_sid_for_tests().expect("the current process token is readable");
    assert_eq!(sid[0], 1, "SID revision");
    let sub_authorities = usize::from(sid[1]);
    assert!((1..=15).contains(&sub_authorities), "{sid:?}");
    assert_eq!(sid.len(), 8 + 4 * sub_authorities, "{sid:?}");
}
