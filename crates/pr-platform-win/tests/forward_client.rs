#[path = "../src/forward_client.rs"]
mod forward_client;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use forward_client::{ForwardClientTransport, ForegroundPermissionError, ForwardPipeClient, ForwardTransportError};
use pr_core::instance::forwarding::{ForwardError, ForwardOutcome};
use pr_core::instance::protocol;
use pr_core::sync::CancelToken;

#[derive(Clone, Default)]
struct FakeTransport(Arc<Mutex<State>>);

struct State {
    connect: Result<(), ForwardTransportError>,
    write: Result<(), ForwardTransportError>,
    reply: Result<Vec<u8>, ForwardTransportError>,
    request: Vec<u8>,
    foreground_calls: usize,
    foreground_result: Result<(), ForegroundPermissionError>,
    disconnect_calls: usize,
    cancel_on_read: Option<CancelToken>,
    server_is_current_user: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            connect: Ok(()),
            write: Ok(()),
            reply: Ok(Vec::new()),
            request: Vec::new(),
            foreground_calls: 0,
            foreground_result: Ok(()),
            disconnect_calls: 0,
            cancel_on_read: None,
            server_is_current_user: true,
        }
    }
}

impl ForwardClientTransport for FakeTransport {
    fn connect(&self, _: &str, _: Instant, cancel: &CancelToken) -> Result<(), ForwardTransportError> {
        if cancel.is_canceled() { return Err(ForwardTransportError::Failed); }
        self.0.lock().unwrap().connect
    }
    fn server_is_current_user(&self) -> bool { self.0.lock().unwrap().server_is_current_user }
    fn allow_server_foreground(&self) -> Result<(), ForegroundPermissionError> {
        let mut state = self.0.lock().unwrap();
        state.foreground_calls += 1;
        state.foreground_result
    }
    fn write_all(&self, bytes: &[u8], _: Instant, cancel: &CancelToken) -> Result<(), ForwardTransportError> {
        if cancel.is_canceled() { return Err(ForwardTransportError::Failed); }
        let mut state = self.0.lock().unwrap();
        state.request = bytes.to_vec();
        state.write
    }
    fn read_reply(&self, limit: usize, _: Instant, cancel: &CancelToken) -> Result<Vec<u8>, ForwardTransportError> {
        if cancel.is_canceled() { return Err(ForwardTransportError::Failed); }
        let (reply, cancel_on_read) = {
            let state = self.0.lock().unwrap();
            (state.reply.clone()?, state.cancel_on_read.clone())
        };
        if let Some(token) = cancel_on_read { token.cancel(); }
        let length = reply.iter().position(|byte| *byte == b'\n').map_or(reply.len(), |index| index + 1);
        Ok(reply.into_iter().take(length.min(limit)).collect())
    }
    fn disconnect(&self) { self.0.lock().unwrap().disconnect_calls += 1; }
}

fn client(fake: FakeTransport, foreground: bool) -> ForwardPipeClient<FakeTransport> {
    ForwardPipeClient::with_transport(Some("test-only-pipe".to_owned()), foreground, fake).unwrap()
}
fn path(name: &str) -> String { format!(r"C:\photos\{name}.jpg") }

#[test]
fn send_ok_reply_is_delivered() {
    // C# twin: OkReply_IsDelivered.
    let fake = FakeTransport::default(); fake.0.lock().unwrap().reply = Ok(b"OK\n".to_vec());
    assert_eq!(client(fake, false).send_with_timeout(&[path("one")], Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::Delivered));
}

#[test]
fn send_non_ok_reply_is_rejected() {
    // C# twin: NonOkReply_IsRejected.
    let fake = FakeTransport::default(); fake.0.lock().unwrap().reply = Ok(b"ERR\n".to_vec());
    assert_eq!(client(fake, false).send_with_timeout(&[path("one")], Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::Rejected));
}

#[test]
fn send_reply_with_ok_prefix_but_different_first_line_is_rejected() {
    // C# twin: ReplyNotStartingWithOk_IsRejected; this enforces the Rust contract's exact first-line match.
    let fake = FakeTransport::default(); fake.0.lock().unwrap().reply = Ok(b"OKAY\n".to_vec());
    assert_eq!(client(fake, false).send_with_timeout(&[path("one")], Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::Rejected));
}

#[test]
fn send_invalid_path_is_rejected_without_connecting() {
    // C# twin: UnencodablePath_IsRejectedWithoutConnecting.
    let fake = FakeTransport::default();
    assert_eq!(client(fake.clone(), false).send_with_timeout(&["relative.jpg".to_owned()], Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::Rejected));
    assert_eq!(fake.0.lock().unwrap().disconnect_calls, 1);
}

#[test]
fn send_no_server_is_no_instance() {
    // C# twin: NoServer_IsNoInstance.
    let fake = FakeTransport::default(); fake.0.lock().unwrap().connect = Err(ForwardTransportError::NoServer);
    assert_eq!(client(fake, false).send_with_timeout(&[], Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::NoInstance));
}

#[test]
fn send_connect_timeout_is_no_instance() {
    // C# twin: NoServer_IsNoInstance.
    let fake = FakeTransport::default(); fake.0.lock().unwrap().connect = Err(ForwardTransportError::TimedOut);
    assert_eq!(client(fake, false).send_with_timeout(&[], Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::NoInstance));
}

