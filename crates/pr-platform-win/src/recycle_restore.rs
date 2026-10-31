//! Port of `PhotoReview.Platform.Windows/Shell/RecycleBinShell.cs` restore orchestration.
//!
//! Candidate selection and proof polling are independent from COM so tests can use fake sessions and a fake file
//! system. The Shell session is opened on a short-lived STA worker, keeping blocking enumeration and restore off the
//! caller (normally the UI) thread.

use std::io;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use pr_core::abstractions::{Log, NullLog};
use pr_core::time::DateTimeUtc;

use crate::recycle_candidate::{self, RecycleCandidate};

/// One shell session. Dropping it releases the virtual Recycle Bin folder and enumerator references.
pub trait RecycleRestoreSession {
    /// Enumerates owned entry wrappers. On error, already-created wrappers are dropped by the implementation.
    fn entries(&mut self) -> io::Result<Vec<Box<dyn RecycleRestoreEntry>>>;
}

/// A single owned shell item. Its wrapper must release its COM reference in `Drop`.
pub trait RecycleRestoreEntry {
    /// Reads only the metadata used by the W6-02d selector.
    fn candidate(&self) -> Result<Option<RecycleCandidate>, RecycleRestoreItemError>;

    /// Opens the item's context menu and invokes one restore route.
    fn invoke_restore(&self) -> io::Result<()>;
}

/// Failure category for reading one Shell item, matching C# `RecycleItemFailure.IsPerItem`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecycleRestoreItemFailureKind {
    Com,
    InvalidCast,
    Format,
    Other,
}

/// Metadata inspection failure. The first three kinds skip only this item; `Other` aborts the restore.
#[derive(Debug)]
pub struct RecycleRestoreItemError {
    pub kind: RecycleRestoreItemFailureKind,
    pub source: io::Error,
}

impl RecycleRestoreItemError {
    fn is_per_item(&self) -> bool {
        matches!(
            self.kind,
            RecycleRestoreItemFailureKind::Com
                | RecycleRestoreItemFailureKind::InvalidCast
                | RecycleRestoreItemFailureKind::Format
        )
    }
}

type SessionFactory = dyn Fn() -> io::Result<Option<Box<dyn RecycleRestoreSession>>> + Send + Sync;
type RestoreProof = dyn Fn(&Path, u64, DateTimeUtc) -> bool + Send + Sync;
type Pause = dyn Fn() + Send + Sync;

/// Upper bound the caller waits for the Shell worker. The C# `ShellStaRunner.Run` joins without limit; a hidden Shell
/// dialog (vanished original folder, name conflict) would then hang the caller forever. Here the caller gives up with
/// an error instead (see `restore_on_worker`). Generous so that a user who answers a real dialog is not cut off.
pub const SHELL_RESTORE_TIMEOUT: Duration = Duration::from_secs(120);

/// Shell restore backend used by `WindowsRecycleBin`.
pub struct WindowsRecycleBinRestoreBackend {
    open_session: Arc<SessionFactory>,
    prove_restored: Arc<RestoreProof>,
    pause: Arc<Pause>,
    attempts: usize,
    log: Arc<dyn Log>,
    restore_timeout: Duration,
}

impl std::fmt::Debug for WindowsRecycleBinRestoreBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("WindowsRecycleBinRestoreBackend")
    }
}

impl Default for WindowsRecycleBinRestoreBackend {
    fn default() -> Self {
        Self::new(Arc::new(NullLog))
    }
}

impl WindowsRecycleBinRestoreBackend {
    /// Creates the production backend. Native COM is unavailable on non-Windows targets.
    pub fn new(log: Arc<dyn Log>) -> Self {
        Self::with_adapters(
            open_native_session,
            native_restore_proof,
            || std::thread::sleep(std::time::Duration::from_millis(60)),
            30,
            log,
        )
    }

