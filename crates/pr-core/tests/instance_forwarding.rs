//! Unit W2-19: instance forwarding core (protocol, coalescer, handoff). C# baseline d92d6aae:
//! `ForwardedPathProtocolLimitTests`, `ForwardingCoreTests`, `InstanceAndWorkerMutationGapTests` (instance half).
//!
//! Not ported: `Encode_PathWithLoneSurrogate_ThrowsArgumentException` (a Rust `&str` cannot hold a lone surrogate).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use pr_core::abstractions::Log;
use pr_core::instance::coalescer::{DelayFailure, DelayFn, ForwardedOpenCoalescer, OpenCallback, Spawner};
use pr_core::instance::forwarding::{ForwardError, ForwardOutcome, InstanceForwardClient};
use pr_core::instance::handoff::try_forward;
use pr_core::instance::protocol::{
    EncodeError, HEADER, MAX_MESSAGE_BYTES, MAX_PATH_CHARS, MAX_PATHS, encode, is_acceptable_path, is_complete,
    try_decode,
};
use pr_core::sync::{CancelToken, Canceled};
use pr_test_support::golden;
use pr_test_support::{LogLevel, RecordingLog};

const PHOTO: &str = r"C:\fwd\a.jpg";
const PHOTO2: &str = r"C:\fwd\b.jpg";
const TEN_SECONDS: Duration = Duration::from_secs(10);

