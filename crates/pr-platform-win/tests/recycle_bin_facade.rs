// These source modules are path-loaded so tests can exercise them before lead-owned lib.rs wiring lands.
#[allow(dead_code)]
#[path = "../src/recycle_bin.rs"]
mod recycle_bin;
#[allow(dead_code)]
#[path = "../src/recycle_bin_capacity.rs"]
mod recycle_bin_capacity;
#[allow(dead_code)]
#[path = "../src/recycle_bin_settings.rs"]
mod recycle_bin_settings;
#[allow(dead_code)]
#[path = "../src/recycle_eligibility.rs"]
mod recycle_eligibility;

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pr_core::abstractions::{Log, RecycleBin};
use pr_core::time::DateTimeUtc;
use recycle_bin::{RecycleBinRestoreBackend, ShellRecycleOutcome};
use recycle_bin_settings::{DriveType, RecycleBinPolicy, RecycleBinSettingsSource, RecycleBinVolumeSettings};

const PHOTO: &str = r"C:\photos\safe-test-photo.jpg";
const MIB: u64 = 1024 * 1024;

#[derive(Default)]
struct TestLog {
    warnings: Mutex<Vec<String>>,
}

impl Log for TestLog {
    fn enabled(&self) -> bool {
        true
    }

    fn info(&self, _message: &str) {}

    fn warn(&self, message: &str) {
        self.warnings.lock().unwrap().push(message.to_owned());
    }

    fn error(&self, _message: &str, _error: Option<&(dyn std::error::Error + 'static)>) {}
}

struct TestSettings {
    policy: Mutex<RecycleBinPolicy>,
    volume: Mutex<RecycleBinVolumeSettings>,
    drive_type: Mutex<Option<DriveType>>,
    fail_policy: AtomicBool,
}

impl Default for TestSettings {
    fn default() -> Self {
        Self {
            policy: Mutex::new(RecycleBinPolicy::default()),
            volume: Mutex::new(RecycleBinVolumeSettings {
                nuke_on_delete: Some(0),
                max_capacity_mb: Some(100),
            }),
            drive_type: Mutex::new(Some(DriveType::Fixed)),
            fail_policy: AtomicBool::new(false),
        }
    }
}

impl RecycleBinSettingsSource for TestSettings {
    fn read_policy(&self) -> io::Result<RecycleBinPolicy> {
        if self.fail_policy.load(Ordering::SeqCst) {
            return Err(io::Error::other("settings unavailable"));
        }
        Ok(*self.policy.lock().unwrap())
    }

    fn get_volume_guid(&self, _path: &str) -> Option<String> {
        Some("{0a089459-c105-4107-99e6-657d2fddd42b}".to_owned())
    }

    fn get_volume_drive_type(&self, _path: &str) -> Option<DriveType> {
        *self.drive_type.lock().unwrap()
    }

    fn read_volume(&self, _volume_guid: &str) -> io::Result<RecycleBinVolumeSettings> {
        Ok(*self.volume.lock().unwrap())
    }

    fn get_user_quota_bytes(&self, _path: &str) -> Option<i64> {
        Some(100 * 1024 * MIB as i64)
    }
}

struct RestoreOutcome {
    result: Mutex<Result<bool, (io::ErrorKind, String)>>,
    calls: Mutex<Vec<(PathBuf, u64, DateTimeUtc)>>,
}

impl RestoreOutcome {
    fn returning(result: Result<bool, (io::ErrorKind, String)>) -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(result),
            calls: Mutex::new(Vec::new()),
        })
    }
}