    /// Creates a backend with fakeable session, proof, and pause seams.
    ///
    /// `attempts` is the count of checks before the final check. Production uses the C# limit of 30 checks, 60 ms
    /// apart; tests should use a short count and a no-op pause.
    pub fn with_adapters(
        open_session: impl Fn() -> io::Result<Option<Box<dyn RecycleRestoreSession>>> + Send + Sync + 'static,
        prove_restored: impl Fn(&Path, u64, DateTimeUtc) -> bool + Send + Sync + 'static,
        pause: impl Fn() + Send + Sync + 'static,
        attempts: usize,
        log: Arc<dyn Log>,
    ) -> Self {
        Self {
            open_session: Arc::new(open_session),
            prove_restored: Arc::new(prove_restored),
            pause: Arc::new(pause),
            attempts,
            log,
            restore_timeout: SHELL_RESTORE_TIMEOUT,
        }
    }

    /// Replaces the bound on waiting for the Shell worker (tests use a few milliseconds).
    #[must_use]
    pub fn with_restore_timeout(mut self, timeout: Duration) -> Self {
        self.restore_timeout = timeout;
        self
    }

    fn restore_on_worker(
        &self,
        original_path: &Path,
        expected_size: u64,
        expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool> {
        let Ok(expected_size_i64) = i64::try_from(expected_size) else {
            return Ok(false);
        };
        let open_session = Arc::clone(&self.open_session);
        let prove_restored = Arc::clone(&self.prove_restored);
        let pause = Arc::clone(&self.pause);
        let log = Arc::clone(&self.log);
        let path = original_path.to_owned();
        let attempts = self.attempts;
        let (sender, receiver) = mpsc::channel::<io::Result<bool>>();
        // The worker is intentionally detached: a Shell call stuck in a dialog cannot be cancelled, and the caller must
        // not wait for it forever. If it finishes later its result is dropped (`send` fails on the closed channel).
        std::thread::Builder::new()
            .name("PhotoReviewRs-RestoreShell".to_owned())
            .spawn(move || {
                let result = (move || -> io::Result<bool> {
                    let Some(mut session) = (open_session)()? else {
                        return Ok(false);
                    };
                    let entries = session.entries()?;
                    let mut matches = Vec::new();
                    for entry in entries {
                        match entry.candidate() {
                            Ok(Some(candidate))
                                if recycle_candidate::is_match(
                                    &candidate,
                                    &path,
                                    expected_size_i64,
                                    expected_last_write_utc,
                                ) =>
                            {
                                matches.push(entry)
                            }
                            Ok(_) => {}
                            Err(error) if error.is_per_item() => {
                                log.error("Recycle Bin item inspection failed", Some(&error.source));
                            }
                            Err(error) => return Err(error.source),
                        }
                    }
                    if matches.len() != 1 {
                        return Ok(false);
                    }
                    matches[0].invoke_restore()?;
                    Ok(wait_for_restore_with(
                        attempts,
                        || (prove_restored)(&path, expected_size, expected_last_write_utc),
                        || (pause)(),
                    ))
                })();
                let _ = sender.send(result);
            })?;
        match receiver.recv_timeout(self.restore_timeout) {
            Ok(result) => result,
            // Never `Ok(true)`: nothing is known about the outcome. The undo flow then proves by the file system, and an
            // item the Shell restores late shows up as "target exists" on a retry.
            Err(mpsc::RecvTimeoutError::Timeout) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Recycle Bin restore has not completed (the Shell may be waiting on a hidden dialog); the file may still be restored later",
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::other("Recycle Bin restore worker panicked")),
        }
    }
}

impl crate::recycle_bin::RecycleBinRestoreBackend for WindowsRecycleBinRestoreBackend {
    fn try_restore(
        &self,
        original_path: &Path,
        expected_size: u64,
        expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool> {
        self.restore_on_worker(original_path, expected_size, expected_last_write_utc)
    }
}

/// C# `WaitForRestore`: `attempts` checks, one pause after each miss, then a final check.
pub fn wait_for_restore_with(attempts: usize, mut proof: impl FnMut() -> bool, mut pause: impl FnMut()) -> bool {
    for _ in 0..attempts {
        if proof() {
            return true;
        }
        pause();
    }
    proof()
}

fn native_restore_proof(path: &Path, expected_size: u64, expected_write: DateTimeUtc) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    let Some(actual_write) = metadata.modified().ok().and_then(DateTimeUtc::from_system_time) else {
        return false;
    };
    is_expected_restore_file(metadata.len(), actual_write, expected_size, expected_write)
}