fn exists(_: &str) -> bool {
    true
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn path_of_length(length: usize) -> String {
    format!(r"C:\{}", "a".repeat(length - 3))
}

fn raw(paths: &[&str]) -> Vec<u8> {
    let mut text = format!("{HEADER}\n");
    for p in paths {
        text.push_str(p);
        text.push('\n');
    }
    text.push('\n');
    text.into_bytes()
}

fn raw_owned(paths: &[String]) -> Vec<u8> {
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    raw(&refs)
}

fn two_paths_filling_to(total_bytes: usize) -> Vec<String> {
    let rest = total_bytes - 19 - 1 - 2;
    let first = MAX_PATH_CHARS;
    vec![path_of_length(first), path_of_length(rest - first)]
}

fn numbered(count: usize) -> Vec<String> {
    (0..count).map(|i| format!(r"C:\p{i}")).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// ForwardedPathProtocolLimitTests

#[test]
fn encode_exactly_max_paths_is_accepted_and_one_more_throws() {
    // C#: ForwardedPathProtocolLimitTests.Encode_ExactlyMaxPaths_IsAcceptedAndOneMoreThrows
    let mut sixteen = numbered(MAX_PATHS);
    let bytes = encode(&sixteen).unwrap();
    assert_eq!(try_decode(&bytes, exists), Some(sixteen.clone()));
    sixteen.push(r"C:\extra".to_owned());
    assert_eq!(encode(&sixteen), Err(EncodeError::TooManyPaths));
}

#[test]
fn encode_message_of_exactly_max_bytes_is_accepted_and_one_byte_more_throws() {
    // C#: ForwardedPathProtocolLimitTests.Encode_MessageOfExactlyMaxBytes_IsAcceptedAndOneByteMoreThrows
    let at_limit = two_paths_filling_to(MAX_MESSAGE_BYTES);
    assert_eq!(encode(&at_limit).unwrap().len(), MAX_MESSAGE_BYTES);
    let over = two_paths_filling_to(MAX_MESSAGE_BYTES + 1);
    assert_eq!(encode(&over), Err(EncodeError::MessageTooLarge));
}

#[test]
fn try_decode_message_of_exactly_max_bytes_is_accepted() {
    // C#: ForwardedPathProtocolLimitTests.TryDecode_MessageOfExactlyMaxBytes_IsAccepted
    let paths = two_paths_filling_to(MAX_MESSAGE_BYTES);
    let message = raw_owned(&paths);
    assert_eq!(message.len(), MAX_MESSAGE_BYTES);
    assert_eq!(try_decode(&message, exists), Some(paths));
}

#[test]
fn try_decode_complete_message_of_max_bytes_plus_one_is_rejected() {
    // C#: ForwardedPathProtocolLimitTests.TryDecode_CompleteMessageOfMaxBytesPlusOne_IsRejected
    let message = raw_owned(&two_paths_filling_to(MAX_MESSAGE_BYTES + 1));
    assert_eq!(message.len(), MAX_MESSAGE_BYTES + 1);
    assert_eq!(try_decode(&message, exists), None);
}

#[test]
fn try_decode_exactly_max_paths_is_accepted_and_max_plus_one_rejected() {
    // C#: ForwardedPathProtocolLimitTests.TryDecode_ExactlyMaxPaths_IsAcceptedAndMaxPlusOneRejected
    let sixteen = numbered(MAX_PATHS);
    let seventeen = numbered(MAX_PATHS + 1);
    assert_eq!(try_decode(&raw_owned(&sixteen), exists), Some(sixteen));
    assert_eq!(try_decode(&raw_owned(&seventeen), exists), None);
}

#[test]
fn try_decode_fifteen_paths_is_accepted() {
    // C#: ForwardedPathProtocolLimitTests.TryDecode_FifteenPaths_IsAccepted
    let fifteen = numbered(MAX_PATHS - 1);
    assert_eq!(try_decode(&raw_owned(&fifteen), exists), Some(fifteen));
}

#[test]
fn encode_path_of_exactly_max_path_chars_is_accepted_and_one_char_more_throws() {
    // C#: ForwardedPathProtocolLimitTests.Encode_PathOfExactlyMaxPathChars_IsAcceptedAndOneCharMoreThrows
    let at_limit = path_of_length(MAX_PATH_CHARS);
    assert_eq!(at_limit.len(), MAX_PATH_CHARS);
    let bytes = encode(std::slice::from_ref(&at_limit)).unwrap();
    assert_eq!(try_decode(&bytes, exists), Some(vec![at_limit]));
    assert!(matches!(
        encode(&[path_of_length(MAX_PATH_CHARS + 1)]),
        Err(EncodeError::PathNotAcceptable(_))
    ));
}

#[test]
fn try_decode_path_of_max_path_chars_plus_one_is_rejected() {
    // C#: ForwardedPathProtocolLimitTests.TryDecode_PathOfMaxPathCharsPlusOne_IsRejected
    let message = raw_owned(&[path_of_length(MAX_PATH_CHARS + 1)]);
    assert_eq!(try_decode(&message, exists), None);
}

#[test]
fn try_decode_empty_path_line_is_rejected() {
    // C#: ForwardedPathProtocolLimitTests.TryDecode_EmptyPathLine_IsRejected
    assert_eq!(try_decode(&raw(&[r"C:\a", ""]), exists), None);
}

#[test]
fn is_complete_only_a_message_ending_with_an_empty_line_is_complete() {
    // C#: ForwardedPathProtocolLimitTests.IsComplete_OnlyAMessageEndingWithAnEmptyLineIsComplete
    let rows: [(&str, bool); 11] = [
        ("", false),
        ("\n", false),
        ("a", false),
        ("ab", false),
        ("abc", false),
        ("x\n", false),
        ("ab\n", false),
        ("\nx", false),
        ("\n\n", true),
        ("x\n\n", true),
        ("PHOTOREVIEW-OPEN 1\n\n", true),
    ];
    for (text, expected) in rows {
        assert_eq!(is_complete(text.as_bytes()), expected, "{text:?}");
    }
}

#[test]
fn try_decode_empty_message_is_rejected_without_throwing() {
    // C#: ForwardedPathProtocolLimitTests.TryDecode_EmptyMessage_IsRejectedWithoutThrowing
    assert_eq!(try_decode(&[], exists), None);
}

#[test]
fn try_decode_unterminated_message_is_rejected_even_when_every_path_exists() {
    // C#: ForwardedPathProtocolLimitTests.TryDecode_UnterminatedMessage_IsRejectedEvenWhenEveryPathExists
    for text in [
        "PHOTOREVIEW-OPEN 1\n",
        "PHOTOREVIEW-OPEN 1\nC:\\a.jpg",
        "PHOTOREVIEW-OPEN 1\nC:\\a.jpg\n",
        "PHOTOREVIEW-OPEN 1\nC:\\a.jpg\nx",
    ] {
        assert_eq!(try_decode(text.as_bytes(), exists), None, "{text:?}");
    }
}

#[test]
fn try_decode_header_only_is_an_empty_valid_request() {
    // C#: ForwardedPathProtocolLimitTests.TryDecode_HeaderOnly_IsAnEmptyValidRequest
    assert_eq!(try_decode(&raw(&[]), exists), Some(vec![]));
}

#[test]
fn path_device_or_extended_namespace_is_rejected_by_encode_and_decode() {
    // C#: ForwardedPathProtocolLimitTests.Path_DeviceOrExtendedNamespace_IsRejectedByEncodeAndDecode
    for path in [
        r"\\?\C:\a.jpg",
        r"\\.\C:\a.jpg",
        "//?/C:/a.jpg",
        "//./C:/a.jpg",
        r"\??\C:\a.jpg",
        "/??/C:/a.jpg",
        r"\\?",
        r"\\.",
        "//?",
        "//.",
        r"\??",
    ] {
        assert!(encode(&[path.to_owned()]).is_err(), "{path:?}");
        assert_eq!(try_decode(&raw(&[path]), exists), None, "{path:?}");
    }
}

#[test]
fn path_one_or_two_characters_is_rejected_cleanly() {
    // C#: ForwardedPathProtocolLimitTests.Path_OneOrTwoCharacters_IsRejectedCleanly
    for path in ["a", "C:", r"\"] {
        assert!(encode(&[path.to_owned()]).is_err(), "{path:?}");
        assert_eq!(try_decode(&raw(&[path]), exists), None, "{path:?}");
    }
}

#[test]
fn path_two_separators_never_crashes_the_shape_check() {
    // C#: ForwardedPathProtocolLimitTests.Path_TwoSeparators_NeverCrashesTheShapeCheck
    // .NET calls "\\" fully qualified, so both are accepted by the shape check and the verdict is Ok / Some.
    for path in [r"\\", "//"] {
        assert!(encode(&[path.to_owned()]).is_ok(), "{path:?}");
        assert_eq!(
            try_decode(&raw(&[path]), exists),
            Some(vec![path.to_owned()]),
            "{path:?}"
        );
    }
}

#[test]
fn path_ordinary_fully_qualified_path_is_accepted() {
    // C#: ForwardedPathProtocolLimitTests.Path_OrdinaryFullyQualifiedPath_IsAccepted
    for path in [
        r"C:\a.jpg",
        r"C:\a\b.jpg",
        r"\\server\share\a.jpg",
        "//server/share/a.jpg",
        r"D:\x?.jpg.bak",
    ] {
        let bytes = encode(&[path.to_owned()]).unwrap();
        assert_eq!(try_decode(&bytes, exists), Some(vec![path.to_owned()]), "{path:?}");
    }
}

#[test]
fn try_decode_path_that_does_not_exist_is_rejected() {
    // C#: ForwardedPathProtocolLimitTests.TryDecode_PathThatDoesNotExist_IsRejected
    let bytes = encode(&[r"C:\a.jpg".to_owned()]).unwrap();
    assert_eq!(try_decode(&bytes, |_| false), None);
}

// ---------------------------------------------------------------------------------------------------------------
// ForwardingCoreTests: protocol

fn photo_exists(p: &str) -> bool {
    p == PHOTO || p == PHOTO2
}

#[test]
fn protocol_round_trip() {
    // C#: ForwardingCoreTests.Protocol_RoundTrip
    let bytes = encode(&strs(&[PHOTO, PHOTO2])).unwrap();
    assert_eq!(try_decode(&bytes, photo_exists), Some(strs(&[PHOTO, PHOTO2])));
}

#[test]
fn protocol_empty_list_is_valid() {
    // C#: ForwardingCoreTests.Protocol_EmptyList_IsValid
    assert_eq!(try_decode(&encode(&[]).unwrap(), photo_exists), Some(vec![]));
}

#[test]
fn protocol_rejects_bad_input() {
    // C#: ForwardingCoreTests.Protocol_RejectsBadInput
    for text in [
        "garbage\n\n",
        "PHOTOREVIEW-OPEN 2\n\n",
        "PHOTOREVIEW-OPEN 1\nrelative\\a.jpg\n\n",
        "PHOTOREVIEW-OPEN 1\n\\\\?\\C:\\x.jpg\n\n",
        "PHOTOREVIEW-OPEN 1\n\\\\.\\pipe\\x\n\n",
        "PHOTOREVIEW-OPEN 1\nC:\\does\\not\\exist.jpg\n\n",
        "PHOTOREVIEW-OPEN 1\nC:\\a\tb.jpg\n\n",
        "PHOTOREVIEW-OPEN 1\nC:\\a.jpg",
    ] {
        assert_eq!(try_decode(text.as_bytes(), photo_exists), None, "{text:?}");
    }
}

#[test]
fn protocol_rejects_hostile_prefixes_even_if_exists() {
    // C#: ForwardingCoreTests.Protocol_RejectsHostilePrefixes_EvenIfExists
    for path in [
        r"\\?\C:\x.jpg",
        r"\\.\C:\x.jpg",
        r"\??\C:\x.jpg",
        "//?/C:/x.jpg",
        "//./C:/x.jpg",
        r"C:\fwd\..\a.jpg",
        "C:/fwd/../a.jpg",
    ] {
        let message = format!("PHOTOREVIEW-OPEN 1\n{path}\n\n");
        assert_eq!(try_decode(message.as_bytes(), exists), None, "{path:?}");
    }
}

#[test]
fn protocol_encode_rejects_what_decode_would_reject() {
    // C#: ForwardingCoreTests.Protocol_Encode_RejectsWhatDecodeWouldReject
    for bad in [
        "C:\\a\tb.jpg",
        "relative\\a.jpg",
        r"C:\fwd\..\a.jpg",
        r"\\?\C:\x.jpg",
        r"\\.\C:\x.jpg",
        r"\??\C:\x.jpg",
        "",
    ] {
        assert_eq!(
            encode(&[bad.to_owned()]),
            Err(EncodeError::PathNotAcceptable(bad.to_owned())),
            "{bad:?}"
        );
        assert!(!is_acceptable_path(bad), "{bad:?}");
    }
}

#[test]
fn try_decode_bytes_of_lone_surrogate_returns_false() {
    // C#: ForwardingCoreTests.TryDecode_BytesOfLoneSurrogate_ReturnsFalse
    let mut bytes = b"PHOTOREVIEW-OPEN 1\nC:\\fwd\\a".to_vec();
    bytes.extend_from_slice(&[0xED, 0xA0, 0x80]);
    bytes.extend_from_slice(b"b.jpg\n\n");
    assert_eq!(try_decode(&bytes, exists), None);
}

#[test]
fn encode_valid_surrogate_pair_round_trips() {
    // C#: ForwardingCoreTests.Encode_ValidSurrogatePair_RoundTrips
    let path = format!("C:\\fwd\\a{}b.jpg", '\u{1F600}');
    let bytes = encode(std::slice::from_ref(&path)).unwrap();
    assert_eq!(try_decode(&bytes, exists), Some(vec![path]));
}

#[test]
fn protocol_encode_output_is_always_accepted_by_decode() {
    // C#: ForwardingCoreTests.Protocol_Encode_OutputIsAlwaysAcceptedByDecode
    for path in [
        r"C:\a.jpg",
        r"C:\folder with spaces\a.jpg",
        "C:\\ünïcödé\\日本語.jpg",
        r"C:\a.b.c.jpg",
        r"C:\.hidden\a.jpg",
        r"C:\a\b\c\d\e\f.jpg",
    ] {
        let bytes = encode(&[path.to_owned()]).unwrap();
        assert_eq!(try_decode(&bytes, exists), Some(vec![path.to_owned()]), "{path:?}");
    }
}

#[test]
fn protocol_rejects_invalid_utf8_oversize_too_many() {
    // C#: ForwardingCoreTests.Protocol_RejectsInvalidUtf8_Oversize_TooMany
    assert_eq!(try_decode(&[0xFF, 0xFE, b'\n', b'\n'], photo_exists), None);
    assert_eq!(try_decode(&vec![0u8; MAX_MESSAGE_BYTES + 1], photo_exists), None);

    let many = format!("PHOTOREVIEW-OPEN 1\n{}\n", format!("{PHOTO}\n").repeat(MAX_PATHS + 1));
    assert_eq!(try_decode(many.as_bytes(), photo_exists), None);
    assert_eq!(
        encode(&vec![PHOTO.to_owned(); MAX_PATHS + 1]),
        Err(EncodeError::TooManyPaths)
    );
}

#[test]
fn encode_checks_too_many_paths_before_looking_at_any_path() {
    // Contract note 2(a): count first, then each path, then size.
    let mut paths = vec!["bad".to_owned(); MAX_PATHS + 1];
    assert_eq!(encode(&paths), Err(EncodeError::TooManyPaths));
    paths.truncate(2);
    assert_eq!(encode(&paths), Err(EncodeError::PathNotAcceptable("bad".to_owned())));
}

// ---------------------------------------------------------------------------------------------------------------
// Coalescer helpers

/// One-shot gate a window delay can block on (never `sleep`).
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    cv: Condvar,
}

impl Gate {
    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.cv.notify_all();
    }

    fn wait(&self) {
        let guard = self.open.lock().unwrap();
        let (_guard, result) = self.cv.wait_timeout_while(guard, TEN_SECONDS, |open| !*open).unwrap();
        assert!(!result.timed_out(), "gate was never released");
    }
}

