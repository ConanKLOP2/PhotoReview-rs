//! Red tests for `pr_app::misc::task_logging` (contract W8-04d). Port of `PhotoReview.App.Shared/TaskLogging.cs` (C#
//! baseline d92d6aae) and `PhotoReview.App.Tests/TaskLoggingTests.cs` (all four tests) plus the cancelled-not-logged part
//! of `AppLogAndServiceGapTests.FireAndLog_CancelledTask_IsNotLoggedButAFaultIs`.
//!
//! DIVERGENCE (trap 12): the C# `ArgumentNullException` checks (`FireAndLog_CompletedAndCancelledTasks_DoNotThrow`'s null
//! task, `FireAndLogStart_SynchronousThrow_DoesNotEscape`'s null delegate) cannot occur for Rust references and are not
//! ported. The non-null parts of those tests are ported below.
//!
//! Waiting for the observer thread uses a bounded channel receive, never a sleep (AGENTS.md, Tests).

use std::error::Error;
use std::fmt;
use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use pr_app::misc::task_logging::{StartFault, TaskEnd, base_error, observe_end, observe_receiver, start_and_observe};
use pr_core::abstractions::Log;
use pr_test_support::{LogLevel, RecordingLog};

/// Hang bound for the observer thread, not a speed budget.
const BOUND: Duration = Duration::from_secs(120);

/// The innermost error of the chain.
#[derive(Debug)]
struct Inner(&'static str);

impl fmt::Display for Inner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl Error for Inner {}

/// A wrapper whose `source()` is [`Inner`]: the logged text must be the inner one (C# `GetBaseException`).
#[derive(Debug)]
struct Wrapper {
    inner: Inner,
}

impl fmt::Display for Wrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("outer-wrap")
    }
}

impl Error for Wrapper {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        let inner: &(dyn Error + 'static) = &self.inner;
        Some(inner)
    }
}

fn wrapped_fault() -> TaskEnd {
    TaskEnd::Faulted(Box::new(Wrapper {
        inner: Inner("root-cause"),
    }))
}

/// A log that records like [`RecordingLog`] and signals `done` after every error, so a test can wait for an observer
/// thread without sleeping.
struct Signal {
    recording: Arc<RecordingLog>,
    done: Mutex<Sender<()>>,
}

impl Log for Signal {
    fn enabled(&self) -> bool {
        true
    }

    fn info(&self, message: &str) {
        self.recording.info(message);
    }

    fn warn(&self, message: &str) {
        self.recording.warn(message);
    }

    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        self.recording.error(message, error);
        let _ = self.done.lock().unwrap_or_else(PoisonError::into_inner).send(());
    }
}

fn signalled_log() -> (Arc<RecordingLog>, Arc<dyn Log>, Receiver<()>) {
    let recording = Arc::new(RecordingLog::new());
    let (tx, rx) = mpsc::channel();
    let log: Arc<dyn Log> = Arc::new(Signal {
        recording: Arc::clone(&recording),
        done: Mutex::new(tx),
    });
    (recording, log, rx)
}

#[test]
fn faulted_end_is_logged_with_context_and_still_visible_to_the_caller() {
    // C#: TaskLoggingTests.FireAndLog_FaultedTask_StillReportsTheFaultToAwaiters
    let recording = RecordingLog::new();
    let end = wrapped_fault();

    observe_end(&end, "test context", &recording);

    assert!(recording.contains(LogLevel::Error, "test context"));
    // Rule 18 and 19: the BASE error is logged, not the wrapper.
    assert!(recording.contains(LogLevel::Error, "root-cause"));
    assert!(!recording.contains(LogLevel::Error, "outer-wrap"));
    // Rule 22: the caller keeps the outcome and still sees the fault.
    match &end {
        TaskEnd::Faulted(error) => assert_eq!(error.to_string(), "outer-wrap"),
        other => panic!("the caller lost the fault: {other:?}"),
    }
}

#[test]
fn completed_and_canceled_ends_are_not_logged() {
    // C#: TaskLoggingTests.FireAndLog_CompletedAndCancelledTasks_DoNotThrow (the non-null part)
    // C#: AppLogAndServiceGapTests.FireAndLog_CancelledTask_IsNotLoggedButAFaultIs (the synchronous part)
    let recording = RecordingLog::new();

    observe_end(&TaskEnd::Completed, "ok", &recording);
    observe_end(&TaskEnd::Canceled, "cancelled", &recording);

    assert!(recording.entries().is_empty());
}

#[test]
fn cancelled_observer_thread_is_silent_while_a_fault_is_logged() {
    // C#: AppLogAndServiceGapTests.FireAndLog_CancelledTask_IsNotLoggedButAFaultIs (the threaded part).
    // Limitation: a silent thread cannot be awaited; the fault is the sync point, so a late wrong log from the cancelled
    // observer would be missed by this check. observe_end's test above and the mutation check cover that path.
    let (recording, log, done) = signalled_log();
    let (cancel_tx, cancel_rx) = mpsc::channel::<TaskEnd>();
    let (fault_tx, fault_rx) = mpsc::channel::<TaskEnd>();

    observe_receiver(cancel_rx, "ctx-cancelled".to_owned(), Arc::clone(&log)).expect("the observer thread starts");
    observe_receiver(fault_rx, "ctx-faulted".to_owned(), log).expect("the observer thread starts");
    cancel_tx
        .send(TaskEnd::Canceled)
        .expect("the cancelled observer is alive");
    fault_tx.send(wrapped_fault()).expect("the faulted observer is alive");
    done.recv_timeout(BOUND).expect("the fault observer reports");

    assert!(recording.contains(LogLevel::Error, "ctx-faulted"));
    assert!(!recording.contains(LogLevel::Error, "ctx-cancelled"));
}