fn is_expected_restore_file(
    actual_size: u64,
    actual_write: DateTimeUtc,
    expected_size: u64,
    expected_write: DateTimeUtc,
) -> bool {
    actual_size == expected_size && actual_write == expected_write
}

#[cfg(windows)]
fn open_native_session() -> io::Result<Option<Box<dyn RecycleRestoreSession>>> {
    crate::recycle_bin_shell::open_session()
}

#[cfg(not(windows))]
fn open_native_session() -> io::Result<Option<Box<dyn RecycleRestoreSession>>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows Shell Recycle Bin restore is available only on Windows",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recycle_bin::RecycleBinRestoreBackend;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn stamp() -> DateTimeUtc {
        DateTimeUtc::from_ticks(638_000_000_000_000_000).expect("test timestamp is in range")
    }

    fn candidate(name: &str) -> RecycleCandidate {
        RecycleCandidate::new(Some(r"C:\photos".to_owned()), Some(name.to_owned()), 123, stamp(), None)
    }

    struct FakeEntry {
        metadata: Result<Option<RecycleCandidate>, RecycleRestoreItemError>,
        invokes: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
        invoke_error: bool,
    }

    impl RecycleRestoreEntry for FakeEntry {
        fn candidate(&self) -> Result<Option<RecycleCandidate>, RecycleRestoreItemError> {
            match &self.metadata {
                Ok(value) => Ok(value.clone()),
                Err(error) => Err(RecycleRestoreItemError {
                    kind: error.kind,
                    source: io::Error::new(error.source.kind(), error.source.to_string()),
                }),
            }
        }

        fn invoke_restore(&self) -> io::Result<()> {
            self.invokes.fetch_add(1, Ordering::SeqCst);
            if self.invoke_error {
                Err(io::Error::other("fake invocation failed"))
            } else {
                Ok(())
            }
        }
    }

    impl Drop for FakeEntry {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[derive(Clone)]
    struct FakeEntrySpec {
        metadata: Result<Option<RecycleCandidate>, (RecycleRestoreItemFailureKind, io::ErrorKind, String)>,
        invokes: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
        invoke_error: bool,
    }

    impl FakeEntrySpec {
        fn from_metadata(
            metadata: Result<Option<RecycleCandidate>, RecycleRestoreItemError>,
            invokes: Arc<AtomicUsize>,
            drops: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                metadata: metadata.map_err(|error| (error.kind, error.source.kind(), error.source.to_string())),
                invokes,
                drops,
                invoke_error: false,
            }
        }

        fn build(&self) -> Box<dyn RecycleRestoreEntry> {
            let metadata = self
                .metadata
                .clone()
                .map_err(|(kind, error_kind, message)| RecycleRestoreItemError {
                    kind,
                    source: io::Error::new(error_kind, message),
                });
            Box::new(FakeEntry {
                metadata,
                invokes: Arc::clone(&self.invokes),
                drops: Arc::clone(&self.drops),
                invoke_error: self.invoke_error,
            })
        }
    }

    struct FakeSession {
        entries: Option<Vec<Box<dyn RecycleRestoreEntry>>>,
        drops: Arc<AtomicUsize>,
    }

    impl RecycleRestoreSession for FakeSession {
        fn entries(&mut self) -> io::Result<Vec<Box<dyn RecycleRestoreEntry>>> {
            self.entries
                .take()
                .ok_or_else(|| io::Error::other("entries already consumed"))
        }
    }

    impl Drop for FakeSession {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn backend(
        entries: Vec<FakeEntrySpec>,
        proof: bool,
        opened: Arc<AtomicUsize>,
        session_drops: Arc<AtomicUsize>,
    ) -> WindowsRecycleBinRestoreBackend {
        WindowsRecycleBinRestoreBackend::with_adapters(
            move || {
                opened.fetch_add(1, Ordering::SeqCst);
                Ok(Some(Box::new(FakeSession {
                    entries: Some(entries.iter().map(FakeEntrySpec::build).collect()),
                    drops: Arc::clone(&session_drops),
                })))
            },
            move |_, _, _| proof,
            || {},
            2,
            Arc::new(NullLog),
        )
    }

    fn restore(backend: &WindowsRecycleBinRestoreBackend) -> io::Result<bool> {
        backend.try_restore(Path::new(r"C:\photos\photo.jpg"), 123, stamp())
    }

    #[test]
    fn wait_for_restore_uses_bounded_checks_and_final_check() {
        let mut checks = 0;
        let mut pauses = 0;
        let restored = wait_for_restore_with(
            3,
            || {
                checks += 1;
                checks == 4
            },
            || pauses += 1,
        );
        assert!(restored);
        assert_eq!(checks, 4);
        assert_eq!(pauses, 3);
    }

    #[test]
    fn wait_for_restore_returns_false_after_final_bounded_check() {
        let mut checks = 0;
        let mut pauses = 0;
        assert!(!wait_for_restore_with(
            2,
            || {
                checks += 1;
                false
            },
            || pauses += 1
        ));
        assert_eq!(checks, 3);
        assert_eq!(pauses, 2);
    }

    #[test]
    fn wait_for_restore_does_not_pause_after_success() {
        let mut pauses = 0;
        assert!(wait_for_restore_with(30, || true, || pauses += 1));
        assert_eq!(pauses, 0);
    }

    #[test]
    fn restore_proof_requires_both_exact_size_and_utc_write_time() {
        assert!(is_expected_restore_file(123, stamp(), 123, stamp()));
        assert!(!is_expected_restore_file(122, stamp(), 123, stamp()));
        assert!(!is_expected_restore_file(
            123,
            DateTimeUtc::from_ticks(stamp().ticks() + 1).unwrap(),
            123,
            stamp()
        ));
    }

    #[test]
    fn one_matching_candidate_invokes_once_and_disposes_all_wrappers() {
        let invokes = Arc::new(AtomicUsize::new(0));
        let entry_drops = Arc::new(AtomicUsize::new(0));
        let session_drops = Arc::new(AtomicUsize::new(0));
        let opened = Arc::new(AtomicUsize::new(0));
        let entries = vec![
            FakeEntrySpec::from_metadata(
                Ok(Some(candidate("photo.jpg"))),
                Arc::clone(&invokes),
                Arc::clone(&entry_drops),
            ),
            FakeEntrySpec::from_metadata(
                Ok(Some(candidate("other.jpg"))),
                Arc::clone(&invokes),
                Arc::clone(&entry_drops),
            ),
        ];
        let backend = backend(entries, true, Arc::clone(&opened), Arc::clone(&session_drops));
        assert!(restore(&backend).expect("single candidate restores"));
        assert_eq!(invokes.load(Ordering::SeqCst), 1);
        assert_eq!(opened.load(Ordering::SeqCst), 1);
        assert_eq!(entry_drops.load(Ordering::SeqCst), 2);
        assert_eq!(session_drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn no_candidate_does_not_invoke_and_disposes_session_and_item() {
        let invokes = Arc::new(AtomicUsize::new(0));
        let entry_drops = Arc::new(AtomicUsize::new(0));
        let session_drops = Arc::new(AtomicUsize::new(0));
        let backend = backend(
            vec![FakeEntrySpec::from_metadata(
                Ok(None),
                Arc::clone(&invokes),
                Arc::clone(&entry_drops),
            )],
            true,
            Arc::new(AtomicUsize::new(0)),
            Arc::clone(&session_drops),
        );
        assert!(!restore(&backend).expect("no match is a normal refusal"));
        assert_eq!(invokes.load(Ordering::SeqCst), 0);
        assert_eq!(entry_drops.load(Ordering::SeqCst), 1);
        assert_eq!(session_drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn ambiguous_candidates_do_not_invoke_and_dispose_both_items() {
        let invokes = Arc::new(AtomicUsize::new(0));
        let entry_drops = Arc::new(AtomicUsize::new(0));
        let backend = backend(
            vec![
                FakeEntrySpec::from_metadata(
                    Ok(Some(candidate("photo.jpg"))),
                    Arc::clone(&invokes),
                    Arc::clone(&entry_drops),
                ),
                FakeEntrySpec::from_metadata(
                    Ok(Some(candidate("photo.jpg"))),
                    Arc::clone(&invokes),
                    Arc::clone(&entry_drops),
                ),
            ],
            true,
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
        );
        assert!(!restore(&backend).expect("ambiguous match is a normal refusal"));
        assert_eq!(invokes.load(Ordering::SeqCst), 0);
        assert_eq!(entry_drops.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn per_item_metadata_failure_is_skipped_before_unique_restore() {
        let invokes = Arc::new(AtomicUsize::new(0));
        let entry_drops = Arc::new(AtomicUsize::new(0));
        let skipped = FakeEntrySpec::from_metadata(
            Err(RecycleRestoreItemError {
                kind: RecycleRestoreItemFailureKind::Com,
                source: io::Error::other("one item has unavailable metadata"),
            }),
            Arc::clone(&invokes),
            Arc::clone(&entry_drops),
        );
        let matched = FakeEntrySpec::from_metadata(
            Ok(Some(candidate("photo.jpg"))),
            Arc::clone(&invokes),
            Arc::clone(&entry_drops),
        );
        let backend = backend(
            vec![skipped, matched],
            true,
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
        );
        assert!(restore(&backend).expect("one readable item uniquely matches"));
        assert_eq!(invokes.load(Ordering::SeqCst), 1);
        assert_eq!(entry_drops.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn unrelated_metadata_failure_propagates_and_disposes_everything() {
        let invokes = Arc::new(AtomicUsize::new(0));
        let entry_drops = Arc::new(AtomicUsize::new(0));
        let session_drops = Arc::new(AtomicUsize::new(0));
        let backend = backend(
            vec![FakeEntrySpec::from_metadata(
                Err(RecycleRestoreItemError {
                    kind: RecycleRestoreItemFailureKind::Other,
                    source: io::Error::other("unexpected metadata failure"),
                }),
                Arc::clone(&invokes),
                Arc::clone(&entry_drops),
            )],
            true,
            Arc::new(AtomicUsize::new(0)),
            Arc::clone(&session_drops),
        );
        let error = restore(&backend).expect_err("unrelated metadata failure propagates");
        assert_eq!(error.to_string(), "unexpected metadata failure");
        assert_eq!(invokes.load(Ordering::SeqCst), 0);
        assert_eq!(entry_drops.load(Ordering::SeqCst), 1);
        assert_eq!(session_drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failed_invocation_propagates_without_retrying_another_route() {
        let invokes = Arc::new(AtomicUsize::new(0));
        let entry_drops = Arc::new(AtomicUsize::new(0));
        let session_drops = Arc::new(AtomicUsize::new(0));
        let entry = FakeEntrySpec {
            metadata: Ok(Some(candidate("photo.jpg"))),
            invokes: Arc::clone(&invokes),
            drops: Arc::clone(&entry_drops),
            invoke_error: true,
        };
        let backend = backend(
            vec![entry],
            true,
            Arc::new(AtomicUsize::new(0)),
            Arc::clone(&session_drops),
        );
        assert_eq!(
            restore(&backend).expect_err("verb failure propagates").to_string(),
            "fake invocation failed"
        );
        assert_eq!(invokes.load(Ordering::SeqCst), 1);
        assert_eq!(entry_drops.load(Ordering::SeqCst), 1);
        assert_eq!(session_drops.load(Ordering::SeqCst), 1);
    }
}