fn gated_delay(gate: Arc<Gate>) -> DelayFn {
    Arc::new(move |_, _| {
        gate.wait();
        Ok(())
    })
}

fn recording_open() -> (OpenCallback, mpsc::Receiver<Option<String>>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let open: OpenCallback = Arc::new(move |p| {
        let _ = tx.lock().unwrap().send(p);
    });
    (open, rx)
}

fn inline_spawner() -> Spawner {
    Arc::new(|f| f())
}

fn coalescer(open: OpenCallback, log: Option<Arc<dyn Log>>, delay: DelayFn) -> ForwardedOpenCoalescer {
    ForwardedOpenCoalescer::new(open, Duration::from_secs(1), log, Some(delay), None)
}

// ---------------------------------------------------------------------------------------------------------------
// ForwardingCoreTests: coalescer

#[test]
fn coalescer_burst_of_launches_opens_first_path_once() {
    // C#: ForwardingCoreTests.Coalescer_BurstOfLaunches_OpensFirstPathOnce
    let gate = Arc::new(Gate::default());
    let (open, rx) = recording_open();
    let c = coalescer(open, None, gated_delay(Arc::clone(&gate)));

    c.submit(&strs(&[PHOTO]));
    c.submit(&strs(&[PHOTO2]));
    c.submit(&strs(&[PHOTO2]));
    c.submit(&[]);
    assert!(rx.try_recv().is_err(), "still inside the window");
    gate.release();

    assert_eq!(rx.recv_timeout(TEN_SECONDS).unwrap(), Some(PHOTO.to_owned()));
    drop(c);
    assert!(rx.try_recv().is_err(), "exactly one open");
}

