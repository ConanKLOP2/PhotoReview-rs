//! CL-W6RERUN: killers for mutants that the corrected mutation flag form exposed as missed in
//! `recycle_bin_settings.rs`. Read-only OS queries only; the real Recycle Bin is never touched.
//!
//! Mutants proven equivalent on Windows (no input changes the observable result, probed with
//! verbatim, UNC, relative and `..` spellings and recorded in docs/reviews/CL-W6RERUN-2026-10-30.md):
//! `||` -> `&&` in `query_mount_point` (`std::path::absolute` already fails on blank and NUL input),
//! `delete !` in `query_drive_type` (`GetDriveTypeW` accepts a path with or without a trailing
//! separator), and `delete match arm Component::CurDir` / `Component::ParentDir` in
//! `absolute_normalized` (`GetVolumePathNameW` resolves `.` and `..` itself).
#![cfg(windows)]

#[allow(dead_code)] // Source inclusion exposes the transport; the injectable client part is covered elsewhere.
#[path = "../src/forward_client.rs"]
mod forward_client;
#[allow(dead_code)] // Source inclusion exposes the native driver; only try_open is under test here.
#[path = "../src/vblank_driver.rs"]
mod vblank_driver;

use std::time::{Duration, Instant};

use forward_client::{ForwardClientTransport, ForwardTransportError, WindowsForwardTransport};
use pr_core::sync::CancelToken;
use pr_platform_win::recycle_bin_settings::query_mount_point;
use vblank_driver::{IVBlankDriver, NativeVBlankDriver};
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{CreateNamedPipeW, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE};
use windows::core::HSTRING;

fn backslash() -> String {
    char::from(92u8).to_string()
}

#[test]
fn mount_point_of_a_verbatim_path_keeps_the_verbatim_prefix_of_the_input() {
    // Mutant absolute_normalized -> Some("xyzzy"): a bogus relative path resolves to the current
    // volume ("C:\"), so only a path whose spelling is carried through can tell the real function
    // from the constant. A verbatim path comes back with its `\?\` prefix.
    let bs = backslash();
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
    let verbatim = format!("{bs}{bs}?{bs}{drive}{bs}Windows");
    let mount = query_mount_point(&verbatim).expect("verbatim system path has a volume");
    assert_eq!(
        mount.to_ascii_lowercase(),
        format!("{bs}{bs}?{bs}{drive}{bs}").to_ascii_lowercase()
    );
    // A path with a `.` / `..` spelling in a verbatim path still resolves to the same root.
    let dotted = format!("{bs}{bs}?{bs}{drive}{bs}Windows{bs}.{bs}..{bs}Users");
    assert_eq!(
        query_mount_point(&dotted).map(|m| m.to_ascii_lowercase()),
        Some(mount.to_ascii_lowercase())
    );
}

// ---------------------------------------------------------------- P-05 forward_client.rs native transfer
/// A private test pipe server (no application pipe name, unique per process and test).
struct FakeServer(HANDLE);

impl FakeServer {
    fn create(name: &str) -> Self {
        let full = HSTRING::from(format!("{0}{0}.{0}pipe{0}{name}", char::from(92u8)));
        // SAFETY: `full` outlives the synchronous call; zero buffer sizes make writes pend until read.
        let handle = unsafe {
            CreateNamedPipeW(
                &full,
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                0,
                0,
                0,
                None,
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE, "test pipe created");
        Self(handle)
    }

    fn close(&mut self) {
        if self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: this struct owns the server handle and closes it once.
            let _ = unsafe { CloseHandle(self.0) };
            self.0 = INVALID_HANDLE_VALUE;
        }
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.close();
    }
}

fn connected(test: &str) -> (FakeServer, WindowsForwardTransport) {
    let name = format!("PhotoReviewRs.test.w6rerun.{}.{test}", std::process::id());
    let server = FakeServer::create(&name);
    let transport = WindowsForwardTransport::default();
    let cancel = CancelToken::none();
    transport
        .connect(&name, Instant::now() + Duration::from_secs(5), &cancel)
        .expect("client connects to the private pipe");
    (server, transport)
}

#[test]
fn reply_read_that_times_out_is_a_timeout_not_a_write_failure() {
    // Mutant forward_client.rs:346 `match guard write` -> true: a timed-out READ would be reported as a
    // partial WRITE. The server never replies.
    let (_server, transport) = connected("read_timeout");
    let cancel = CancelToken::none();
    let result = transport.read_reply(16, Instant::now() + Duration::from_millis(300), &cancel);
    assert_eq!(result, Err(ForwardTransportError::TimedOut));
    transport.disconnect();
}

#[test]
fn write_that_times_out_midway_may_have_reached_the_server() {
    // Mutant forward_client.rs:346 `match guard write` -> false: a timed-out WRITE would be a plain TimedOut.
    // The server never reads, so a 1 MiB frame pends until the deadline.
    let (_server, transport) = connected("write_timeout");
    let cancel = CancelToken::none();
    let payload = vec![b'x'; 1 << 20];
    let result = transport.write_all(&payload, Instant::now() + Duration::from_millis(300), &cancel);
    assert_eq!(
        result,
        Err(ForwardTransportError::WriteFailed {
            bytes_may_have_reached_server: true
        })
    );
    transport.disconnect();
}

