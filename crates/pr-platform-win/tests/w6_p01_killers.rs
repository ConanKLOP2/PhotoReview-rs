//! CL-P01: killers for glue mutants left by CX-HOST2 (P-03 settings adapters, P-05 descriptor drop, P-11 display
//! clock). C# oracle: 7eaee2a2. Only read-only OS queries, fakes and in-memory objects; the real Recycle Bin is
//! never touched and no window is opened.
#![cfg(windows)]

#[path = "../src/display_clock.rs"]
mod display_clock;
#[allow(dead_code)] // Only the descriptor type is under test.
#[path = "../src/forward_pipe.rs"]
mod forward_pipe;
#[allow(dead_code)] // Required by forward_pipe; not under test here.
#[path = "../src/instance_keys.rs"]
mod instance_keys;
#[allow(dead_code)] // Source inclusion provides the fake-driver seam, not native adapter coverage.
#[path = "../src/vblank_driver.rs"]
mod vblank_driver;

use std::sync::{
    Arc, Mutex,
    mpsc::{self, Receiver, Sender},
};
use std::time::{Duration, Instant};

use display_clock::WindowsDisplayClock;
use pr_core::abstractions::{DisplayClock, DisplayTiming};
use pr_platform_win::recycle_bin_settings::{
    DriveType, RecycleBinSettingsSource, RegistryHive, RegistryValue, WindowsRecycleBinSettingsSource,
    drive_type_from_win32, query_drive_type, query_mount_point, read_registry_value,
};
use vblank_driver::{IVBlankDriver, NativeImportError};

unsafe extern "system" {
    fn QueryPerformanceFrequency(frequency: *mut i64) -> i32;
}

const DEADLINE: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------- P-11 display_clock.rs QPC wrappers

#[test]
fn qpc_frequency_is_the_value_reported_by_the_os() {
    // Mutants display_clock.rs:393 (-> 0/1/-1), 397:65 (!= -> ==), 397:83 (> -> ==, <).
    // C# WindowsDisplayClock uses Stopwatch.Frequency, the QueryPerformanceFrequency value.
    let mut expected = 0_i64;
    // SAFETY: valid writable i64 for the synchronous call.
    assert_ne!(unsafe { QueryPerformanceFrequency(&mut expected) }, 0);
    assert!(expected > 0);
    assert_eq!(display_clock::qpc_frequency(), expected);
}

#[test]
fn qpc_timestamp_is_positive_and_advances() {
    // Mutants display_clock.rs:407 (-> 0/1/-1) and 409:57 (!= -> ==, which yields 0 on success).
    let first = display_clock::qpc_timestamp();
    assert!(first > 1, "QPC counter is a large positive tick count, got {first}");
    let start = Instant::now();
    let mut advanced = first;
    while advanced <= first {
        assert!(start.elapsed() < DEADLINE, "QPC did not advance");
        std::hint::spin_loop();
        advanced = display_clock::qpc_timestamp();
    }
    // The delta in ticks is tiny relative to one second of ticks (counter is not a wall clock constant).
    assert!(advanced - first < display_clock::qpc_frequency() * 20);
}

#[test]
#[ignore = "native: requires a Windows desktop with DWM composition"]
fn static_dwm_timing_returns_a_positive_grid() {
    // Mutants display_clock.rs:110 (dwm_timing -> None / Some(Default)).
    // C# DwmTiming_Static_ReturnsNullOrAPositiveGrid; on a composed desktop it is a positive grid.
    let timing = WindowsDisplayClock::dwm_timing().expect("DWM reports a timing grid on a composed desktop");
    assert!(timing.refresh_period > 0 && timing.last_vblank > 0);
}

// ---------------------------------------------------------------- P-11 worker bookkeeping and priority

struct GateDriver {
    waiting: Sender<(u32, i32)>,
    closed: Sender<u32>,
    release: Mutex<Receiver<()>>,
    failing_adapter: u32,
}

impl IVBlankDriver for GateDriver {
    fn monitor_from_window(&self, window: isize) -> isize {
        window
    }