#[test]
fn coalescer_separate_bursts_are_separate_opens() {
    // C#: ForwardingCoreTests.Coalescer_SeparateBursts_AreSeparateOpens
    let gates = [Arc::new(Gate::default()), Arc::new(Gate::default())];
    let calls = Arc::new(AtomicUsize::new(0));
    let delay: DelayFn = {
        let gates = gates.clone();
        let calls = Arc::clone(&calls);
        Arc::new(move |_, _| {
            let i = calls.fetch_add(1, Ordering::SeqCst);
            gates[i].wait();
            Ok(())
        })
    };
    let (open, rx) = recording_open();
    let c = coalescer(open, None, delay);

    c.submit(&[]);
    c.submit(&strs(&[PHOTO]));
    gates[0].release();
    assert_eq!(rx.recv_timeout(TEN_SECONDS).unwrap(), Some(PHOTO.to_owned()));
    c.submit(&[]);
    gates[1].release();
    assert_eq!(rx.recv_timeout(TEN_SECONDS).unwrap(), None);
}

#[test]
fn coalescer_later_request_only_fills_an_empty_slot() {
    // Contract note 7 (`_first ??= first`): Some(a), None, Some(b) keeps a; None then Some(b) becomes b.
    let gate = Arc::new(Gate::default());
    let (open, rx) = recording_open();
    let c = coalescer(open, None, gated_delay(Arc::clone(&gate)));
    c.submit(&[]);
    c.submit(&strs(&[PHOTO2]));
    c.submit(&strs(&[PHOTO]));
    gate.release();
    assert_eq!(rx.recv_timeout(TEN_SECONDS).unwrap(), Some(PHOTO2.to_owned()));
}