impl RecycleBinRestoreBackend for RestoreOutcome {
    fn try_restore(
        &self,
        original_path: &Path,
        expected_size: u64,
        expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool> {
        self.calls
            .lock()
            .unwrap()
            .push((original_path.to_path_buf(), expected_size, expected_last_write_utc));
        match &*self.result.lock().unwrap() {
            Ok(value) => Ok(*value),
            Err((kind, message)) => Err(io::Error::new(*kind, message.clone())),
        }
    }
}

#[allow(clippy::too_many_arguments)] // This helper exposes each independent fail-closed seam to fake tests.
fn make_bin(
    settings: Arc<TestSettings>,
    log: Arc<TestLog>,
    drive_type: impl Fn(&str) -> Option<DriveType> + Send + Sync + 'static,
    mount_point: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    exists: impl Fn(&Path) -> bool + Send + Sync + 'static,
    length: impl Fn(&Path) -> io::Result<u64> + Send + Sync + 'static,
    recycle: impl Fn(&Path) -> io::Result<ShellRecycleOutcome> + Send + Sync + 'static,
    delete: impl Fn(&Path) -> io::Result<()> + Send + Sync + 'static,
    restore: Arc<dyn RecycleBinRestoreBackend>,
) -> recycle_bin::WindowsRecycleBin {
    let settings: Arc<dyn RecycleBinSettingsSource> = settings;
    let log: Arc<dyn Log> = log;
    recycle_bin::WindowsRecycleBin::with_adapters(
        settings,
        log,
        drive_type,
        mount_point,
        exists,
        length,
        recycle,
        delete,
        restore,
    )
}

fn healthy_bin(
    drive_type: Option<DriveType>,
    exists: impl Fn(&Path) -> bool + Send + Sync + 'static,
    length: impl Fn(&Path) -> io::Result<u64> + Send + Sync + 'static,
    recycle: impl Fn(&Path) -> io::Result<ShellRecycleOutcome> + Send + Sync + 'static,
    delete: impl Fn(&Path) -> io::Result<()> + Send + Sync + 'static,
    restore: Arc<dyn RecycleBinRestoreBackend>,
) -> recycle_bin::WindowsRecycleBin {
    let settings = Arc::new(TestSettings::default());
    *settings.drive_type.lock().unwrap() = drive_type;
    make_bin(
        settings,
        Arc::new(TestLog::default()),
        move |_| drive_type,
        |_| Some(r"C:\".to_owned()),
        exists,
        length,
        recycle,
        delete,
        restore,
    )
}

#[test]
// C# test: WindowsRecycleBinFitsVerdictTests.Fits_HealthySettings_True
fn fits_in_recycle_bin_uses_capacity_guard() {
    let bin = healthy_bin(
        Some(DriveType::Fixed),
        |_| false,
        |_| Ok(1),
        |_| panic!("preflight must not call the Shell"),
        |_| panic!("preflight must not delete"),
        RestoreOutcome::returning(Ok(false)),
    );
    assert!(bin.fits_in_recycle_bin(Path::new(PHOTO), 1));
}

#[test]
// C# test: WindowsRecycleBinFitsVerdictTests.Fits_SettingsUnreadable_False
fn fits_in_recycle_bin_returns_false_when_settings_are_unreadable() {
    let settings = Arc::new(TestSettings::default());
    settings.fail_policy.store(true, Ordering::SeqCst);
    let bin = make_bin(
        settings,
        Arc::new(TestLog::default()),
        |_| Some(DriveType::Fixed),
        |_| Some(r"C:\".to_owned()),
        |_| false,
        |_| Ok(1),
        |_| panic!("settings failure must refuse before Shell"),
        |_| panic!("settings failure must refuse before delete"),
        RestoreOutcome::returning(Ok(false)),
    );
    assert!(!bin.fits_in_recycle_bin(Path::new(PHOTO), 1));
}

#[test]
// C# test: WindowsRecycleBinFitsVerdictTests.Fits_VolumeBinOff_False
fn fits_in_recycle_bin_returns_false_when_volume_bin_is_disabled() {
    let settings = Arc::new(TestSettings::default());
    settings.volume.lock().unwrap().nuke_on_delete = Some(1);
    let bin = make_bin(
        settings,
        Arc::new(TestLog::default()),
        |_| Some(DriveType::Fixed),
        |_| Some(r"C:\".to_owned()),
        |_| false,
        |_| Ok(1),
        |_| panic!("disabled bin must refuse before Shell"),
        |_| panic!("disabled bin must refuse before delete"),
        RestoreOutcome::returning(Ok(false)),
    );
    assert!(!bin.fits_in_recycle_bin(Path::new(PHOTO), 1));
}

#[test]
// C# test: WindowsRecycleBinFitsVerdictTests.Fits_NoRecycleFilesPolicy_False
fn fits_in_recycle_bin_returns_false_under_no_recycle_files_policy() {
    let settings = Arc::new(TestSettings::default());
    settings.policy.lock().unwrap().no_recycle_files = true;
    let bin = make_bin(
        settings,
        Arc::new(TestLog::default()),
        |_| Some(DriveType::Fixed),
        |_| Some(r"C:\".to_owned()),
        |_| false,
        |_| Ok(1),
        |_| panic!("policy refusal must not reach Shell"),
        |_| panic!("policy refusal must not delete"),
        RestoreOutcome::returning(Ok(false)),
    );
    assert!(!bin.fits_in_recycle_bin(Path::new(PHOTO), 1));
}

#[test]
// C# test: WindowsRecycleBinFitsVerdictTests.Fits_NonFixedVolume_False
fn fits_in_recycle_bin_returns_false_for_non_fixed_volume_or_unknown_type() {
    for kind in [
        None,
        Some(DriveType::Removable),
        Some(DriveType::Network),
        Some(DriveType::Unknown),
    ] {
        let settings = Arc::new(TestSettings::default());
        *settings.drive_type.lock().unwrap() = kind;
        let bin = make_bin(
            settings,
            Arc::new(TestLog::default()),
            move |_| kind,
            |_| Some(r"C:\".to_owned()),
            |_| false,
            |_| Ok(1),
            |_| panic!("non-fixed type must not reach Shell"),
            |_| panic!("non-fixed type must not delete"),
            RestoreOutcome::returning(Ok(false)),
        );
        assert!(!bin.fits_in_recycle_bin(Path::new(PHOTO), 1));
    }
}

#[test]
// C# test: WindowsRecycleBinFitsVerdictTests.Fits_UnresolvableVolumeGuid_False
fn fits_in_recycle_bin_returns_false_when_volume_guid_is_missing() {
    // Use the adapter constructor with a settings source whose queried volume has no GUID.
    struct NoGuid(TestSettings);
    impl RecycleBinSettingsSource for NoGuid {
        fn read_policy(&self) -> io::Result<RecycleBinPolicy> {
            self.0.read_policy()
        }
        fn get_volume_guid(&self, _path: &str) -> Option<String> {
            None
        }
        fn get_volume_drive_type(&self, _path: &str) -> Option<DriveType> {
            Some(DriveType::Fixed)
        }
        fn read_volume(&self, guid: &str) -> io::Result<RecycleBinVolumeSettings> {
            self.0.read_volume(guid)
        }
        fn get_user_quota_bytes(&self, path: &str) -> Option<i64> {
            self.0.get_user_quota_bytes(path)
        }
    }
    let settings: Arc<dyn RecycleBinSettingsSource> = Arc::new(NoGuid(TestSettings::default()));
    let log: Arc<dyn Log> = Arc::new(TestLog::default());
    let bin = recycle_bin::WindowsRecycleBin::with_adapters(
        settings,
        log,
        |_| Some(DriveType::Fixed),
        |_| Some(r"C:\".to_owned()),
        |_| false,
        |_| Ok(1),
        |_| panic!("unresolved settings must not reach Shell"),
        |_| panic!("unresolved settings must not delete"),
        RestoreOutcome::returning(Ok(false)),
    );
    assert!(!bin.fits_in_recycle_bin(Path::new(PHOTO), 1));
}

#[test]
// C# test: WindowsRecycleBinFitsVerdictTests.Fits_NegativeSize_False
fn fits_in_recycle_bin_rejects_size_outside_signed_capacity_range() {
    let bin = healthy_bin(
        Some(DriveType::Fixed),
        |_| false,
        |_| Ok(1),
        |_| panic!("invalid size must not reach Shell"),
        |_| panic!("invalid size must not delete"),
        RestoreOutcome::returning(Ok(false)),
    );
    assert!(!bin.fits_in_recycle_bin(Path::new(PHOTO), u64::MAX));
}

#[test]
// C# test: RecycleBinPlatformGapTests.CanRecycle_FollowsTheVolumeTypeOfTheMountPoint
fn can_recycle_delegates_to_resolved_mount_drive_type() {
    for (kind, expected) in [
        (Some(DriveType::Fixed), true),
        (Some(DriveType::Removable), false),
        (Some(DriveType::Network), false),
        (None, false),
    ] {
        let bin = healthy_bin(
            kind,
            |_| false,
            |_| Ok(1),
            |_| panic!("eligibility query must not invoke recycle"),
            |_| panic!("eligibility query must not delete"),
            RestoreOutcome::returning(Ok(false)),
        );
        assert_eq!(bin.can_recycle(Path::new(PHOTO)), expected);
    }
}

#[test]
// C# test: RecycleBinDeleteGuardTests.SendToRecycleBin_FileGrewAfterPreflight_RefusedWithoutShell
fn send_to_recycle_bin_rechecks_grown_file_size_before_shell() {
    let size = Arc::new(AtomicU64::new(1));
    let shell_calls = Arc::new(AtomicUsize::new(0));
    let observed_size = Arc::clone(&size);
    let observed_shell = Arc::clone(&shell_calls);
    let bin = healthy_bin(
        Some(DriveType::Fixed),
        |_| false,
        move |_| Ok(observed_size.load(Ordering::SeqCst)),
        move |_| {
            observed_shell.fetch_add(1, Ordering::SeqCst);
            Ok(ShellRecycleOutcome::MovedToRecycleBin)
        },
        |_| panic!("recycle refusal must not switch to permanent delete"),
        RestoreOutcome::returning(Ok(false)),
    );
    assert!(bin.fits_in_recycle_bin(Path::new(PHOTO), 1));
    size.store(500 * MIB, Ordering::SeqCst);

    let error = bin.send_to_recycle_bin(Path::new(PHOTO)).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(shell_calls.load(Ordering::SeqCst), 0);
}

#[test]
// C# test: RecycleBinDeleteGuardTests.SendToRecycleBin_SizeUnreadable_RefusedWithoutShell
fn send_to_recycle_bin_refuses_unreadable_size_without_shell() {
    let log = Arc::new(TestLog::default());
    let observed_log = Arc::clone(&log);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&calls);
    let bin = make_bin(
        Arc::new(TestSettings::default()),
        log,
        |_| Some(DriveType::Fixed),
        |_| Some(r"C:\".to_owned()),
        |_| false,
        |_| Err(io::Error::new(io::ErrorKind::PermissionDenied, "locked")),
        move |_| {
            observed_calls.fetch_add(1, Ordering::SeqCst);
            Ok(ShellRecycleOutcome::MovedToRecycleBin)
        },
        |_| panic!("size read failure must not delete"),
        RestoreOutcome::returning(Ok(false)),
    );
    let error = bin.send_to_recycle_bin(Path::new(PHOTO)).unwrap_err();
    assert!(error.to_string().contains("locked"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(observed_log.warnings.lock().unwrap().len(), 1);
}

#[test]
// C# test: RecycleBinPlatformGapTests.SendToRecycleBin_VolumeWithoutABin_ThrowsBeforeTheShellIsCalled
fn send_to_recycle_bin_refuses_non_fixed_or_unresolved_volume_before_size_read() {
    let size_reads = Arc::new(AtomicUsize::new(0));
    let shell_calls = Arc::new(AtomicUsize::new(0));
    let seen_size = Arc::clone(&size_reads);
    let seen_shell = Arc::clone(&shell_calls);
    let bin = healthy_bin(
        Some(DriveType::Unknown),
        |_| false,
        move |_| {
            seen_size.fetch_add(1, Ordering::SeqCst);
            Ok(1)
        },
        move |_| {
            seen_shell.fetch_add(1, Ordering::SeqCst);
            Ok(ShellRecycleOutcome::MovedToRecycleBin)
        },
        |_| panic!("ineligible drive must not be permanently deleted"),
        RestoreOutcome::returning(Ok(false)),
    );
    assert_eq!(
        bin.send_to_recycle_bin(Path::new(PHOTO)).unwrap_err().kind(),
        io::ErrorKind::Unsupported
    );
    assert_eq!(size_reads.load(Ordering::SeqCst), 0);
    assert_eq!(shell_calls.load(Ordering::SeqCst), 0);
}

#[test]
// C# test: RecycleBinPlatformGapTests.SendToRecycleBin_ShellReturnsButTheFileIsStillThere_IsAnErrorNotASuccess
fn send_to_recycle_bin_reports_failure_if_shell_leaves_source_in_place() {
    let shell_calls = Arc::new(AtomicUsize::new(0));
    let seen_shell = Arc::clone(&shell_calls);
    let bin = healthy_bin(
        Some(DriveType::Fixed),
        |_| true,
        |_| Ok(1),
        move |_| {
            seen_shell.fetch_add(1, Ordering::SeqCst);
            Ok(ShellRecycleOutcome::MovedToRecycleBin)
        },
        |_| panic!("a failed recycle must not become permanent delete"),
        RestoreOutcome::returning(Ok(false)),
    );
    assert!(bin.send_to_recycle_bin(Path::new(PHOTO)).is_err());
    assert_eq!(shell_calls.load(Ordering::SeqCst), 1);
}

#[test]
// C# test: RecycleBinDeleteGuardTests.SendToRecycleBin_BinDisabled_RefusedWithoutShell
fn send_to_recycle_bin_refuses_disabled_bin_without_shell_or_delete_fallback() {
    let settings = Arc::new(TestSettings::default());
    settings.volume.lock().unwrap().nuke_on_delete = Some(1);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_recycle = Arc::clone(&calls);
    let observed_delete = Arc::clone(&calls);
    let bin = make_bin(
        settings,
        Arc::new(TestLog::default()),
        |_| Some(DriveType::Fixed),
        |_| Some(r"C:\".to_owned()),
        |_| false,
        |_| Ok(1),
        move |_| {
            observed_recycle.fetch_add(1, Ordering::SeqCst);
            Ok(ShellRecycleOutcome::MovedToRecycleBin)
        },
        move |_| {
            observed_delete.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
        RestoreOutcome::returning(Ok(false)),
    );
    assert!(bin.send_to_recycle_bin(Path::new(PHOTO)).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
// C# test: RecycleBinPlatformGapTests.SendToRecycleBin_ShellRemovesTheFile_Succeeds
fn send_to_recycle_bin_accepts_fake_shell_removal_of_test_owned_temp_file() {
    let owned = TestOwnedFile::create();
    let path = owned.path.clone();
    let shell_path = path.clone();
    let bin = healthy_bin(
        Some(DriveType::Fixed),
        |candidate| candidate.exists(),
        |candidate| fs::metadata(candidate).map(|metadata| metadata.len()),
        move |_| fs::remove_file(&shell_path).map(|()| ShellRecycleOutcome::MovedToRecycleBin),
        |_| panic!("recycle test must not call permanent delete"),
        RestoreOutcome::returning(Ok(false)),
    );

    bin.send_to_recycle_bin(&path).unwrap();

    assert!(!path.exists());
}

#[test]
// C# test: RecycleBinDeleteGuardTests.DeletePermanently_FixedOrUnknownDrive_RefusesWithoutDeleting
fn delete_permanently_refuses_fixed_unknown_and_unresolved_volumes() {
    for kind in [Some(DriveType::Fixed), Some(DriveType::Unknown), None] {
        let deletes = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&deletes);
        let bin = healthy_bin(
            kind,
            |_| true,
            |_| Ok(1),
            |_| panic!("permanent delete refusal must not call recycle"),
            move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            RestoreOutcome::returning(Ok(false)),
        );
        assert_eq!(
            bin.delete_permanently(Path::new(PHOTO)).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(deletes.load(Ordering::SeqCst), 0);
    }
}

#[test]
// C# test: RecycleBinDeleteGuardTests.DeletePermanently_KnownNoBinDrive_Deletes
fn delete_permanently_runs_injected_delete_only_on_positive_no_bin_type() {
    let deletes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&deletes);
    let bin = healthy_bin(
        Some(DriveType::Removable),
        |_| false,
        |_| Ok(1),
        |_| panic!("permanent deletion is not a recycle fallback"),
        move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
        RestoreOutcome::returning(Ok(false)),
    );
    bin.delete_permanently(Path::new(PHOTO)).unwrap();
    assert_eq!(deletes.load(Ordering::SeqCst), 1);
}

#[test]
// C# test: RecycleBinDeleteGuardTests.DeletePermanently_DriveQueryThrows_Refuses
fn delete_permanently_refuses_when_mount_point_is_unresolved() {
    let deletes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&deletes);
    let bin = make_bin(
        Arc::new(TestSettings::default()),
        Arc::new(TestLog::default()),
        |_| Some(DriveType::Removable),
        |_| None,
        |_| true,
        |_| Ok(1),
        |_| panic!("unresolved mount must not recycle"),
        move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
        RestoreOutcome::returning(Ok(false)),
    );
    assert!(bin.delete_permanently(Path::new(PHOTO)).is_err());
    assert_eq!(deletes.load(Ordering::SeqCst), 0);
}

#[test]
// C# test: RecycleBinPlatformGapTests.DeletePermanently_FileSurvivesTheDelete_IsAnError
fn delete_permanently_reports_failure_if_injected_delete_leaves_source() {
    let deletes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&deletes);
    let bin = healthy_bin(
        Some(DriveType::Network),
        |_| true,
        |_| Ok(1),
        |_| panic!("delete path does not invoke recycle"),
        move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
        RestoreOutcome::returning(Ok(false)),
    );
    assert!(bin.delete_permanently(Path::new(PHOTO)).is_err());
    assert_eq!(deletes.load(Ordering::SeqCst), 1);
}

#[test]
// C# test: WindowsRecycleBin.TryRestore_DelegatesResultAndError
fn try_restore_preserves_backend_result_and_error() {
    let written = DateTimeUtc::MIN;
    let backend = RestoreOutcome::returning(Ok(true));
    let calls = Arc::clone(&backend);
    let bin = healthy_bin(
        Some(DriveType::Fixed),
        |_| false,
        |_| Ok(1),
        |_| panic!("restore does not recycle"),
        |_| panic!("restore does not delete"),
        backend,
    );
    assert!(bin.try_restore(Path::new(PHOTO), 123, written).unwrap());
    assert_eq!(
        calls.calls.lock().unwrap().as_slice(),
        &[(PathBuf::from(PHOTO), 123, written)]
    );

    let backend = RestoreOutcome::returning(Err((io::ErrorKind::Other, "shell restore failed".to_owned())));
    let bin = healthy_bin(
        Some(DriveType::Fixed),
        |_| false,
        |_| Ok(1),
        |_| panic!("restore does not recycle"),
        |_| panic!("restore does not delete"),
        backend,
    );
    assert_eq!(
        bin.try_restore(Path::new(PHOTO), 123, written).unwrap_err().to_string(),
        "shell restore failed"
    );

    let bin = healthy_bin(
        Some(DriveType::Fixed),
        |_| false,
        |_| Ok(1),
        |_| panic!("restore does not recycle"),
        |_| panic!("restore does not delete"),
        RestoreOutcome::returning(Ok(false)),
    );
    assert!(!bin.try_restore(Path::new(PHOTO), 123, written).unwrap());
}

#[test]
// C# test: WindowsRecycleBin.TryRestore_ExistingDestinationRefusesShell
fn try_restore_refuses_existing_destination_without_backend_call() {
    let backend = RestoreOutcome::returning(Ok(true));
    let observed = Arc::clone(&backend);
    let bin = healthy_bin(
        Some(DriveType::Fixed),
        |_| true,
        |_| Ok(1),
        |_| panic!("existing destination must not recycle"),
        |_| panic!("existing destination must not delete"),
        backend,
    );
    assert!(!bin.try_restore(Path::new(PHOTO), 123, DateTimeUtc::MIN).unwrap());
    assert!(observed.calls.lock().unwrap().is_empty());
}

struct TestOwnedFile {
    path: PathBuf,
}

impl TestOwnedFile {
    fn create() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir();
        loop {
            let id = NEXT.fetch_add(1, Ordering::SeqCst);
            let path = directory.join(format!("photoreview-rs-w6-02c-{}-{id}.tmp", std::process::id()));
            match File::options().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    file.write_all(b"owned-test-item").unwrap();
                    file.flush().unwrap();
                    return Self { path };
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("failed to create uniquely named test-owned temp file: {error}"),
            }
        }
    }
}

impl Drop for TestOwnedFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[test]
fn with_log_routes_production_refusals_to_the_supplied_log() {
    // ST-COM-RECYCLE T5: `WindowsRecycleBin::new` used NullLog, so "Recycle refused" never reached the app log. This
    // uses the production adapters but only read-only queries (drive type, file length) and a path that does not exist:
    // the size read fails before any Shell call, so nothing is recycled, deleted or restored.
    let path = std::env::temp_dir().join(format!("pr-recycle-with-log-{}-missing.jpg", std::process::id()));
    let log = Arc::new(TestLog::default());
    let bin = recycle_bin::WindowsRecycleBin::with_log(RestoreOutcome::returning(Ok(false)), log.clone());

    let result = bin.send_to_recycle_bin(&path);

    assert!(result.is_err());
    if result.as_ref().is_err_and(|e| e.kind() == io::ErrorKind::Unsupported) {
        return; // temp dir is on a non-fixed volume: refused before logging, nothing to assert
    }
    let warnings = log.warnings.lock().unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("Recycle refused (size unreadable)")),
        "{warnings:?}"
    );
}

/// Facade whose Shell seam reports `outcome`; `still_exists` is what the post-call `exists` check answers.
fn bin_with_shell_outcome(
    outcome: ShellRecycleOutcome,
    still_exists: bool,
) -> (recycle_bin::WindowsRecycleBin, Arc<TestLog>) {
    let log = Arc::new(TestLog::default());
    let bin = make_bin(
        Arc::new(TestSettings::default()),
        Arc::clone(&log),
        |_| Some(DriveType::Fixed),
        |_| Some(r"C:\".to_owned()),
        move |_| still_exists,
        |_| Ok(1),
        move |_| Ok(outcome),
        |_| panic!("a recycle outcome must never fall back to a permanent delete"),
        RestoreOutcome::returning(Ok(false)),
    );
    (bin, log)
}

#[test]
// ST-COM-RECYCLE C2 (no C# twin: C# trusts the Shell). Only a positive "moved" report with a vanished source is Ok.
fn send_to_recycle_bin_succeeds_only_for_moved_to_recycle_bin_with_source_gone() {
    let (bin, log) = bin_with_shell_outcome(ShellRecycleOutcome::MovedToRecycleBin, false);
    bin.send_to_recycle_bin(Path::new(PHOTO)).unwrap();
    assert!(log.warnings.lock().unwrap().is_empty());
}

#[test]
// ST-COM-RECYCLE C2: the Shell says moved but the source is still there -> error, as before.
fn send_to_recycle_bin_moved_but_source_still_present_is_an_error() {
    let (bin, _) = bin_with_shell_outcome(ShellRecycleOutcome::MovedToRecycleBin, true);
    let error = bin.send_to_recycle_bin(Path::new(PHOTO)).unwrap_err();
    assert!(error.to_string().contains("left the source in place"), "{error}");
}

#[test]
// ST-COM-RECYCLE C2: a permanent delete is an error that says so (journal must not record Recycle), and is logged.
fn send_to_recycle_bin_deleted_permanently_is_a_distinct_fail_closed_error() {
    let (bin, log) = bin_with_shell_outcome(ShellRecycleOutcome::DeletedPermanently, false);
    let error = bin.send_to_recycle_bin(Path::new(PHOTO)).unwrap_err();
    assert!(error.to_string().contains("permanently"), "{error}");
    assert!(!error.to_string().contains("left the source in place"), "{error}");
    let warnings = log.warnings.lock().unwrap();
    assert!(
        warnings.iter().any(|w| w.contains("deleted permanently")),
        "{warnings:?}"
    );
}

#[test]
// ST-COM-RECYCLE C2: even if the (fake) source still exists, a permanent-delete report stays the permanent-delete error.
fn send_to_recycle_bin_deleted_permanently_wins_over_the_exists_check() {
    let (bin, _) = bin_with_shell_outcome(ShellRecycleOutcome::DeletedPermanently, true);
    let error = bin.send_to_recycle_bin(Path::new(PHOTO)).unwrap_err();
    assert!(error.to_string().contains("permanently"), "{error}");
}

#[test]
// ST-COM-RECYCLE C2: no per-item report and the file is gone -> unconfirmed error (never Ok).
fn send_to_recycle_bin_unknown_with_source_gone_is_unconfirmed_error() {
    let (bin, log) = bin_with_shell_outcome(ShellRecycleOutcome::Unknown, false);
    let error = bin.send_to_recycle_bin(Path::new(PHOTO)).unwrap_err();
    assert!(error.to_string().contains("unconfirmed"), "{error}");
    assert!(!log.warnings.lock().unwrap().is_empty());
}

#[test]
// ST-COM-RECYCLE C2: no per-item report and the file is still there -> nothing was lost, "left in place".
fn send_to_recycle_bin_unknown_with_source_present_reports_left_in_place() {
    let (bin, _) = bin_with_shell_outcome(ShellRecycleOutcome::Unknown, true);
    let error = bin.send_to_recycle_bin(Path::new(PHOTO)).unwrap_err();
    assert!(error.to_string().contains("left the source in place"), "{error}");
}

#[test]
// ST-COM-RECYCLE C2 / L6: a failed HRESULT is reported with its value; a surviving source keeps the old message.
fn send_to_recycle_bin_failed_hresult_is_an_error_carrying_the_code() {
    let hr = 0x8007_0005u32 as i32;
    let (bin, _) = bin_with_shell_outcome(ShellRecycleOutcome::Failed(hr), false);
    let error = bin.send_to_recycle_bin(Path::new(PHOTO)).unwrap_err();
    assert!(error.to_string().contains("0x80070005"), "{error}");

    let (bin, _) = bin_with_shell_outcome(ShellRecycleOutcome::Failed(hr), true);
    let error = bin.send_to_recycle_bin(Path::new(PHOTO)).unwrap_err();
    assert!(error.to_string().contains("left the source in place"), "{error}");
}

#[test]
// ST-COM-RECYCLE C2: a seam error is propagated unchanged.
fn send_to_recycle_bin_propagates_a_shell_seam_error() {
    let bin = healthy_bin(
        Some(DriveType::Fixed),
        |_| false,
        |_| Ok(1),
        |_| Err(io::Error::new(io::ErrorKind::PermissionDenied, "shell denied")),
        |_| panic!("no permanent delete fallback"),
        RestoreOutcome::returning(Ok(false)),
    );
    let error = bin.send_to_recycle_bin(Path::new(PHOTO)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

fn long_path(total_units: usize) -> PathBuf {
    // The prefix is ASCII, so the whole path has exactly `total_units` UTF-16 units.
    let prefix = r"C:\photos\";
    let name_len = total_units - prefix.len() - ".jpg".len();
    PathBuf::from(format!("{prefix}{}.jpg", "a".repeat(name_len)))
}

#[test]
// ST-COM-RECYCLE C2: a path over MAX_PATH-1 units is refused before the size read and the Shell call.
fn send_to_recycle_bin_refuses_a_path_longer_than_259_units_before_shell() {
    let size_reads = Arc::new(AtomicUsize::new(0));
    let shell_calls = Arc::new(AtomicUsize::new(0));
    let seen_size = Arc::clone(&size_reads);
    let seen_shell = Arc::clone(&shell_calls);
    let log = Arc::new(TestLog::default());
    let bin = make_bin(
        Arc::new(TestSettings::default()),
        Arc::clone(&log),
        |_| Some(DriveType::Fixed),
        |_| Some(r"C:\".to_owned()),
        |_| false,
        move |_| {
            seen_size.fetch_add(1, Ordering::SeqCst);
            Ok(1)
        },
        move |_| {
            seen_shell.fetch_add(1, Ordering::SeqCst);
            Ok(ShellRecycleOutcome::MovedToRecycleBin)
        },
        |_| panic!("a long path must not be permanently deleted"),
        RestoreOutcome::returning(Ok(false)),
    );
    let path = long_path(260);
    assert_eq!(path.to_str().unwrap().len(), 260);
    let error = bin.send_to_recycle_bin(&path).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(size_reads.load(Ordering::SeqCst), 0);
    assert_eq!(shell_calls.load(Ordering::SeqCst), 0);
    assert!(
        log.warnings
            .lock()
            .unwrap()
            .iter()
            .any(|w| w.starts_with("Recycle refused (path too long)"))
    );
}

#[test]
// ST-COM-RECYCLE C2: boundary: exactly 259 units is still handed to the Shell seam.
fn send_to_recycle_bin_accepts_a_path_of_exactly_259_units() {
    let shell_calls = Arc::new(AtomicUsize::new(0));
    let seen_shell = Arc::clone(&shell_calls);
    let bin = healthy_bin(
        Some(DriveType::Fixed),
        |_| false,
        |_| Ok(1),
        move |_| {
            seen_shell.fetch_add(1, Ordering::SeqCst);
            Ok(ShellRecycleOutcome::MovedToRecycleBin)
        },
        |_| panic!("no permanent delete"),
        RestoreOutcome::returning(Ok(false)),
    );
    let path = long_path(259);
    assert_eq!(path.to_str().unwrap().len(), 259);
    bin.send_to_recycle_bin(&path).unwrap();
    assert_eq!(shell_calls.load(Ordering::SeqCst), 1);
}

#[test]
// ST-COM-RECYCLE C2: the limit counts UTF-16 units, not bytes or scalar values.
fn send_to_recycle_bin_counts_utf16_units_for_the_length_limit() {
    // U+1F4F7 is 2 UTF-16 units: 130 of them plus the 10-unit prefix and ".jpg" = 274 > 259, but only 144 chars.
    let name = "\u{1F4F7}".repeat(130);
    let path = PathBuf::from(format!(r"C:\photos\{name}.jpg"));
    assert!(path.to_str().unwrap().chars().count() < 259);
    let bin = healthy_bin(
        Some(DriveType::Fixed),
        |_| false,
        |_| Ok(1),
        |_| panic!("long path must not reach the Shell"),
        |_| panic!("no permanent delete"),
        RestoreOutcome::returning(Ok(false)),
    );
    assert_eq!(
        bin.send_to_recycle_bin(&path).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
#[ignore = "attended: needs a human watching, sends a temp file to the real Recycle Bin"]
// ST-COM-RECYCLE C2 native check (skeleton, never run by agents or CI). Creates its OWN temp file, sends it through the
// production Shell path and asserts the sink reported a recycle. Afterwards the human must restore or delete ONLY that
// item (name `photoreview-rs-w6-02c-<pid>-<n>.tmp`) from the Recycle Bin by hand; this test never touches the bin.
fn native_attended_recycle_of_own_temp_file_reports_moved_to_recycle_bin() {
    let owned = TestOwnedFile::create();
    let bin =
        recycle_bin::WindowsRecycleBin::with_log(RestoreOutcome::returning(Ok(false)), Arc::new(TestLog::default()));
    bin.send_to_recycle_bin(&owned.path)
        .expect("the Shell must report MovedToRecycleBin for a normal temp file");
    assert!(!owned.path.exists());
}