#[test]
fn start_sync_failure_is_logged_once_and_cancellation_is_silent() {
    // C#: TaskLoggingTests.FireAndLogStart_SynchronousThrow_DoesNotEscape (the ObjectDisposed and Canceled throws)
    let recording = Arc::new(RecordingLog::new());
    let log: Arc<dyn Log> = recording.clone();

    start_and_observe(
        || Err(StartFault::Failed(Box::new(Inner("disposed")))),
        "disposed context",
        Arc::clone(&log),
    );
    assert_eq!(recording.entries().len(), 1, "a failed start is logged exactly once");
    assert!(recording.contains(LogLevel::Error, "disposed context"));
    assert!(recording.contains(LogLevel::Error, "disposed"));

    start_and_observe(|| Err(StartFault::Canceled), "cancelled context", log);
    assert_eq!(recording.entries().len(), 1, "a cancelled start is silent");
}

#[test]
fn start_panic_is_logged_as_a_fault() {
    // ADR R0003 (new): a panic in the start closure is a fault, logged with its payload text.
    let recording = Arc::new(RecordingLog::new());
    let log: Arc<dyn Log> = recording.clone();

    start_and_observe(|| panic!("start-panic-marker"), "panicked context", log);

    assert!(recording.contains(LogLevel::Error, "panicked context"));
    assert!(recording.contains(LogLevel::Error, "start-panic-marker"));
}

#[test]
fn absent_handle_is_a_no_op() {
    // C#: TaskLoggingTests.FireAndLogStart_SynchronousThrow_DoesNotEscape (the "absent collaborator" null task)
    let recording = Arc::new(RecordingLog::new());
    let log: Arc<dyn Log> = recording.clone();

    start_and_observe(|| Ok(None), "absent collaborator", log);

    assert!(recording.entries().is_empty());
}

#[test]
fn returned_handle_fault_is_logged_when_observed() {
    // C#: TaskLoggingTests.FireAndLogStart_ReturnedTaskFaults_StillReportsToAwaiters (the observed side)
    let (recording, log, done) = signalled_log();
    let (tx, rx) = mpsc::channel::<TaskEnd>();

    observe_receiver(rx, "returned context".to_owned(), log).expect("the observer thread starts");
    tx.send(wrapped_fault()).expect("the observer is alive");
    done.recv_timeout(BOUND).expect("the observer reports the fault");

    assert!(recording.contains(LogLevel::Error, "returned context"));
    assert!(recording.contains(LogLevel::Error, "root-cause"));
}

#[test]
fn base_error_returns_the_innermost_source() {
    // Trap 19 (new): base_error walks source() to the end; an error without a source is its own base.
    let wrapped = Wrapper {
        inner: Inner("root-cause"),
    };
    assert_eq!(base_error(&wrapped).to_string(), "root-cause");

    let plain = io::Error::other("plain");
    assert_eq!(base_error(&plain).to_string(), "plain");
}

#[test]
fn dropped_sender_is_silent() {
    // Rule 21 (new): a sender dropped without sending is silent. Checked with a later observed fault as the sync point
    // (a silent thread has nothing to wait for); see the limitation note in cancelled_observer_thread_is_silent_...
    let (recording, log, done) = signalled_log();
    let (dropped_tx, dropped_rx) = mpsc::channel::<TaskEnd>();
    let (fault_tx, fault_rx) = mpsc::channel::<TaskEnd>();

    observe_receiver(dropped_rx, "dropped context".to_owned(), Arc::clone(&log)).expect("the observer thread starts");
    drop(dropped_tx);
    observe_receiver(fault_rx, "fault context".to_owned(), log).expect("the observer thread starts");
    fault_tx.send(wrapped_fault()).expect("the observer is alive");
    done.recv_timeout(BOUND).expect("the fault observer reports");

    assert!(recording.contains(LogLevel::Error, "fault context"));
    assert!(!recording.contains(LogLevel::Error, "dropped context"));
}

#[test]
fn start_failure_logs_the_exception_itself_not_its_base() {
    // C# catch arm: AppLog.Error(context, ex), NOT GetBaseException (only the task-fault path unwraps).
    let recording = Arc::new(RecordingLog::new());
    let log: Arc<dyn Log> = recording.clone();

    start_and_observe(
        || {
            Err(StartFault::Failed(Box::new(Wrapper {
                inner: Inner("root-cause"),
            })))
        },
        "wrapped start",
        log,
    );

    assert!(recording.contains(LogLevel::Error, "outer-wrap"));
}

#[test]
fn debug_text_of_task_end_and_start_fault() {
    assert_eq!(format!("{:?}", TaskEnd::Completed), "Completed");
    assert_eq!(format!("{:?}", TaskEnd::Canceled), "Canceled");
    assert_eq!(
        format!("{:?}", TaskEnd::Faulted(Box::new(Inner("boom")))),
        "Faulted(\"boom\")"
    );
    assert_eq!(format!("{:?}", StartFault::Canceled), "Canceled");
    assert_eq!(
        format!("{:?}", StartFault::Failed(Box::new(Inner("bad start")))),
        "Failed(\"bad start\")"
    );
}