#[test]
fn coalescer_submit_does_not_run_or_wait_for_the_open_callback() {
    // C#: ForwardingCoreTests.Coalescer_Submit_DoesNotRunOrWaitForTheOpenCallback
    let gate = Arc::new(Gate::default());
    let release_open = Arc::new(Gate::default());
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (finished_tx, finished_rx) = mpsc::channel::<()>();
    let open: OpenCallback = {
        let release_open = Arc::clone(&release_open);
        let started_tx = Mutex::new(started_tx);
        let finished_tx = Mutex::new(finished_tx);
        Arc::new(move |_| {
            let _ = started_tx.lock().unwrap().send(());
            release_open.wait();
            let _ = finished_tx.lock().unwrap().send(());
        })
    };
    let c = coalescer(open, None, gated_delay(Arc::clone(&gate)));

    c.submit(&strs(&[PHOTO]));
    assert!(started_rx.try_recv().is_err());

    gate.release();
    started_rx.recv_timeout(TEN_SECONDS).unwrap();
    release_open.release();
    finished_rx.recv_timeout(TEN_SECONDS).unwrap();
}

#[test]
fn coalescer_dispose_cancels_pending_open() {
    // C#: ForwardingCoreTests.Coalescer_Dispose_CancelsPendingOpen
    let opened = Arc::new(AtomicUsize::new(0));
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (cancelled_tx, cancelled_rx) = mpsc::channel::<()>();
    let delay: DelayFn = {
        let started_tx = Mutex::new(started_tx);
        let cancelled_tx = Mutex::new(cancelled_tx);
        Arc::new(move |_, cancel| {
            let _ = started_tx.lock().unwrap().send(());
            cancel.wait_canceled();
            let _ = cancelled_tx.lock().unwrap().send(());
            Err(DelayFailure::Canceled)
        })
    };
    let open: OpenCallback = {
        let opened = Arc::clone(&opened);
        Arc::new(move |_| {
            opened.fetch_add(1, Ordering::SeqCst);
        })
    };
    let c = coalescer(open, None, delay);

    c.submit(&strs(&[PHOTO]));
    started_rx.recv_timeout(TEN_SECONDS).unwrap();
    c.dispose();
    cancelled_rx.recv_timeout(TEN_SECONDS).unwrap();

    assert_eq!(opened.load(Ordering::SeqCst), 0);
}

#[test]
fn coalescer_default_delay_is_woken_by_dispose() {
    // Default delay + default spawner: dispose wakes the waiting window, nothing opens.
    let opened = Arc::new(AtomicUsize::new(0));
    let open: OpenCallback = {
        let opened = Arc::clone(&opened);
        Arc::new(move |_| {
            opened.fetch_add(1, Ordering::SeqCst);
        })
    };
    let c = ForwardedOpenCoalescer::new(open, Duration::from_secs(60), None, None, None);
    c.submit(&strs(&[PHOTO]));
    c.dispose();
    c.dispose();
    c.submit(&strs(&[PHOTO2]));
    assert_eq!(opened.load(Ordering::SeqCst), 0);
}

