//! Q11 killers for P-05 (pipes): Debug/Display of the pipe types and the native client transport.
//! Tests only. Every pipe is private (unique per process and test, never the application pipe name); the real
//! Recycle Bin is not touched.
//!
//! Mutants (Q11 run, 2026-11-04) these tests are written for:
//! `forward_client.rs:71` / `:207` Debug, `:389` connect retry guard, `:409` `server_is_current_user -> true`,
//! `:428` `allow_server_foreground -> Ok(())`, `:507` `read_reply` `<` -> `<=`;
//! `forward_server.rs:44` Display, `:101` Debug, `:440` `Drop for NativeForwardPipeFactory`.
//! C# has no Debug/Display counterpart; those tests pin what a log line must contain.

#[cfg(windows)]
#[allow(dead_code)] // Source inclusion exposes the crate-private native driver; only the Windows methods are under test.
#[path = "../src/vblank_driver.rs"]
mod vblank_driver;

use pr_platform_win::forward_client::ForwardPipeClient;
use pr_platform_win::forward_server::{ForwardPipeError, InstanceForwardServer};

#[test]
fn forward_pipe_error_display_names_each_failure() {
    assert_eq!(ForwardPipeError::Canceled.to_string(), "operation canceled");
    assert_eq!(ForwardPipeError::TimedOut.to_string(), "named pipe request timed out");
    assert_eq!(ForwardPipeError::Failed("boom".to_owned()).to_string(), "boom");
}

#[test]
fn forward_pipe_client_debug_shows_name_and_foreground_flag_without_touching_the_pipe() {
    let client = ForwardPipeClient::new(Some("PhotoReviewRs.q11.debug".to_owned()), true).expect("client");
    let text = format!("{client:?}");
    assert!(text.contains("ForwardPipeClient"), "{text}");
    assert!(text.contains("PhotoReviewRs.q11.debug"), "{text}");
    assert!(text.contains("allow_server_foreground: true"), "{text}");
}

#[test]
fn instance_forward_server_debug_shows_name_and_disposed_flag_without_starting() {
    let server =
        InstanceForwardServer::new("PhotoReviewRs.q11.debug.server", |_| {}, None, None, None).expect("server");
    let text = format!("{server:?}");
    assert!(text.contains("InstanceForwardServer"), "{text}");
    assert!(text.contains("PhotoReviewRs.q11.debug.server"), "{text}");
    assert!(text.contains("disposed: false"), "{text}");
}

// ---------------------------------------------------------------- native transport (private test pipes only)
#[cfg(windows)]
mod native {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use pr_core::sync::CancelToken;
    use pr_platform_win::forward_client::{
        ForegroundPermissionError, ForwardClientTransport, ForwardTransportError, WindowsForwardTransport,
    };
    use pr_platform_win::forward_server::InstanceForwardServer;
    use windows::Win32::Foundation::{CloseHandle, ERROR_IO_PENDING, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, PIPE_ACCESS_INBOUND, ReadFile, WriteFile,
    };
    use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows::Win32::System::Pipes::{CreateNamedPipeW, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE};
    use windows::Win32::System::Threading::{
        CreateEventW, GetCurrentProcess, GetProcessHandleCount, WaitForSingleObject,
    };
    use windows::core::HSTRING;

    fn pipe_path(name: &str) -> String {
        format!("{0}{0}.{0}pipe{0}{name}", char::from(92u8))
    }

    /// A private same-user server end.
    struct Server(HANDLE);

    impl Server {
        fn create(name: &str) -> Self {
            let full = HSTRING::from(pipe_path(name));
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
    }

    impl Drop for Server {
        fn drop(&mut self) {
            // SAFETY: this struct owns the server handle and closes it once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    fn connected(test: &str) -> (Server, WindowsForwardTransport) {
        let name = format!("PhotoReviewRs.test.q11.{}.{test}", std::process::id());
        let server = Server::create(&name);
        let transport = WindowsForwardTransport::default();
        transport
            .connect(&name, Instant::now() + Duration::from_secs(5), &CancelToken::none())
            .expect("client connects to the private pipe");
        (server, transport)
    }

    #[test]
    #[ignore = "native: creates a uniquely named private test pipe"]
    fn transport_debug_names_the_type() {
        // Mutant forward_client.rs:207 `Debug::fmt -> Ok(Default::default())`.
        assert_eq!(
            format!("{:?}", WindowsForwardTransport::default()),
            "WindowsForwardTransport"
        );
    }

    #[test]
    #[ignore = "native: creates a uniquely named private test pipe"]
    fn unconnected_transport_is_not_a_current_user_server_and_has_no_foreground_target() {
        // Mutants `server_is_current_user -> true` (:409) and `allow_server_foreground -> Ok(())` (:428):
        // with no connected pipe both must refuse.
        let transport = WindowsForwardTransport::default();
        assert!(!transport.server_is_current_user());
        assert_eq!(
            transport.allow_server_foreground(),
            Err(ForegroundPermissionError::InvalidOperation)
        );
    }