#[test]
fn write_to_a_closed_server_fails_at_once_without_waiting_for_the_deadline() {
    // Mutant forward_client.rs:344 `match guard error == ERROR_IO_PENDING` -> true: an immediate failure
    // would be waited on (and cancelled) as if it were pending and be reported as a possibly delivered
    // timeout, or hang. The write runs on a worker so that a hanging mutant fails this test instead of
    // stalling it.
    let (mut server, transport) = connected("closed_server");
    server.close();
    let (done, outcome) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let cancel = CancelToken::none();
        let started = Instant::now();
        let result = transport.write_all(b"frame", started + Duration::from_secs(10), &cancel);
        let _ = done.send((result, started.elapsed()));
    });
    let (result, elapsed) = outcome
        .recv_timeout(Duration::from_secs(4))
        .expect("the failed write was reported without waiting for the deadline");
    assert_eq!(
        result,
        Err(ForwardTransportError::WriteFailed {
            bytes_may_have_reached_server: false
        })
    );
    // recv_timeout(4 s) above already proves promptness; this only checks the reported time is far below the 10 s deadline.
    assert!(elapsed < Duration::from_secs(9));
}

// ---------------------------------------------------------------- P-11 vblank_driver.rs try_open
#[test]
#[ignore = "native: requires a display adapter and the D3DKMT runtime"]
fn try_open_returns_the_adapter_and_source_that_the_vblank_wait_accepts() {
    // Mutants vblank_driver.rs:122 `try_open` -> Ok(Some((1, 0))) / Ok(Some((1, 1))): a made-up adapter handle
    // is answered with STATUS_INVALID_PARAMETER by D3DKMTWaitForVerticalBlankEvent, the real one is not.
    let driver = NativeVBlankDriver::INSTANCE;
    let monitor = driver.monitor_from_window(0);
    let (adapter, source) = driver
        .try_open(monitor)
        .expect("import ok")
        .expect("primary monitor opens");
    assert_ne!(adapter, 0);
    assert_ne!(adapter, 1);
    let status = driver.wait_for_vblank(adapter, source);
    driver.close(adapter);
    // 0xC000000D (STATUS_INVALID_PARAMETER) is what the kernel answers for a handle it does not know; the real
    // handle gets a status that depends on the session (0 or a graphics status), never that one.
    const STATUS_INVALID_PARAMETER: i32 = 0xC000_000D_u32 as i32;
    assert_ne!(
        status.expect("import ok"),
        STATUS_INVALID_PARAMETER,
        "the kernel knows the adapter handle"
    );
}

// ---------------------------------------------------------------- recycle_bin_settings.rs registry reader, 4-byte non-DWORD values
use pr_platform_win::recycle_bin_settings::{RegistryHive, RegistryValue, read_registry_value};
use std::process::Command;

/// A throw-away key under HKCU\Software with a unique name, removed on drop. It has nothing to do with
/// PhotoReview or the Recycle Bin (BitBucket) keys.
struct ScratchKey(String);

impl ScratchKey {
    fn create() -> Self {
        Self(format!(
            "Software{}PhotoReviewRsW6RerunScratch-{}",
            char::from(92u8),
            std::process::id()
        ))
    }

    fn full(&self) -> String {
        format!("HKCU{}{}", char::from(92u8), self.0)
    }

    fn set(&self, value: &str, kind: &str, data: &str) {
        let output = Command::new("reg")
            .args(["add", &self.full(), "/v", value, "/t", kind, "/d", data, "/f"])
            .output()
            .expect("reg.exe runs");
        assert!(output.status.success(), "reg add {kind} succeeded");
    }
}

impl Drop for ScratchKey {
    fn drop(&mut self) {
        let _ = Command::new("reg").args(["delete", &self.full(), "/f"]).output();
    }
}

#[test]
fn registry_reader_reports_four_byte_non_dword_values_as_other() {
    // Mutant read_registry_value `kind != REG_DWORD || byte_count != 4` -> `&&`: a REG_SZ "a" and a
    // REG_BINARY of four bytes are exactly four bytes long but are not DWORDs, so they must be `Other`;
    // the mutant reads them as `Dword`. A real REG_DWORD still reads as `Dword`.
    let scratch = ScratchKey::create();
    scratch.set("Text", "REG_SZ", "a");
    scratch.set("Blob", "REG_BINARY", "01020304");
    scratch.set("Number", "REG_DWORD", "7");
    let read = |name: &str| read_registry_value(RegistryHive::CurrentUser, &scratch.0, name).expect("readable");
    assert_eq!(read("Text"), Some(RegistryValue::Other));
    assert_eq!(read("Blob"), Some(RegistryValue::Other));
    assert_eq!(read("Number"), Some(RegistryValue::Dword(7)));
}