    fn try_open(&self, monitor: isize) -> Result<Option<(u32, u32)>, NativeImportError> {
        Ok(Some((monitor as u32, 3)))
    }

    fn wait_for_vblank(&self, adapter: u32, _: u32) -> Result<i32, NativeImportError> {
        use windows::Win32::System::Threading::{GetCurrentThread, GetThreadPriority};
        // SAFETY: pseudo-handle of the calling thread.
        let priority = unsafe { GetThreadPriority(GetCurrentThread()) };
        self.waiting.send((adapter, priority)).expect("test owns receiver");
        if adapter == self.failing_adapter {
            return Ok(0x8000_0001_u32 as i32);
        }
        // Timeout is only a hang guard; nothing time-derived selects the outcome.
        self.release
            .lock()
            .expect("gate")
            .recv_timeout(DEADLINE)
            .expect("test releases the worker");
        Ok(0)
    }

    fn close(&self, adapter: u32) {
        let _ = self.closed.send(adapter);
    }

    fn dwm_timing(&self) -> Option<DisplayTiming> {
        None
    }
}

struct Rig {
    clock: WindowsDisplayClock,
    waiting: Receiver<(u32, i32)>,
    closed: Receiver<u32>,
    release: Sender<()>,
}

fn rig(failing_adapter: u32) -> Rig {
    let (waiting_tx, waiting) = mpsc::channel();
    let (closed_tx, closed) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let driver = Arc::new(GateDriver {
        waiting: waiting_tx,
        closed: closed_tx,
        release: Mutex::new(release_rx),
        failing_adapter,
    });
    let clock = WindowsDisplayClock::with_driver(None, Some(driver), Some(Arc::new(|| 1_000_000))).expect("fakes");
    Rig {
        clock,
        waiting,
        closed,
        release,
    }
}

#[test]
fn clock_worker_runs_at_highest_priority() {
    // Mutant display_clock.rs:321: set_clock_worker_priority -> Ok(()). THREAD_PRIORITY_HIGHEST == 2.
    // C# WindowsDisplayClock.Run sets Thread.Priority = ThreadPriority.Highest.
    let rig = rig(0);
    assert_eq!(rig.clock.get_timing(0xA1), None);
    let (adapter, priority) = rig.waiting.recv_timeout(DEADLINE).expect("worker waits");
    assert_eq!(adapter, 0xA1);
    assert_eq!(priority, 2);
    rig.clock.request_stop();
    rig.release.send(()).expect("release");
    rig.clock.shutdown_and_join();
}

#[test]
fn stopped_worker_leaves_the_active_slot() {
    // Mutant display_clock.rs:357: retire_worker -> (). After a stop-requested exit the handle must move out of
    // the active slot (C# `_thread = null` in the thread's finally), otherwise a later request could never
    // start a fresh worker.
    let rig = rig(0);
    assert_eq!(rig.clock.get_timing(0xC1), None);
    rig.waiting.recv_timeout(DEADLINE).expect("worker waits");
    assert!(rig.clock.worker_slots().0);
    rig.clock.request_stop();
    rig.release.send(()).expect("release");
    rig.closed.recv_timeout(DEADLINE).expect("adapter closed on exit");
    let start = Instant::now();
    while rig.clock.worker_slots().0 {
        assert!(start.elapsed() < DEADLINE, "worker kept the active slot after exiting");
        std::thread::yield_now();
    }
    rig.clock.shutdown_and_join();
}