    #[test]
    #[ignore = "native: creates a uniquely named private test pipe"]
    fn same_user_private_server_is_recognised_as_current_user() {
        // Pairs with the test above: the real token comparison says "same user" for our own pipe server.
        let (_server, transport) = connected("same_user");
        assert!(transport.server_is_current_user());
        transport.disconnect();
    }

    #[test]
    #[ignore = "native: creates a uniquely named private test pipe"]
    fn connect_to_a_pipe_that_refuses_this_client_fails_at_once_instead_of_polling_until_the_deadline() {
        // Mutant :389 retry guard -> true: every CreateFile error would be retried until the deadline. An inbound-only
        // pipe refuses a read/write client with ERROR_ACCESS_DENIED, which is neither "missing" nor "busy": NoServer now.
        let name = format!("PhotoReviewRs.test.q11.{}.inbound_only", std::process::id());
        let full = HSTRING::from(pipe_path(&name));
        // SAFETY: `full` outlives the synchronous call.
        let handle = unsafe {
            CreateNamedPipeW(
                &full,
                PIPE_ACCESS_INBOUND,
                PIPE_TYPE_BYTE | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                0,
                0,
                0,
                None,
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE, "test pipe created");
        let server = Server(handle);
        let (done, outcome) = mpsc::channel();
        std::thread::spawn(move || {
            let transport = WindowsForwardTransport::default();
            let started = Instant::now();
            let result = transport.connect(&name, started + Duration::from_secs(30), &CancelToken::none());
            let _ = done.send((result, started.elapsed()));
        });
        let (result, elapsed) = outcome
            .recv_timeout(Duration::from_secs(10))
            .expect("connect failed without waiting for its 30 s deadline");
        drop(server);
        assert_eq!(result, Err(ForwardTransportError::NoServer));
        assert!(elapsed < Duration::from_secs(20), "{elapsed:?}");
    }

    #[test]
    #[ignore = "native: creates a uniquely named private test pipe"]
    fn reply_of_exactly_the_limit_is_returned_without_reading_one_byte_more() {
        // Mutant :507 `reply.len() < limit` -> `<=`: the loop would block for a fifth byte and time out.
        let (server, transport) = connected("exact_limit");
        let (done, outcome) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = transport.read_reply(4, Instant::now() + Duration::from_secs(3), &CancelToken::none());
            let _ = done.send(result);
            transport
        });
        // SAFETY: a manual-reset event with default security, closed below.
        let event = unsafe { CreateEventW(None, true, false, None) }.expect("event");
        let mut overlapped = OVERLAPPED {
            hEvent: event,
            ..Default::default()
        };
        // SAFETY: the buffer and OVERLAPPED stay alive until GetOverlappedResult(wait) below drains the write.
        let _ = unsafe { WriteFile(server.0, Some(b"ABCD"), None, Some(&mut overlapped)) };
        let reply = outcome
            .recv_timeout(Duration::from_secs(8))
            .expect("read_reply returned");
        assert_eq!(reply, Ok(b"ABCD".to_vec()));
        let mut transferred = 0;
        // SAFETY: drains the write started above on the same handle/OVERLAPPED.
        let _ = unsafe { GetOverlappedResult(server.0, &overlapped, &mut transferred, true) };
        // SAFETY: closes the event created above exactly once.
        let _ = unsafe { CloseHandle(event) };
        worker.join().expect("worker").disconnect();
    }

    /// True when a pending 1-byte read on the server end completes within 3 s because the client hung up.
    fn server_sees_hangup(server: &Server) -> bool {
        // SAFETY: a manual-reset event with default security, closed below.
        let event = unsafe { CreateEventW(None, true, false, None) }.expect("event");
        let mut overlapped = OVERLAPPED {
            hEvent: event,
            ..Default::default()
        };
        let mut byte = [0u8; 1];
        // SAFETY: the buffer and OVERLAPPED stay alive until the operation is drained below.
        if let Err(error) = unsafe { ReadFile(server.0, Some(&mut byte), None, Some(&mut overlapped)) } {
            if error.code() != ERROR_IO_PENDING.to_hresult() {
                // Failed at once (broken or not connected pipe): the client end is gone, nothing is pending.
                // SAFETY: closes the event created above exactly once.
                let _ = unsafe { CloseHandle(event) };
                return true;
            }
        }
        // SAFETY: waits on the event created above.
        let completed = unsafe { WaitForSingleObject(event, 3000) } == WAIT_OBJECT_0;
        let mut transferred = 0;
        if !completed {
            // SAFETY: cancels only the read started above before its buffer leaves scope.
            let _ = unsafe { CancelIoEx(server.0, Some(&overlapped)) };
        }
        // SAFETY: drains the operation (completed or canceled) before buffer and OVERLAPPED are released.
        let _ = unsafe { GetOverlappedResult(server.0, &overlapped, &mut transferred, true) };
        // SAFETY: closes the event created above exactly once.
        let _ = unsafe { CloseHandle(event) };
        completed
    }