#[test]
fn coalescer_default_delay_and_spawner_open_after_the_window() {
    let (open, rx) = recording_open();
    let c = ForwardedOpenCoalescer::new(open, Duration::from_millis(20), None, None, None);
    c.submit(&strs(&[PHOTO]));
    assert_eq!(rx.recv_timeout(TEN_SECONDS).unwrap(), Some(PHOTO.to_owned()));
}

// ---------------------------------------------------------------------------------------------------------------
// ForwardingCoreTests: handoff

type SendFn = Box<dyn Fn(&[String]) -> Result<ForwardOutcome, ForwardError> + Send + Sync>;

struct FakeClient {
    send: SendFn,
    sent: Mutex<Vec<Vec<String>>>,
}

impl FakeClient {
    fn new(send: impl Fn(&[String]) -> Result<ForwardOutcome, ForwardError> + Send + Sync + 'static) -> Self {
        Self {
            send: Box::new(send),
            sent: Mutex::new(Vec::new()),
        }
    }

    fn returning(outcome: ForwardOutcome) -> Self {
        Self::new(move |_| Ok(outcome))
    }
}

impl InstanceForwardClient for FakeClient {
    fn send(
        &self,
        paths: &[String],
        _timeout: Duration,
        _cancel: &CancelToken,
    ) -> Result<ForwardOutcome, ForwardError> {
        self.sent.lock().unwrap().push(paths.to_vec());
        (self.send)(paths)
    }
}

#[test]
fn handoff_exits_only_when_the_owner_took_or_likely_took_the_request() {
    // C#: ForwardingCoreTests.Handoff_ExitsOnlyWhenTheOwnerTookOrLikelyTookTheRequest
    for (outcome, expected) in [
        (ForwardOutcome::Delivered, true),
        (ForwardOutcome::Unknown, true),
        (ForwardOutcome::Rejected, false),
        (ForwardOutcome::NoInstance, false),
    ] {
        let client = FakeClient::returning(outcome);
        let result = try_forward(
            &client,
            &strs(&[PHOTO]),
            Duration::from_secs(1),
            None,
            &CancelToken::none(),
        );
        assert_eq!(result, Ok(expected), "{outcome}");
        assert_eq!(*client.sent.lock().unwrap(), vec![strs(&[PHOTO])]);
    }
}

#[test]
fn handoff_client_throws_falls_back() {
    // C#: ForwardingCoreTests.Handoff_ClientThrows_FallsBack
    let client = FakeClient::new(|_| {
        Err(ForwardError::Failed {
            kind: "InvalidOperation".to_owned(),
        })
    });
    let result = try_forward(
        &client,
        &strs(&[PHOTO]),
        Duration::from_secs(1),
        None,
        &CancelToken::none(),
    );
    assert_eq!(result, Ok(false));
}

#[test]
fn handoff_client_panics_falls_back_with_a_warning() {
    let log = RecordingLog::new();
    let client = FakeClient::new(|_| panic!("pipe broke"));
    let result = try_forward(&client, &[], Duration::from_secs(1), Some(&log), &CancelToken::none());
    assert_eq!(result, Ok(false));
    assert!(log.contains(LogLevel::Warn, "Forwarding to the running instance failed: Panic"));
}

#[test]
fn handoff_canceled_propagates_and_logs_nothing() {
    let log = RecordingLog::new();
    let client = FakeClient::new(|_| Err(ForwardError::Canceled));
    let result = try_forward(&client, &[], Duration::from_secs(1), Some(&log), &CancelToken::none());
    assert_eq!(result, Err(Canceled));
    assert!(log.entries().is_empty());
}

// ---------------------------------------------------------------------------------------------------------------
// InstanceAndWorkerMutationGapTests (instance half)

#[test]
fn try_forward_outcome_is_logged_with_the_path_count_and_mapped_to_the_exit_decision() {
    // C#: InstanceAndWorkerMutationGapTests.TryForward_Outcome_IsLoggedWithThePathCountAndMappedToTheExitDecision
    for (outcome, expected) in [
        (ForwardOutcome::Delivered, true),
        (ForwardOutcome::Unknown, true),
        (ForwardOutcome::Rejected, false),
        (ForwardOutcome::NoInstance, false),
    ] {
        let log = RecordingLog::new();
        let client = FakeClient::returning(outcome);
        let result = try_forward(
            &client,
            &strs(&[r"C:\a.jpg", r"C:\b.jpg"]),
            Duration::from_secs(1),
            Some(&log),
            &CancelToken::none(),
        );
        assert_eq!(result, Ok(expected));
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, LogLevel::Info);
        assert!(entries[0].1.contains(outcome.name()));
        assert!(entries[0].1.contains("2 path(s)"));
    }
}