#[test]
fn finished_retired_workers_are_reaped_on_the_next_request() {
    // Mutant display_clock.rs:368: reap_finished_workers -> (). A failed worker is retired; once its thread has
    // finished, the next get_timing must join and drop the handle instead of retaining it until shutdown.
    let rig = rig(0xA1);
    assert_eq!(rig.clock.get_timing(0xA1), None);
    rig.clock.await_failed_monitor(0xA1);
    assert_eq!(rig.closed.recv_timeout(DEADLINE).expect("failed adapter closed"), 0xA1);
    // A second monitor starts worker B, which blocks on the gate so it stays active; only A's handle is retired.
    let start = Instant::now();
    loop {
        let _ = rig.clock.get_timing(0xB1);
        if rig.clock.worker_slots().1 == 0 {
            break;
        }
        assert!(start.elapsed() < DEADLINE, "finished retired worker was never reaped");
        std::thread::yield_now();
    }
    assert!(rig.clock.worker_slots().0, "worker B is the only handle left");
    rig.clock.request_stop();
    rig.release.send(()).expect("release B");
    rig.clock.shutdown_and_join();
}

// ---------------------------------------------------------------- P-03 recycle_bin_settings.rs adapters

#[test]
fn drive_type_codes_map_to_the_c_sharp_enum_order() {
    // Mutants recycle_bin_settings.rs query_drive_type match arms (now drive_type_from_win32).
    // DRIVE_* values from fileapi.h; C# System.IO.DriveType uses the same numbering.
    let expected = [
        (0, DriveType::Unknown),
        (1, DriveType::NoRootDirectory),
        (2, DriveType::Removable),
        (3, DriveType::Fixed),
        (4, DriveType::Network),
        (5, DriveType::CdRom),
        (6, DriveType::Ram),
        (7, DriveType::Unknown),
        (u32::MAX, DriveType::Unknown),
    ];
    for (code, drive_type) in expected {
        assert_eq!(drive_type_from_win32(code), drive_type, "code {code}");
    }
}

fn system_root() -> String {
    format!("{}\\", std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned()))
}

#[test]
fn query_drive_type_classifies_the_system_drive_and_appends_the_separator() {
    // Mutants query_drive_type: `ends_with('\\')` negation (a bare "C:" must still resolve), NUL guard, windows arm.
    let with_separator = system_root();
    let without_separator = with_separator.trim_end_matches('\\').to_owned();
    assert_eq!(query_drive_type(&with_separator), Some(DriveType::Fixed));
    assert_eq!(query_drive_type(&without_separator), Some(DriveType::Fixed));
    assert_eq!(query_drive_type("C:\\\0"), None);
}

#[test]
fn query_mount_point_returns_the_volume_root_of_a_real_directory() {
    // Mutants query_mount_point: -> None / Some(empty) / Some("xyzzy"), blank and NUL guards, buffer handling.
    let temp = std::env::temp_dir();
    let temp = temp.to_string_lossy().into_owned();
    let mount = query_mount_point(&temp).expect("temp dir has a volume");
    assert!(mount.ends_with('\\'), "mount point {mount:?} ends with a separator");
    assert!(temp.to_ascii_lowercase().starts_with(&mount.to_ascii_lowercase()));
    assert_eq!(query_mount_point(""), None);
    assert_eq!(query_mount_point("   "), None);
    assert_eq!(query_mount_point("C:\\a\0b"), None);
    // A relative path is made absolute first; a non-normalized spelling resolves to the same volume.
    assert!(query_mount_point(&format!("{temp}\\.\\..")).is_some());
}

#[test]
fn default_source_resolves_guid_drive_type_and_quota_from_the_real_volume() {
    // Mutants query_volume_name (-> None/empty/"xyzzy"), query_total_bytes (-> None/Some(0)/Some(1)),
    // and the mount-point plumbing in get_volume_guid/get_volume_drive_type/get_user_quota_bytes.
    let source = WindowsRecycleBinSettingsSource::default();
    let temp = std::env::temp_dir().to_string_lossy().into_owned();
    let guid = source.get_volume_guid(&temp).expect("volume GUID");
    assert!(
        guid.starts_with('{') && guid.ends_with('}') && guid.len() == 38,
        "{guid}"
    );
    assert_eq!(source.get_volume_drive_type(&temp), Some(DriveType::Fixed));
    let quota = source.get_user_quota_bytes(&temp).expect("volume size");
    assert!(
        quota > 1 << 30,
        "a fixed system volume is larger than 1 GiB, got {quota}"
    );
}