#[test]
fn send_failure_after_request_write_is_unknown() {
    // C# twin: HangupAfterRead_IsUnknown and Client_OwnerHangsUpSilentlyAfterWrite_ReportsUnknown.
    let fake = FakeTransport::default(); fake.0.lock().unwrap().reply = Err(ForwardTransportError::Failed);
    assert_eq!(client(fake, false).send_with_timeout(&[path("one")], Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::Unknown));
}

#[test]
fn send_eof_after_request_write_is_unknown() {
    // C# twin: Client_OwnerHangsUpSilentlyAfterWrite_ReportsUnknown.
    let fake = FakeTransport::default(); fake.0.lock().unwrap().reply = Ok(Vec::new());
    assert_eq!(client(fake, false).send_with_timeout(&[path("one")], Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::Unknown));
}

#[test]
fn send_timeout_after_request_write_is_unknown() {
    // C# twin: NoReplyBeforeDeadline_IsUnknown and Client_OwnerAnswersTooLate_ReportsUnknown.
    let fake = FakeTransport::default(); fake.0.lock().unwrap().reply = Err(ForwardTransportError::TimedOut);
    assert_eq!(client(fake, false).send_with_timeout(&[path("one")], Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::Unknown));
}

#[test]
fn send_caller_cancellation_propagates() {
    // C# twin: CallerCancellation_Propagates and AlreadyCancelledToken_Propagates.
    let fake = FakeTransport::default(); let token = CancelToken::new();
    fake.0.lock().unwrap().reply = Ok(b"OK\n".to_vec());
    fake.0.lock().unwrap().cancel_on_read = Some(token.clone());
    assert_eq!(client(fake, false).send_with_timeout(&[], Duration::from_secs(1), &token), Err(ForwardError::Canceled));
    let fake = FakeTransport::default(); token.cancel();
    assert_eq!(client(fake, false).send_with_timeout(&[], Duration::from_secs(1), &token), Err(ForwardError::Canceled));
}

#[test]
fn send_request_write_failure_before_any_bytes_is_no_instance() {
    // C# twin: Client_NoServer_ReportsNoInstance.
    let fake = FakeTransport::default(); fake.0.lock().unwrap().write = Err(ForwardTransportError::WriteFailed { bytes_may_have_reached_server: false });
    assert_eq!(client(fake, false).send_with_timeout(&[path("one")], Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::NoInstance));
}

#[test]
fn send_partial_request_write_failure_is_unknown() {
    // C# twin: Client_OwnerAcceptsThenHangsUpBeforeReplying_ReportsUnknown.
    let fake = FakeTransport::default(); fake.0.lock().unwrap().write = Err(ForwardTransportError::WriteFailed { bytes_may_have_reached_server: true });
    assert_eq!(client(fake, false).send_with_timeout(&[path("one")], Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::Unknown));
}

#[test]
fn send_trims_to_max_paths_and_preserves_order() {
    // C# twin: MoreThanMaxPaths_AreTrimmedAndDelivered.
    let fake = FakeTransport::default(); fake.0.lock().unwrap().reply = Ok(b"OK\n".to_vec());
    let paths: Vec<_> = (0..20).map(|index| path(&index.to_string())).collect();
    assert_eq!(client(fake.clone(), false).send_with_timeout(&paths, Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::Delivered));
    let expected = protocol::encode(&paths[..16]).unwrap();
    assert_eq!(fake.0.lock().unwrap().request, expected);
}

#[test]
fn send_foreground_permission_is_best_effort() {
    // C# twin: AllowServerForeground_StillDelivers.
    let fake = FakeTransport::default(); fake.0.lock().unwrap().reply = Ok(b"OK\n".to_vec());
    assert_eq!(client(fake.clone(), true).send_with_timeout(&[], Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::Delivered));
    assert_eq!(fake.0.lock().unwrap().foreground_calls, 1);
}

#[test]
fn send_foreground_api_errors_do_not_change_delivery_outcome() {
    // C# twin: AllowServerForeground_StillDelivers, including missing-library and invalid-operation paths.
    for error in [
        ForegroundPermissionError::LibraryUnavailable,
        ForegroundPermissionError::EntryPointUnavailable,
        ForegroundPermissionError::InvalidOperation,
    ] {
        let fake = FakeTransport::default();
        {
            let mut state = fake.0.lock().unwrap();
            state.reply = Ok(b"OK\n".to_vec());
            state.foreground_result = Err(error);
        }
        assert_eq!(client(fake.clone(), true).send_with_timeout(&[], Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::Delivered));
        assert_eq!(fake.0.lock().unwrap().foreground_calls, 1);
    }
}

#[test]
fn send_pipe_owned_by_another_user_is_rejected_before_write() {
    // C# twin: NamedPipeClientStream.CurrentUserOnly refuses a pipe owned by a different user.
    let fake = FakeTransport::default();
    fake.0.lock().unwrap().server_is_current_user = false;
    assert_eq!(client(fake.clone(), true).send_with_timeout(&[path("one")], Duration::from_secs(1), &CancelToken::none()), Ok(ForwardOutcome::NoInstance));
    let state = fake.0.lock().unwrap();
    assert!(state.request.is_empty());
    assert_eq!(state.foreground_calls, 0);
}

#[test]
fn constructor_rejects_null_pipe_name() {
    // C# twin: NullPipeName_Throws.
    assert_eq!(ForwardPipeClient::with_transport(None, false, FakeTransport::default()).unwrap_err(), ForwardError::Failed { kind: "ArgumentNullException".to_owned() });
}