#[test]
fn try_forward_client_throws_logs_a_warning_naming_the_exception_and_returns_false() {
    // C#: InstanceAndWorkerMutationGapTests.TryForward_ClientThrows_LogsAWarningNamingTheExceptionAndReturnsFalse
    let log = RecordingLog::new();
    let client = FakeClient::new(|_| {
        Err(ForwardError::Failed {
            kind: "InvalidOperation".to_owned(),
        })
    });
    let result = try_forward(&client, &[], Duration::from_secs(1), Some(&log), &CancelToken::none());
    assert_eq!(result, Ok(false));
    let entries = log.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, LogLevel::Warn);
    assert!(entries[0].1.contains("InvalidOperation"));
}

#[test]
fn coalescer_open_callback_throws_is_logged_and_the_next_submit_still_works() {
    // C#: InstanceAndWorkerMutationGapTests.Coalescer_OpenCallbackThrows_IsLoggedAndTheNextSubmitStillWorks
    let log = Arc::new(RecordingLog::new());
    let opened = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let attempts = Arc::new(AtomicUsize::new(0));
    let open: OpenCallback = {
        let opened = Arc::clone(&opened);
        let attempts = Arc::clone(&attempts);
        Arc::new(move |p| {
            opened.lock().unwrap().push(p);
            if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("window gone");
            }
        })
    };
    let delay: DelayFn = Arc::new(|_, _| Ok(()));
    let log_dyn: Arc<dyn Log> = log.clone();
    let c = ForwardedOpenCoalescer::new(
        open,
        Duration::from_secs(1),
        Some(log_dyn),
        Some(delay),
        Some(inline_spawner()),
    );

    // The injected delay is already complete and the spawner inline, so the whole flush runs inside submit.
    c.submit(&strs(&[r"C:\a.jpg"]));
    assert_eq!(log.entries().len(), 1);
    c.submit(&strs(&[r"C:\b.jpg"]));

    assert_eq!(
        *opened.lock().unwrap(),
        vec![Some(r"C:\a.jpg".to_owned()), Some(r"C:\b.jpg".to_owned())]
    );
    assert!(log.contains(LogLevel::Error, "Forwarded open failed"));
    assert!(log.contains(LogLevel::Error, "window gone"));
}

#[test]
fn coalescer_window_delay_fails_is_logged_and_the_open_still_happens() {
    // C#: InstanceAndWorkerMutationGapTests.Coalescer_WindowDelayFails_IsLoggedAndTheOpenStillHappens
    let log = Arc::new(RecordingLog::new());
    let (open, rx) = recording_open();
    let delay: DelayFn = Arc::new(|_, _| Err(DelayFailure::Failed("timer broke".to_owned())));
    let log_dyn: Arc<dyn Log> = log.clone();
    let c = coalescer(open, Some(log_dyn), delay);

    c.submit(&strs(&[r"C:\a.jpg"]));

    assert_eq!(rx.recv_timeout(TEN_SECONDS).unwrap(), Some(r"C:\a.jpg".to_owned()));
    assert!(log.contains(LogLevel::Error, "window failed"));
}

#[test]
fn coalescer_window_is_cancelled_without_dispose_does_not_open_anything() {
    // C#: InstanceAndWorkerMutationGapTests.Coalescer_WindowIsCancelledWithoutDispose_DoesNotOpenAnything
    let opened = Arc::new(AtomicUsize::new(0));
    let open: OpenCallback = {
        let opened = Arc::clone(&opened);
        Arc::new(move |_| {
            opened.fetch_add(1, Ordering::SeqCst);
        })
    };
    let delay: DelayFn = Arc::new(|_, _| Err(DelayFailure::Canceled));
    let c = ForwardedOpenCoalescer::new(open, Duration::from_secs(1), None, Some(delay), Some(inline_spawner()));

    c.submit(&strs(&[r"C:\a.jpg"]));
    // pending stays true, so a later request is swallowed too (C# behaviour).
    c.submit(&strs(&[r"C:\b.jpg"]));

    assert_eq!(opened.load(Ordering::SeqCst), 0);
}