const NT_VERSION_KEY: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion";

#[test]
fn registry_reader_distinguishes_dword_other_and_missing_values() {
    // Mutants read_registry_value: -> Ok(None)/Ok(Some(Dword(0|1)))/Ok(Some(Other)), status comparisons,
    // hive mapping, REG_DWORD/size check. Read-only reads of the OS version key (no Recycle Bin keys).
    match read_registry_value(RegistryHive::LocalMachine, NT_VERSION_KEY, "CurrentMajorVersionNumber") {
        Ok(Some(RegistryValue::Dword(major))) => assert!(major >= 10, "Windows 10 or newer, got {major}"),
        other => panic!("expected a DWORD, got {other:?}"),
    }
    assert!(matches!(
        read_registry_value(RegistryHive::LocalMachine, NT_VERSION_KEY, "ProductName"),
        Ok(Some(RegistryValue::Other))
    ));
    assert!(matches!(
        read_registry_value(RegistryHive::LocalMachine, NT_VERSION_KEY, "NoSuchValueCl"),
        Ok(None)
    ));
    assert!(matches!(
        read_registry_value(RegistryHive::LocalMachine, "SOFTWARE\\NoSuchKeyClP01\\Deeper", "X"),
        Ok(None)
    ));
    // The same key exists under HKCU but the DWORD only under HKLM: proves the hive switch is honored.
    assert!(matches!(
        read_registry_value(RegistryHive::CurrentUser, NT_VERSION_KEY, "CurrentMajorVersionNumber"),
        Ok(None)
    ));
    assert_eq!(
        read_registry_value(RegistryHive::LocalMachine, "SOFTWARE\0X", "V")
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
    assert_eq!(
        read_registry_value(RegistryHive::LocalMachine, NT_VERSION_KEY, "V\0X")
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
}

fn process_handle_count() -> u32 {
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
    let mut count = 0_u32;
    // SAFETY: pseudo-handle plus a valid output pointer.
    unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) }.expect("handle count");
    count
}

#[test]
fn registry_reader_closes_every_key_handle() {
    // Mutant recycle_bin_settings.rs: `RegistryKey::drop` -> () leaks one HKEY per read.
    // 400 reads would add 400 handles; unrelated concurrent tests add far fewer.
    let before = process_handle_count();
    for _ in 0..400 {
        let _ = read_registry_value(RegistryHive::LocalMachine, NT_VERSION_KEY, "CurrentMajorVersionNumber");
    }
    let after = process_handle_count();
    assert!(
        after < before + 150,
        "registry key handles leaked: {before} -> {after} after 400 reads"
    );
}

// ---------------------------------------------------------------- P-05 PipeSecurityDescriptor::drop

fn private_bytes() -> usize {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // SAFETY: PROCESS_MEMORY_COUNTERS_EX starts with PROCESS_MEMORY_COUNTERS; cb is its size.
    unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            counters.cb,
        )
    }
    .expect("memory info");
    counters.PrivateUsage
}

#[test]
fn dropping_a_pipe_security_descriptor_frees_its_allocation() {
    // Mutant forward_pipe.rs:Drop for PipeSecurityDescriptor -> (): the LocalAlloc'd descriptor leaks.
    // Observed through the process private-byte count over many create/drop cycles (growth stays far below
    // the several MiB a per-iteration leak would add).
    const CYCLES: usize = 60_000;
    // Warm the allocator so the baseline includes its steady-state working set.
    for _ in 0..2_000 {
        drop(forward_pipe::PipeSecurityDescriptor::for_sid("S-1-5-18").expect("descriptor"));
    }
    let before = private_bytes();
    for _ in 0..CYCLES {
        drop(forward_pipe::PipeSecurityDescriptor::for_sid("S-1-5-18").expect("descriptor"));
    }
    let after = private_bytes();
    let grown = after.saturating_sub(before);
    assert!(
        grown < 3 * 1024 * 1024,
        "descriptor allocations leaked: {grown} bytes over {CYCLES} cycles"
    );
}
