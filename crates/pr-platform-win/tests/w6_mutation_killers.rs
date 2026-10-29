//! Killers for the survivors of the Wave 6 platform mutation runs (2026-10-29, see
//! `docs/reviews/W6-PLATFORM-EVIDENCE-2026-10-29.md`). Fakes and pure functions only; nothing here touches the
//! Recycle Bin, a pipe or the shell. Each test names the mutant it kills.

#[allow(dead_code)]
#[path = "../src/explorer_window_selector.rs"]
mod explorer_window_selector;
#[allow(dead_code)]
#[path = "../src/forward_pipe.rs"]
mod forward_pipe;
#[path = "../src/instance_keys.rs"]
mod instance_keys;
#[allow(dead_code)]
#[path = "../src/recycle_candidate.rs"]
mod recycle_candidate;

use std::error::Error;
use std::sync::{Arc, Mutex};

use explorer_window_selector::{
    ExplorerSelectionWindow, LocationReadError, SelectionReadError, select, try_canonicalize_location,
};
use pr_core::abstractions::Log;
use pr_core::catalog::explorer_types::{ExplorerOrderStatus, ExplorerViewSnapshot};
use pr_core::catalog::snapshot_validator::canonicalize_folder;
use pr_core::sync::CancelToken;

struct NullLog;
impl Log for NullLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, _text: &str) {}
    fn warn(&self, _text: &str) {}
    fn error(&self, _text: &str, _error: Option<&(dyn Error + 'static)>) {}
}

struct OtherFolderWindow {
    released: Arc<Mutex<usize>>,
}
impl ExplorerSelectionWindow for OtherFolderWindow {
    fn location_url(&self) -> Result<Option<String>, LocationReadError> {
        Ok(Some("file:///D:/elsewhere".to_owned()))
    }
    fn read(&self, _folder: &str, _cancellation: &CancelToken) -> Result<ExplorerViewSnapshot, SelectionReadError> {
        unreachable!("a window on another folder is never read")
    }
    fn release(&self) {
        *self.released.lock().unwrap() += 1;
    }
}

fn no_match_reason(window_count: usize) -> String {
    let released = Arc::new(Mutex::new(0usize));
    let windows: Vec<Result<OtherFolderWindow, Box<dyn Error + Send + Sync>>> = (0..window_count)
        .map(|_| {
            Ok(OtherFolderWindow {
                released: released.clone(),
            })
        })
        .collect();
    let result = select(r"C:\photos", windows, &NullLog, &CancelToken::none());
    assert_eq!(result.status, ExplorerOrderStatus::NoMatchingWindow);
    assert_eq!(*released.lock().unwrap(), window_count);
    result.reason.expect("reason")
}

// Mutant: explorer_window_selector.rs `replace += with *= in select` (inspected counter stays 0).
#[test]
fn no_match_reason_counts_each_inspected_window() {
    assert_eq!(no_match_reason(1), "no-matching-window|1");
    assert_eq!(no_match_reason(3), "no-matching-window|3");
}

// Mutant: explorer_window_selector.rs `replace <impl Display for SelectionReadError>::fmt with Ok(Default::default())`.
// C# twin: the failure reason text `type|0xHRESULT` is built from the same two fields.
#[test]
fn selection_read_error_display_names_type_and_hresult() {
    let error = SelectionReadError {
        type_name: "COMException".to_owned(),
        hresult: 0x8000_4005_u32 as i32,
        canceled: false,
    };
    assert_eq!(error.to_string(), "COMException (HRESULT 0x80004005)");
}

// Mutant: explorer_window_selector.rs:196 `replace && with || in try_canonicalize_location` (a rooted path without a
// drive letter lost its leading separator and became relative).
#[test]
fn rooted_file_url_without_a_drive_keeps_its_leading_separator() {
    let expected = canonicalize_folder(r"\foo\bar").ok();
    assert!(expected.is_some());
    assert_eq!(try_canonicalize_location("file:///foo/bar"), expected);
}

// Mutants: forward_pipe.rs:21 `replace current_pipe_name -> String with String::new() / "xyzzy".into()`.
#[test]
fn current_pipe_name_is_the_recipe_over_the_cached_process_identity() {
    let name = forward_pipe::current_pipe_name("mutex-a", "PhotoReviewRs");
    assert_eq!(
        name,
        forward_pipe::name_for_mutex(
            "mutex-a",
            "PhotoReviewRs",
            instance_keys::current_user_sid(),
            instance_keys::current_session_id()
        )
    );
    assert!(name.starts_with("PhotoReviewRs.Forward."));
    assert_eq!(name.len(), "PhotoReviewRs.Forward.".len() + 64);
}

// Mutants: forward_pipe.rs:52 `replace || with &&` (twice), `delete !`, `replace == with !=` in
// `PipeSecurityDescriptor::for_sid`: only a canonical `S-<digits and dashes>` string may reach the SDDL parser.
// C# twin: InstanceForwardPipe.CreateSecurity takes a SecurityIdentifier, so a non-SID cannot be expressed.
#[cfg(windows)]
#[test]
fn security_descriptor_rejects_non_canonical_sid_strings() {
    for bad in [
        "",
        "S-",
        "1-5-18",
        "s-1-5-18",
        "X-1-5-18",
        "S-1-5-x",
        "S-1-5-18)(A;;GA;;;WD",
        "S-1-5-18 ",
        "S- 1",
    ] {
        assert!(forward_pipe::PipeSecurityDescriptor::for_sid(bad).is_err(), "{bad:?}");
    }
    for good in ["S-1-5-18", "S-1-5-21-1-2-3-1001"] {
        assert!(forward_pipe::PipeSecurityDescriptor::for_sid(good).is_ok(), "{good:?}");
    }
}

// Mutants: forward_pipe.rs:73 `replace as_ptr with Default::default()` (null descriptor would mean "no security").
#[cfg(windows)]
#[test]
fn security_descriptor_pointer_is_non_null() {
    let descriptor = forward_pipe::PipeSecurityDescriptor::for_sid("S-1-5-18").expect("descriptor");
    assert!(!descriptor.as_ptr().0.is_null());
}

// Mutant: forward_pipe.rs:90 `replace == with != in current_security_descriptor` ("nosid" must be the refused case).
#[cfg(windows)]
#[test]
fn current_security_descriptor_is_built_for_a_real_user_sid() {
    assert_ne!(instance_keys::current_user_sid(), "nosid");
    assert!(forward_pipe::current_security_descriptor().is_ok());
}

// ---- recycle eligibility (policy only: read-only volume queries, no Shell, no Recycle Bin) ----

mod eligibility {
    use pr_platform_win::recycle_bin_settings::DriveType;
    use pr_platform_win::recycle_eligibility::{
        can_recycle, can_recycle_with, is_permanent_delete_allowed, is_permanent_delete_allowed_with,
    };

    fn system_root_file() -> String {
        // The Windows directory is always on a fixed local volume.
        format!(
            r"{}\system32\w6-killer.jpg",
            std::env::var("SystemRoot").expect("SystemRoot")
        )
    }

    // Mutants: recycle_eligibility.rs:12 `replace can_recycle -> bool with true / false`.
    // Only read-only GetVolumePathName/GetDriveType queries run; nothing is sent anywhere.
    #[test]
    fn can_recycle_uses_the_real_volume_queries() {
        assert!(can_recycle(&system_root_file()), "fixed local volume is recyclable");
        assert!(
            !can_recycle(r"\\server\share\a.jpg"),
            "a real UNC path is never recycled"
        );
        assert!(!can_recycle(""), "a blank path is refused");
    }

    // Mutants: recycle_eligibility.rs:38 `replace is_permanent_delete_allowed -> bool with true / false`.
    #[test]
    fn is_permanent_delete_allowed_uses_the_real_volume_queries() {
        assert!(
            !is_permanent_delete_allowed(&system_root_file()),
            "a fixed volume has a bin: permanent delete refused"
        );
        assert!(
            is_permanent_delete_allowed(r"\\server\share\a.jpg"),
            "a real UNC path has no bin: permanent delete allowed"
        );
        assert!(!is_permanent_delete_allowed(""), "a blank path fails closed");
    }

    fn fixed(_: &str) -> Option<DriveType> {
        Some(DriveType::Fixed)
    }
    fn c_root(_: &str) -> Option<String> {
        Some(r"C:\".to_owned())
    }

    // Mutant: recycle_eligibility.rs:79 `replace || with && in normalized_full_path` (blank-only or NUL-only
    // input must each be refused, not just a path that is both).
    #[test]
    fn blank_or_nul_paths_are_refused_even_when_the_volume_looks_fixed() {
        assert!(!can_recycle_with("   ", fixed, c_root));
        let with_nul = format!("C:{}a{}b.jpg", '\\', '\0');
        assert!(!can_recycle_with(&with_nul, fixed, c_root));
        assert!(!is_permanent_delete_allowed_with(
            "   ",
            |_| Some(DriveType::Removable),
            c_root
        ));
        assert!(!is_permanent_delete_allowed_with(
            &with_nul,
            |_| Some(DriveType::Removable),
            c_root
        ));
    }

    // Mutants: recycle_eligibility.rs:86 `delete match arm Component::CurDir` and :87 `... Component::ParentDir`.
    // C# twin: Path.GetFullPath resolves `.` and `..` before the volume is looked up.
    #[test]
    fn dot_and_dotdot_segments_are_resolved_before_the_volume_lookup() {
        let mut seen = Vec::new();
        assert!(can_recycle_with(r"C:\a\..\b\.\c.jpg", fixed, |full| {
            seen.push(full.to_owned());
            Some(r"C:\".to_owned())
        }));
        assert_eq!(seen, vec![r"C:\b\c.jpg".to_owned()]);

        let mut above_root = Vec::new();
        let _ = can_recycle_with(r"C:\..\x.jpg", fixed, |full| {
            above_root.push(full.to_owned());
            Some(r"C:\".to_owned())
        });
        assert_eq!(above_root, vec![r"C:\x.jpg".to_owned()]);
    }
}

// ---- recycle candidate matching (metadata policy; the Shell is never involved) ----

mod candidate {
    use std::path::Path;

    use super::recycle_candidate::{RecycleCandidate, is_match};
    use pr_core::time::DateTimeUtc;

    fn stamp() -> DateTimeUtc {
        DateTimeUtc::parse_json("2026-09-20T01:02:03Z")
            .expect("valid timestamp")
            .0
    }

    fn from_folder(folder: &str) -> RecycleCandidate {
        RecycleCandidate::new(Some(folder.to_owned()), Some("a.jpg".to_owned()), 7, stamp(), None)
    }

    // Mutant: recycle_candidate.rs:97 `replace == with != in trim_ending_separator` (drive-root `C:\` must stay
    // `C:\`, whereas `C:` is a different location; a non-drive two-character stem loses its separator).
    // C# twin: Path.TrimEndingDirectorySeparator keeps a root and trims every other trailing separator.
    #[test]
    fn drive_root_keeps_its_separator_and_other_folders_lose_it() {
        let root_file = Path::new(r"C:\a.jpg");
        assert!(is_match(&from_folder(r"C:\"), root_file, 7, stamp()));
        assert!(
            !is_match(&from_folder("C:"), root_file, 7, stamp()),
            "C: is not the drive root"
        );

        let stem_file = Path::new(r"ab\a.jpg");
        assert!(is_match(&from_folder(r"ab\"), stem_file, 7, stamp()));
        assert!(is_match(&from_folder("ab"), stem_file, 7, stamp()));
    }

    // Mutant: recycle_candidate.rs:96 `replace > with >= in trim_ending_separator` (a lone separator must not be
    // trimmed away, otherwise a slash and a backslash both become empty and compare equal).
    #[test]
    fn a_lone_separator_is_not_trimmed_to_nothing() {
        assert!(!is_match(&from_folder("/"), Path::new(r"\a.jpg"), 7, stamp()));
        assert!(is_match(&from_folder(r"\"), Path::new(r"\a.jpg"), 7, stamp()));
    }
}

// ---- recycle capacity decision (settings come from a fake; the registry and the Shell are never read) ----

mod capacity {
    use std::io;

    use pr_core::abstractions::NullLog;
    use pr_platform_win::recycle_bin_capacity::{RecycleBinCapacityGuard, RecycleCapacityVerdict as Verdict};
    use pr_platform_win::recycle_bin_settings::{
        DriveType, RecycleBinPolicy, RecycleBinSettingsSource, RecycleBinVolumeSettings,
    };

    const MIB: i64 = 1_048_576;

    struct Settings {
        policy: RecycleBinPolicy,
        max_capacity_mb: Option<i64>,
        quota: Option<i64>,
    }
    impl RecycleBinSettingsSource for Settings {
        fn read_policy(&self) -> io::Result<RecycleBinPolicy> {
            Ok(self.policy)
        }
        fn get_volume_guid(&self, _path: &str) -> Option<String> {
            Some("{0a089459-c105-4107-99e6-657d2fddd42b}".to_owned())
        }
        fn get_volume_drive_type(&self, _path: &str) -> Option<DriveType> {
            Some(DriveType::Fixed)
        }
        fn read_volume(&self, _volume_guid: &str) -> io::Result<RecycleBinVolumeSettings> {
            Ok(RecycleBinVolumeSettings {
                nuke_on_delete: Some(0),
                max_capacity_mb: self.max_capacity_mb,
            })
        }
        fn get_user_quota_bytes(&self, _path: &str) -> Option<i64> {
            self.quota
        }
    }

    fn verdict(settings: &Settings, size: i64) -> Verdict {
        RecycleBinCapacityGuard::evaluate(r"C:\photos\a.jpg", Some(size), settings, Some(&NullLog)).expect("valid path")
    }

    fn percent(percent: i64) -> RecycleBinPolicy {
        RecycleBinPolicy {
            max_size_percent: Some(percent),
            ..RecycleBinPolicy::default()
        }
    }

    // Mutants: recycle_bin_capacity.rs:29 `replace * with + / /` in MEBIBYTE (the tests used the constant on both
    // sides, so a wrong constant cancelled out).
    #[test]
    fn mebibyte_and_safety_margin_are_literal_binary_megabytes() {
        assert_eq!(RecycleBinCapacityGuard::MEBIBYTE, 1_048_576);
        assert_eq!(RecycleBinCapacityGuard::SAFETY_MARGIN_BYTES, 1_048_576);
        assert_eq!(RecycleBinCapacityGuard::DEFAULT_CAPACITY_DIVISOR, 20);
    }

    // Mutant: recycle_bin_capacity.rs:82 `replace < with <=` (an empty file is valid and fits).
    #[test]
    fn empty_file_fits() {
        let settings = Settings {
            policy: RecycleBinPolicy::default(),
            max_capacity_mb: Some(100),
            quota: Some(100 * 1024 * MIB),
        };
        assert_eq!(verdict(&settings, 0), Verdict::Fits);
        assert_eq!(verdict(&settings, -1), Verdict::Unknown);
    }

    // Mutants: recycle_bin_capacity.rs:87 and :101 `replace > with >= ` on the quota check (a zero quota is unusable)
    // for the percent-policy branch and the no-volume-entry fallback branch.
    #[test]
    fn zero_user_quota_is_unknown_in_both_quota_branches() {
        let with_percent = Settings {
            policy: percent(10),
            max_capacity_mb: Some(100),
            quota: Some(0),
        };
        assert_eq!(verdict(&with_percent, 1), Verdict::Unknown);
        let fallback = Settings {
            policy: RecycleBinPolicy::default(),
            max_capacity_mb: None,
            quota: Some(0),
        };
        assert_eq!(verdict(&fallback, 1), Verdict::Unknown);
    }

    // Mutant: recycle_bin_capacity.rs:90 `replace * with + in evaluate_core` (percent of the user quota:
    // `(quota / 100) * percent`, checked at the exact boundary with literal sizes).
    #[test]
    fn percent_policy_capacity_is_the_percentage_of_the_user_quota() {
        let settings = Settings {
            policy: percent(2),
            max_capacity_mb: None,
            quota: Some(100 * 1024 * MIB),
        };
        assert_eq!(verdict(&settings, 2 * 1024 * MIB - MIB), Verdict::Fits);
        assert_eq!(verdict(&settings, 2 * 1024 * MIB - MIB + 1), Verdict::TooLarge);
    }

    // Mutants: recycle_bin_capacity.rs:92 `replace * with + / /` (volume MB to bytes inside the percent-policy cap).
    #[test]
    fn percent_policy_is_capped_by_the_volume_capacity_in_bytes() {
        let settings = Settings {
            policy: percent(100),
            max_capacity_mb: Some(100),
            quota: Some(100 * 1024 * MIB),
        };
        assert_eq!(verdict(&settings, 99 * MIB), Verdict::Fits);
        assert_eq!(verdict(&settings, 99 * MIB + 1), Verdict::TooLarge);
    }
}

// ---- instance keys: identity reads and lexical path resolution (read-only OS queries) ----

mod keys {
    use super::instance_keys::{canonical_key, current_session_id, current_user_sid};

    // Mutants: instance_keys.rs:22/108/172 (`current_user_sid`, `read_user_sid` -> "" / "xyzzy"), :123 Drop, :131, :153,
    // :159, :160 (token buffer arithmetic): the cached SID must be the real token SID, as `whoami` reports it.
    // C# twin: WindowsIdentity.GetCurrent().User (the SID in every mutex and pipe name).
    #[test]
    fn current_user_sid_is_the_real_token_sid() {
        let output = std::process::Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .expect("whoami runs");
        let text = String::from_utf8_lossy(&output.stdout);
        let expected = text
            .trim()
            .rsplit(',')
            .next()
            .expect("csv column")
            .trim()
            .trim_matches('"')
            .to_owned();
        assert!(expected.starts_with("S-1-"), "{expected}");
        assert_eq!(current_user_sid(), expected);
    }

    // Mutants: instance_keys.rs:29/177/190 (`current_session_id`, `read_session_id` -> 0 / 1).
    #[test]
    fn current_session_id_is_this_process_session() {
        use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
        let mut session = u32::MAX;
        // SAFETY: the process id is our own and `session` is a valid output slot.
        unsafe { ProcessIdToSessionId(std::process::id(), &mut session) }.expect("session of this process");
        assert_eq!(current_session_id(), session);
        // The "-> 1" mutants are only observable in a session other than 1 (for example a remote desktop session);
        // on this machine's session-1 desktop they are equivalent and are reported as such.
        assert_ne!(
            session, 0,
            "services run in session 0; an interactive session is expected"
        );
    }

    // Mutants: instance_keys.rs:84 and :85 `delete match arm Component::CurDir / ParentDir in absolute_lexical_path`.
    // C# twin: Path.GetFullPath resolves `.` and `..` lexically before the key is hashed.
    #[test]
    fn dot_and_dotdot_segments_resolve_lexically_in_the_key() {
        assert_eq!(canonical_key(Some(r"C:\a\..\b\.\c")), canonical_key(Some(r"C:\b\c")));
        assert_eq!(canonical_key(Some(r"C:\b\c")), r"C:\B\C");
        assert_eq!(canonical_key(Some(r"C:\..\x")), r"C:\X");
    }
}

// ---- named-pipe server loop and client outcome classification (scripted fakes; no real pipe is created) ----

mod pipes {
    use std::collections::VecDeque;
    use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
    use std::time::{Duration, Instant};

    use pr_core::abstractions::Log;
    use pr_platform_win::forward_server::{ForwardPipe, ForwardPipeError, ForwardPipeFactory, InstanceForwardServer};

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn valid_request() -> Vec<u8> {
        pr_core::instance::protocol::encode(&[r"C:\x.jpg".to_owned()]).expect("encodable request")
    }

    #[derive(Clone)]
    enum Accept {
        Ok,
        Fail,
        CanceledWhileNotCanceled,
        BlockUntilFactoryCancel,
    }

    struct Script {
        accept: Accept,
        reads: VecDeque<Result<Vec<u8>, ForwardPipeError>>,
    }

    #[derive(Default)]
    struct State {
        scripts: VecDeque<Script>,
        canceled: bool,
        create_failures: usize,
        create_count: usize,
        accepting: usize,
        retry_delays: Vec<Duration>,
        replies: Vec<Vec<u8>>,
        logs: Vec<String>,
    }

    #[derive(Default)]
    struct Factory {
        state: Mutex<State>,
        changed: Condvar,
    }

    impl Factory {
        fn with(scripts: Vec<Script>, create_failures: usize) -> Arc<Self> {
            let factory = Arc::new(Self::default());
            {
                let mut state = lock(&factory.state);
                state.scripts = scripts.into();
                state.create_failures = create_failures;
            }
            factory
        }

        fn wait_until(&self, what: &str, done: impl Fn(&State) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut state = lock(&self.state);
            while !done(&state) {
                let remaining = deadline.saturating_duration_since(Instant::now());
                assert!(!remaining.is_zero(), "timed out waiting for {what}");
                state = self
                    .changed
                    .wait_timeout(state, remaining)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        }
    }

    impl ForwardPipeFactory for Factory {
        fn create(self: Arc<Self>, _pipe_name: &str) -> Result<Box<dyn ForwardPipe>, ForwardPipeError> {
            let mut state = lock(&self.state);
            state.create_count += 1;
            self.changed.notify_all();
            if state.canceled {
                return Err(ForwardPipeError::Canceled);
            }
            if state.create_failures != 0 {
                state.create_failures -= 1;
                return Err(ForwardPipeError::Failed("scripted create failure".to_owned()));
            }
            // Out of scripts: park until the server is disposed, like an idle listener.
            while state.scripts.is_empty() && !state.canceled {
                state = self.changed.wait(state).unwrap_or_else(PoisonError::into_inner);
            }
            if state.canceled {
                return Err(ForwardPipeError::Canceled);
            }
            let script = state.scripts.pop_front().expect("a script is queued");
            drop(state);
            Ok(Box::new(Pipe { factory: self, script }))
        }

        fn cancel(&self) {
            lock(&self.state).canceled = true;
            self.changed.notify_all();
        }

        fn is_canceled(&self) -> bool {
            lock(&self.state).canceled
        }

        fn wait_retry(&self, delay: Duration) -> bool {
            let mut state = lock(&self.state);
            state.retry_delays.push(delay);
            self.changed.notify_all();
            !state.canceled
        }
    }

    struct Pipe {
        factory: Arc<Factory>,
        script: Script,
    }

    impl ForwardPipe for Pipe {
        fn accept(&mut self) -> Result<(), ForwardPipeError> {
            let mut state = lock(&self.factory.state);
            state.accepting += 1;
            self.factory.changed.notify_all();
            match self.script.accept {
                Accept::Ok => Ok(()),
                Accept::Fail => Err(ForwardPipeError::Failed("scripted accept failure".to_owned())),
                Accept::CanceledWhileNotCanceled => Err(ForwardPipeError::Canceled),
                Accept::BlockUntilFactoryCancel => {
                    while !state.canceled {
                        state = self.factory.changed.wait(state).unwrap_or_else(PoisonError::into_inner);
                    }
                    Err(ForwardPipeError::Canceled)
                }
            }
        }

        fn read(&mut self, buffer: &mut [u8], _deadline: Instant) -> Result<usize, ForwardPipeError> {
            match self.script.reads.pop_front() {
                Some(Ok(bytes)) => {
                    buffer[..bytes.len()].copy_from_slice(&bytes);
                    Ok(bytes.len())
                }
                Some(Err(error)) => Err(error),
                None => Ok(0),
            }
        }

        fn write_all(&mut self, bytes: &[u8], _deadline: Instant) -> Result<(), ForwardPipeError> {
            lock(&self.factory.state).replies.push(bytes.to_vec());
            self.factory.changed.notify_all();
            Ok(())
        }
    }

    struct StateLog(Arc<Factory>);
    impl Log for StateLog {
        fn enabled(&self) -> bool {
            true
        }
        fn info(&self, text: &str) {
            lock(&self.0.state).logs.push(format!("I:{text}"));
        }
        fn warn(&self, text: &str) {
            lock(&self.0.state).logs.push(format!("W:{text}"));
        }
        fn error(&self, text: &str, _error: Option<&(dyn std::error::Error + 'static)>) {
            lock(&self.0.state).logs.push(format!("E:{text}"));
        }
    }

    fn server(factory: &Arc<Factory>) -> InstanceForwardServer {
        let server = InstanceForwardServer::with_factory(
            "w6-killer-pipe",
            |_| {},
            Some(Arc::new(StateLog(factory.clone()))),
            |_| true,
            None,
            factory.clone(),
        );
        server.start().expect("start the scripted listener");
        server
    }

    fn good_pipe() -> Script {
        Script {
            accept: Accept::Ok,
            reads: VecDeque::from([Ok(valid_request())]),
        }
    }

    // Mutant: forward_server.rs:210 `replace << with >> in retry_delay` (exponential backoff 500 ms, 1 s, 2 s).
    // C# twin: InstanceForwardServer backs off between listener failures with a doubling, capped delay.
    #[test]
    fn creation_retry_delay_doubles_with_each_consecutive_failure() {
        let factory = Factory::with(vec![good_pipe()], 3);
        let server = server(&factory);
        factory.wait_until("the reply", |state| state.replies.len() == 1);
        server.dispose();
        let delays = lock(&factory.state).retry_delays.clone();
        assert_eq!(
            delays,
            vec![
                Duration::from_millis(500),
                Duration::from_millis(1000),
                Duration::from_millis(2000)
            ]
        );
    }

    // Mutant: forward_server.rs:238 `replace match guard inner.factory.is_canceled() with true in run_server`
    // (a canceled accept that the factory did not cancel is a failure to retry, not the end of the listener).
    #[test]
    fn accept_canceled_without_a_dispose_is_retried_on_a_new_instance() {
        let factory = Factory::with(
            vec![
                Script {
                    accept: Accept::CanceledWhileNotCanceled,
                    reads: VecDeque::new(),
                },
                good_pipe(),
            ],
            0,
        );
        let server = server(&factory);
        factory.wait_until("the reply from the second instance", |state| state.replies.len() == 1);
        server.dispose();
        assert_eq!(lock(&factory.state).replies, vec![b"OK\n".to_vec()]);
        assert_eq!(lock(&factory.state).retry_delays.len(), 1);
    }

    // Mutant: forward_server.rs:238 `... with false in run_server` (an accept canceled by dispose ends the loop
    // quietly: no failure is logged and no retry delay is requested).
    #[test]
    fn accept_canceled_by_dispose_is_not_a_listener_failure() {
        let factory = Factory::with(
            vec![Script {
                accept: Accept::BlockUntilFactoryCancel,
                reads: VecDeque::new(),
            }],
            0,
        );
        let server = server(&factory);
        factory.wait_until("the listener to wait in accept", |state| state.accepting == 1);
        server.dispose();
        let state = lock(&factory.state);
        assert!(state.retry_delays.is_empty(), "{:?}", state.retry_delays);
        assert!(
            state
                .logs
                .iter()
                .all(|line| !line.starts_with("W:") && !line.starts_with("E:")),
            "{:?}",
            state.logs
        );
        assert_eq!(state.create_count, 1);
    }

    // Mutant: forward_server.rs:243 `delete ! in run_server` (after an accept failure the loop continues while the
    // retry wait reports "not canceled").
    #[test]
    fn accept_failure_is_retried_with_a_fresh_instance() {
        let factory = Factory::with(
            vec![
                Script {
                    accept: Accept::Fail,
                    reads: VecDeque::new(),
                },
                good_pipe(),
            ],
            0,
        );
        let server = server(&factory);
        factory.wait_until("the reply from the second instance", |state| state.replies.len() == 1);
        server.dispose();
        assert_eq!(lock(&factory.state).replies, vec![b"OK\n".to_vec()]);
    }

    // Mutant: forward_server.rs:259 `replace && with || in handle_request` (a complete request must not trigger
    // another read: the second scripted read would fail and the reply would be lost).
    #[test]
    fn complete_request_is_answered_without_a_second_read() {
        let factory = Factory::with(
            vec![Script {
                accept: Accept::Ok,
                reads: VecDeque::from([
                    Ok(valid_request()),
                    Err(ForwardPipeError::Failed("must not be read".to_owned())),
                ]),
            }],
            0,
        );
        let server = server(&factory);
        factory.wait_until("the reply", |state| !state.replies.is_empty());
        server.dispose();
        assert_eq!(lock(&factory.state).replies, vec![b"OK\n".to_vec()]);
    }
}

mod client {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use pr_core::instance::forwarding::{ForwardError, ForwardOutcome};
    use pr_core::sync::CancelToken;
    use pr_platform_win::forward_client::{
        ForegroundPermissionError, ForwardClientTransport, ForwardPipeClient, ForwardTransportError,
    };

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum CancelDuring {
        Connect,
        Write,
        Read,
    }

    struct Transport {
        token: CancelToken,
        cancel_during: CancelDuring,
        calls: Mutex<Vec<&'static str>>,
    }

    impl ForwardClientTransport for Transport {
        fn connect(&self, _name: &str, _deadline: Instant, _cancel: &CancelToken) -> Result<(), ForwardTransportError> {
            self.calls.lock().unwrap().push("connect");
            if self.cancel_during == CancelDuring::Connect {
                self.token.cancel();
                return Err(ForwardTransportError::NoServer);
            }
            Ok(())
        }
        fn server_is_current_user(&self) -> bool {
            true
        }
        fn allow_server_foreground(&self) -> Result<(), ForegroundPermissionError> {
            Ok(())
        }
        fn write_all(
            &self,
            _bytes: &[u8],
            _deadline: Instant,
            _cancel: &CancelToken,
        ) -> Result<(), ForwardTransportError> {
            self.calls.lock().unwrap().push("write");
            if self.cancel_during == CancelDuring::Write {
                self.token.cancel();
                return Err(ForwardTransportError::Failed);
            }
            Ok(())
        }
        fn read_reply(
            &self,
            _limit: usize,
            _deadline: Instant,
            _cancel: &CancelToken,
        ) -> Result<Vec<u8>, ForwardTransportError> {
            self.calls.lock().unwrap().push("read");
            self.token.cancel();
            Err(ForwardTransportError::TimedOut)
        }
        fn disconnect(&self) {}
    }

    fn outcome(cancel_during: CancelDuring) -> Result<ForwardOutcome, ForwardError> {
        let token = CancelToken::new();
        let client = ForwardPipeClient::with_transport(
            Some("w6-killer-client".to_owned()),
            false,
            Transport {
                token: token.clone(),
                cancel_during,
                calls: Mutex::new(Vec::new()),
            },
        )
        .expect("client");
        client.send_with_timeout(&[r"C:\x.jpg".to_owned()], Duration::from_secs(5), &token)
    }

    // Mutants: forward_client.rs:133 / :144 / :159 `replace match guard cancel.is_canceled() with false in
    // send_locked`: a failure that happens because the caller canceled is reported as Canceled, not as NoInstance
    // (connect, write) or Unknown (read). C# twin: InstanceForwardClient.SendAsync rethrows OperationCanceledException.
    #[test]
    fn caller_cancellation_during_connect_write_or_read_is_reported_as_canceled() {
        for stage in [CancelDuring::Connect, CancelDuring::Write, CancelDuring::Read] {
            assert!(
                matches!(outcome(stage), Err(ForwardError::Canceled)),
                "stage {}",
                stage as u8
            );
        }
    }
}

// ---- recycle settings: pure parsing and the quota guard (fake adapters; no registry or volume is read) ----

mod settings {
    use pr_platform_win::recycle_bin_settings::{DriveType, RecycleBinSettingsSource, WindowsRecycleBinSettingsSource};

    const GOOD: &str = "{0a089459-c105-4107-99e6-657d2fddd42b}";

    fn volume_name(guid: &str) -> String {
        format!(r"\\?\Volume{guid}\")
    }

    // Mutants: recycle_bin_settings.rs:142 / :143 / :144 `replace || with && in extract_guid`: each of the length, the
    // braces, the dash positions and the hexadecimal digits is checked on its own.
    // C# twin: RecycleBinCapacityGuardTests.ExtractGuid_ParsesVolumeName (strict Guid.TryParseExact "B" format).
    #[test]
    fn extract_guid_rejects_each_kind_of_malformation_on_its_own() {
        let ok = WindowsRecycleBinSettingsSource::extract_guid(&volume_name(GOOD));
        assert_eq!(ok.as_deref(), Some(GOOD));
        for (label, bad) in [
            (
                "one hex digit short",
                "{0a089459-c105-4107-99e6-657d2fddd42}".to_owned(),
            ),
            (
                "one hex digit long",
                "{0a089459-c105-4107-99e6-657d2fddd42bc}".to_owned(),
            ),
            (
                "dash replaced at 9",
                "{0a089459+c105-4107-99e6-657d2fddd42b}".to_owned(),
            ),
            (
                "dash replaced at 24",
                "{0a089459-c105-4107-99e6+657d2fddd42b}".to_owned(),
            ),
            ("non-hex digit", "{0a08945g-c105-4107-99e6-657d2fddd42b}".to_owned()),
            (
                "38 chars but dash in a digit slot",
                "{0a089459-c105-4107-99e6-657d-fddd42b}".to_owned(),
            ),
        ] {
            assert_eq!(
                WindowsRecycleBinSettingsSource::extract_guid(&volume_name(&bad)),
                None,
                "{label}: {bad}"
            );
        }
        // Exactly 38 characters with a wrong closing brace position cannot be built from a well-formed name.
        assert_eq!(
            WindowsRecycleBinSettingsSource::extract_guid(r"\\?\Volume{0a089459-c105-4107-99e6-657d2fddd42b)\"),
            None
        );
    }

    fn quota_for(total: Option<u64>) -> Option<i64> {
        let source = WindowsRecycleBinSettingsSource::with_adapters(
            |_, _, _| Ok(None),
            |_| Some(r"C:\".to_owned()),
            |_| None,
            |_| Some(DriveType::Fixed),
            move |_| total,
        );
        source.get_user_quota_bytes(r"C:\a.jpg")
    }

    // Mutants: recycle_bin_settings.rs:209 `replace || with &&`, `replace > with ==`, `replace > with >=` in
    // get_user_quota_bytes: a zero total is unusable, the largest representable total is used, anything above
    // i64::MAX is refused.
    #[test]
    fn user_quota_is_the_volume_total_when_positive_and_representable() {
        assert_eq!(quota_for(Some(0)), None);
        assert_eq!(quota_for(Some(1)), Some(1));
        assert_eq!(quota_for(Some(i64::MAX as u64)), Some(i64::MAX));
        assert_eq!(quota_for(Some(i64::MAX as u64 + 1)), None);
        assert_eq!(quota_for(Some(u64::MAX)), None);
        assert_eq!(quota_for(None), None);
    }
}