    #[test]
    #[ignore = "native: creates a uniquely named private test pipe"]
    fn disconnect_closes_the_client_end_so_the_server_sees_a_hangup() {
        // Mutant forward_client.rs:518 `disconnect -> ()`: the handle would stay open until the transport is dropped.
        let (server, transport) = connected("disconnect_hangup");
        transport.disconnect();
        assert!(server_sees_hangup(&server));
        drop(transport);
    }

    #[test]
    #[ignore = "native: creates a uniquely named private test pipe"]
    fn dropping_a_connected_transport_closes_its_client_end() {
        // Mutant forward_client.rs:528 `Drop for WindowsForwardTransport -> ()`.
        let (server, transport) = connected("drop_hangup");
        drop(transport);
        assert!(server_sees_hangup(&server));
    }

    // ------------------------------------------------------------ P-11 vblank_driver.rs (real primary monitor)
    #[test]
    #[ignore = "native: requires a display adapter and the D3DKMT runtime"]
    fn vblank_chain_on_the_primary_monitor_blocks_for_real_vblanks_and_close_releases_the_adapter() {
        use crate::vblank_driver::{IVBlankDriver, NativeVBlankDriver};
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromWindow};

        let driver = NativeVBlankDriver::INSTANCE;
        // Mutants `monitor_from_window -> 1 / -1` (vblank_driver.rs:189): must be the handle Windows hands out.
        // SAFETY: a null HWND is the documented "no window" input; the API returns the nearest (primary) monitor.
        let expected = unsafe { MonitorFromWindow(HWND::default(), MONITOR_DEFAULTTONEAREST) };
        let monitor = driver.monitor_from_window(0);
        assert_eq!(monitor, expected.0 as isize);
        assert!(monitor != 0 && monitor != 1 && monitor != -1, "{monitor}");

        let (adapter, source) = driver
            .try_open(monitor)
            .expect("import ok")
            .expect("primary monitor opens");

        // Mutants `wait_for_vblank -> Ok(0) / Ok(1) / Ok(-1)` (:164). The status is whatever the kernel answers for a
        // real adapter handle: STATUS_SUCCESS after a real vertical blank (so four waits take at least three refresh
        // intervals, at least 4 ms even on a 360 Hz panel), or a graphics failure status (negative, session dependent;
        // this desktop answers 0xC01E0006 at once). A constant 1 or -1 is neither, and a constant 0 returns at once.
        let started = std::time::Instant::now();
        let mut statuses = Vec::new();
        for _ in 0..4 {
            statuses.push(driver.wait_for_vblank(adapter, source).expect("import ok"));
        }
        let waited = started.elapsed();
        assert!(
            statuses
                .iter()
                .all(|status| *status == 0 || (*status < 0 && *status != -1)),
            "{statuses:?}"
        );
        if statuses.iter().all(|status| *status == 0) {
            assert!(
                waited >= std::time::Duration::from_millis(4),
                "four vblank waits took {waited:?}"
            );
        }

        // Mutant `close -> ()` (:175): the kernel answers STATUS_INVALID_PARAMETER only for a handle it no longer knows.
        driver.close(adapter);
        const STATUS_INVALID_PARAMETER: i32 = 0xC000_000D_u32 as i32;
        assert_eq!(
            driver.wait_for_vblank(adapter, source).expect("import ok"),
            STATUS_INVALID_PARAMETER,
            "the adapter was released"
        );
    }

    #[test]
    #[ignore = "native: process handle count; run with --test-threads=1 --include-ignored"]
    fn dropping_servers_releases_their_stop_events() {
        // Mutant forward_server.rs:440 `Drop for NativeForwardPipeFactory -> ()` leaks one stop event per server.
        fn handles() -> u32 {
            let mut count = 0;
            // SAFETY: the pseudo-handle of the current process is always valid; `count` is a valid output slot.
            unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) }.expect("handle count");
            count
        }
        let warm = format!("PhotoReviewRs.test.q11.{}.warm", std::process::id());
        drop(InstanceForwardServer::new(warm, |_| {}, None, None, None).expect("server"));
        let before = handles();
        for index in 0..40 {
            let name = format!("PhotoReviewRs.test.q11.{}.drop{index}", std::process::id());
            drop(InstanceForwardServer::new(name, |_| {}, None, None, None).expect("server"));
        }
        let after = handles();
        assert!(after < before + 20, "handles grew from {before} to {after}");
    }
}
