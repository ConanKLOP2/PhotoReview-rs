//! `WindowsEngineHost` wiring (ST-BENCH-HOST). No benchmark is run and no real Recycle Bin is touched: the "real bin" is a
//! fake that records calls, and the default deleter only removes a temp copy inside a `TempRoot`.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pr_bench::cli::{EngineHost, UnwiredEngineHost};
use pr_bench_win::{DefaultFileLog, WindowsEngineHost};
use pr_core::abstractions::RecycleBin;
use pr_test_support::TempRoot;

#[derive(Default)]
struct RecordingBin {
    sent: Mutex<Vec<PathBuf>>,
}

impl RecycleBin for RecordingBin {
    fn send_to_recycle_bin(&self, path: &Path) -> io::Result<()> {
        self.sent.lock().unwrap().push(path.to_path_buf());
        Ok(())
    }
    fn try_restore(&self, _: &Path, _: u64, _: pr_core::time::DateTimeUtc) -> io::Result<bool> {
        Ok(false)
    }
}

fn host_with(use_real: bool) -> (WindowsEngineHost, Arc<AtomicUsize>, Arc<RecordingBin>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let bin = Arc::new(RecordingBin::default());
    let (c, b) = (Arc::clone(&calls), Arc::clone(&bin));
    let host = WindowsEngineHost::with_parts(
        Arc::new(DefaultFileLog),
        use_real,
        Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
            Arc::clone(&b) as Arc<dyn RecycleBin>
        }),
    );
    (host, calls, bin)
}

// C#: BenchmarkRecycleBin.Create(...) default: the temp copy is deleted directly, the real bin factory never runs.
#[test]
fn recycle_bin_without_opt_in_deletes_temp_copy_and_never_builds_real_bin() {
    let root = TempRoot::new("bench-host-default-bin");
    let file = root.path().join("copy.jpg");
    std::fs::write(&file, b"x").unwrap();
    let (host, calls, bin) = host_with(false);
    host.recycle_bin().send_to_recycle_bin(&file).unwrap();
    assert!(!file.exists(), "the default deleter removes the temp copy");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(bin.sent.lock().unwrap().is_empty());
}

// C#: BenchmarkRecycleBin.Create(...) opt-in: the real bin is used (here a fake; the file stays untouched).
#[test]
fn recycle_bin_with_opt_in_uses_the_real_bin_factory_once_per_call() {
    let root = TempRoot::new("bench-host-real-bin");
    let file = root.path().join("copy.jpg");
    std::fs::write(&file, b"x").unwrap();
    let (host, calls, bin) = host_with(true);
    host.recycle_bin().send_to_recycle_bin(&file).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(*bin.sent.lock().unwrap(), vec![file.clone()]);
    assert!(file.exists(), "the fake bin does not delete anything");
}

// The production host must supply what the executor needs: a decoder, a memory probe and the RAM size.
#[test]
fn executor_parts_carry_wic_decoder_memory_probe_and_physical_memory() {
    let (host, _, _) = host_with(false);
    let parts = host.executor_parts().expect("parts");
    assert!(parts.options.decoder.is_some());
    assert!(parts.options.memory_probe.is_some());
    assert!(parts.options.has_headroom.is_none(), "headroom comes from the OS probe");
    assert!(parts.options.log.is_some());
    assert!(parts.options.physical_memory_bytes > 0, "GlobalMemoryStatusEx answered");
}

#[test]
fn machine_name_is_computername() {
    let (host, _, _) = host_with(false);
    assert_eq!(host.machine_name(), std::env::var("COMPUTERNAME").ok());
}

// C#: AppLog.Enabled getter/setter go through the process-wide log; restored at the end.
#[test]
fn logging_flag_round_trips_through_the_default_log() {
    let (host, _, _) = host_with(false);
    let original = host.logging_enabled();
    host.set_logging_enabled(!original);
    assert_eq!(host.logging_enabled(), !original);
    assert_eq!(host.log().enabled(), !original);
    host.set_logging_enabled(original);
    assert_eq!(host.logging_enabled(), original);
}

// The unwired host stays available in pr-bench (and keeps failing honestly); the new host is a different type.
#[test]
fn unwired_host_is_still_exported_and_has_no_decoder() {
    assert!(UnwiredEngineHost.executor_parts().is_err());
}

// `--benchmark-list-profiles` must be identical to the one of the `pr-bench` binary (same catalog, no host involved).
#[test]
fn list_profiles_lists_the_thirty_profile_catalog() {
    let output = Command::new(env!("CARGO_BIN_EXE_pr-bench-win"))
        .arg("--benchmark-list-profiles")
        .output()
        .expect("start pr-bench-win");
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = text.split("\r\n").filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 30);
    assert!(lines.iter().all(|l| l.split('\t').count() == 8), "{text}");
}