#[test]
fn coalescer_cancelled_window_without_dispose_swallows_later_requests_even_when_delay_would_succeed() {
    // Rust-only guard for the same C# test: `pending` must stay true after a cancelled window (contract note 7).
    let opened = Arc::new(AtomicUsize::new(0));
    let open: OpenCallback = {
        let opened = Arc::clone(&opened);
        Arc::new(move |_| {
            opened.fetch_add(1, Ordering::SeqCst);
        })
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let delay: DelayFn = {
        let calls = Arc::clone(&calls);
        Arc::new(move |_, _| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(DelayFailure::Canceled)
            } else {
                Ok(())
            }
        })
    };
    let c = ForwardedOpenCoalescer::new(open, Duration::from_secs(1), None, Some(delay), Some(inline_spawner()));
    c.submit(&strs(&[r"C:.jpg"]));
    c.submit(&strs(&[r"C:.jpg"]));
    assert_eq!(opened.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn coalescer_submit_after_dispose_does_not_even_start_a_window() {
    // Rust-only guard: C# `Submit` returns at once when disposed, so neither spawner nor delay runs.
    let spawned = Arc::new(AtomicUsize::new(0));
    let delayed = Arc::new(AtomicUsize::new(0));
    let open: OpenCallback = Arc::new(|_| {});
    let spawner: Spawner = {
        let spawned = Arc::clone(&spawned);
        Arc::new(move |f| {
            spawned.fetch_add(1, Ordering::SeqCst);
            f()
        })
    };
    let delay: DelayFn = {
        let delayed = Arc::clone(&delayed);
        Arc::new(move |_, _| {
            delayed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    };
    let c = ForwardedOpenCoalescer::new(open, Duration::from_secs(1), None, Some(delay), Some(spawner));
    c.dispose();
    c.submit(&strs(&[r"C:.jpg"]));
    assert_eq!(spawned.load(Ordering::SeqCst), 0);
    assert_eq!(delayed.load(Ordering::SeqCst), 0);
}

// ---------------------------------------------------------------------------------------------------------------
// Golden: forwarded-path.json

fn golden_path(value: &serde_json::Value) -> String {
    if let Some(s) = value.as_str() {
        return s.to_owned();
    }
    let prefix = golden::str_of(value, "prefix");
    let fill: Vec<char> = golden::str_of(value, "fill").chars().collect();
    let length = golden::i64_of(value, "length") as usize;
    let mut out = String::from(prefix);
    let mut units = prefix.encode_utf16().count();
    let mut i = 0;
    while units < length {
        let c = fill[i % fill.len()];
        out.push(c);
        units += c.len_utf16();
        i += 1;
    }
    out
}

fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn unhex(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn golden_forwarded_path() {
    let g = golden::load("forwarded-path");
    let mut seen = 0;
    for case in &g.cases {
        let id = golden::str_of(case, "id");
        match golden::str_of(case, "op") {
            "constants" => {
                assert_eq!(HEADER, golden::str_of(case, "header"), "{id}");
                assert_eq!(
                    MAX_MESSAGE_BYTES as i64,
                    golden::i64_of(case, "max_message_bytes"),
                    "{id}"
                );
                assert_eq!(MAX_PATHS as i64, golden::i64_of(case, "max_paths"), "{id}");
                assert_eq!(MAX_PATH_CHARS as i64, golden::i64_of(case, "max_path_chars"), "{id}");
            }
            "encode" => {
                let paths: Vec<String> = case["paths"].as_array().unwrap().iter().map(golden_path).collect();
                if let Some(lengths) = case["path_lengths"].as_array() {
                    let actual: Vec<i64> = paths.iter().map(|p| p.encode_utf16().count() as i64).collect();
                    let expected: Vec<i64> = lengths.iter().map(|v| v.as_i64().unwrap()).collect();
                    assert_eq!(actual, expected, "{id}: path_lengths");
                }
                let result = encode(&paths);
                if golden::bool_of(case, "ok") {
                    let bytes = result.unwrap_or_else(|e| panic!("{id}: {e}"));
                    assert_eq!(bytes.len() as i64, golden::i64_of(case, "length"), "{id}: length");
                    if let Some(hex) = case["hex"].as_str() {
                        assert_eq!(hex_of(&bytes), hex, "{id}: hex");
                    }
                } else {
                    assert_eq!(golden::str_of(case, "param"), "paths", "{id}");
                    assert!(result.is_err(), "{id}: expected an EncodeError");
                }
            }
            "decode" => {
                let message: Vec<u8> = match case["hex"].as_str() {
                    Some(hex) => unhex(hex),
                    None => golden::str_of(case, "text").as_bytes().to_vec(),
                };
                let existing: Option<Vec<String>> = case["exists"]
                    .as_array()
                    .map(|a| a.iter().map(|v| v.as_str().unwrap().to_owned()).collect());
                let none = case["exists"].as_str() == Some("none");
                let mut calls: Vec<String> = Vec::new();
                let result = try_decode(&message, |p| {
                    calls.push(p.to_owned());
                    match &existing {
                        Some(list) => list.iter().any(|e| e == p),
                        None => !none,
                    }
                });
                let expected_calls: Vec<String> = case["exists_calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_owned())
                    .collect();
                assert_eq!(calls, expected_calls, "{id}: exists_calls");
                let expected_paths: Vec<String> = case["paths"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_owned())
                    .collect();
                assert_eq!(result.is_some(), golden::bool_of(case, "ok"), "{id}: ok");
                assert_eq!(result.unwrap_or_default(), expected_paths, "{id}: paths");
            }
            "complete" => {
                let text = golden::str_of(case, "text");
                assert_eq!(is_complete(text.as_bytes()), golden::bool_of(case, "complete"), "{id}");
            }
            other => panic!("{id}: unknown op {other}"),
        }
        seen += 1;
    }
    assert_eq!(seen, 114);
}
