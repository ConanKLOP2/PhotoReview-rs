//! Native evidence for P-04 (named-mutex ownership) and P-05 (named-pipe forwarding), run by hand on a desktop:
//! `cargo test -p pr-platform-win --locked --test w6_platform_native -- --include-ignored --test-threads=1`.
//! Names carry a unique prefix, so nothing here can collide with a running PhotoReviewRs; no Recycle Bin is touched.

#![cfg(windows)]

#[allow(dead_code)]
#[path = "../src/vblank_driver.rs"]
mod vblank_driver;

use std::sync::mpsc;
use std::time::Duration;

use pr_core::instance::forwarding::{ForwardOutcome, InstanceForwardClient};
use pr_core::model::InstanceMode;
use pr_core::sync::CancelToken;
use pr_platform_win::instance_scope::{InstanceClaimResult, InstanceScope};
use pr_test_support::TempRoot;
use vblank_driver::{IVBlankDriver, NativeVBlankDriver};

fn unique_prefix(tag: &str) -> String {
    format!("PhotoReviewRsW6{tag}{}", std::process::id())
}

fn scope(mode: InstanceMode, prefix: &str, sender: Option<mpsc::Sender<Vec<String>>>) -> InstanceScope {
    InstanceScope::with_options(
        mode,
        move |paths| {
            if let Some(sender) = &sender {
                let _ = sender.send(paths.to_vec());
            }
        },
        None,
        prefix,
        None,
        false,
    )
    .expect("scope options are valid")
}

// C# twin: InstanceScopeTests (mutex ownership) over the real kernel mutex: the second owner of the same
// folder key is refused, a different folder is independent, and disposing the owner frees the name.
#[test]
#[ignore = "native: creates a uniquely named kernel mutex and named pipe"]
fn real_named_mutex_gives_one_owner_per_folder_and_frees_on_dispose() {
    let prefix = unique_prefix("Own");
    let folder_a = r"C:\w6-native-a";
    let folder_b = r"C:\w6-native-b";
    let first = scope(InstanceMode::PerFolder, &prefix, None);
    let second = scope(InstanceMode::PerFolder, &prefix, None);

    assert_eq!(first.claim(Some(folder_a)).unwrap(), InstanceClaimResult::Acquired);
    assert_eq!(first.claim(Some(folder_a)).unwrap(), InstanceClaimResult::AlreadyHeld);
    assert!(first.holds(Some(folder_a)).unwrap());
    assert_eq!(
        second.claim(Some(folder_a)).unwrap(),
        InstanceClaimResult::OwnedElsewhere
    );
    assert!(!second.try_acquire(Some(folder_a)).unwrap());
    // A spelling variant of the same folder is the same key.
    assert_eq!(
        second.claim(Some(r"c:\W6-NATIVE-A\")).unwrap(),
        InstanceClaimResult::OwnedElsewhere
    );
    assert_eq!(second.claim(Some(folder_b)).unwrap(), InstanceClaimResult::Acquired);

    first.dispose();
    assert_eq!(second.claim(Some(folder_a)).unwrap(), InstanceClaimResult::Acquired);
    second.dispose();
}

// C# twin: InstanceForwardPipeTests (forwarding round trip) over the real pipe: a second launch hands a
// path to the owner, which receives it; a refused path is Rejected; after dispose nobody answers.
#[test]
#[ignore = "native: creates a uniquely named kernel mutex and named pipe"]
fn real_pipe_forwards_a_path_to_the_owner_and_reports_no_instance_after_dispose() {
    let prefix = unique_prefix("Fwd");
    let temp = TempRoot::new("w6-native-forward");
    let folder = temp.path().to_string_lossy().into_owned();
    let file = temp.path().join("forwarded.jpg");
    std::fs::write(&file, b"x").expect("create the forwarded file");
    let (sender, receiver) = mpsc::channel();
    let owner = scope(InstanceMode::PerFolder, &prefix, Some(sender));
    assert_eq!(owner.claim(Some(&folder)).unwrap(), InstanceClaimResult::Acquired);

    let client = scope(InstanceMode::PerFolder, &prefix, None)
        .create_client(Some(&folder))
        .expect("client for the owner pipe");
    let path = file.to_string_lossy().into_owned();
    let outcome = send(client.as_ref(), std::slice::from_ref(&path));
    assert_eq!(outcome, ForwardOutcome::Delivered);
    let received = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("the owner callback receives the forwarded path");
    assert_eq!(received, vec![path]);

    let missing = temp.path().join("missing.jpg").to_string_lossy().into_owned();
    assert_eq!(send(client.as_ref(), &[missing]), ForwardOutcome::Rejected);

    owner.dispose();
    assert_eq!(
        send(client.as_ref(), &[file.to_string_lossy().into_owned()]),
        ForwardOutcome::NoInstance
    );
}

fn send(client: &dyn InstanceForwardClient, paths: &[String]) -> ForwardOutcome {
    client
        .send(paths, Duration::from_secs(3), &CancelToken::none())
        .expect("send returns an outcome")
}

// P-11 native evidence and killers for the FFI-struct survivors of the vblank_driver mutation run
// (`delete field size from struct DwmTimingInfo / MonitorInfoExW`, `delete field hdc from struct OpenAdapterFromHdc`):
// on a desktop with a WDDM adapter the real chain must produce a positive grid and an adapter, and any wrong
// struct size or missing HDC makes the corresponding system call fail. C# twin: DisplayClockTests
// (DwmTiming_Static_ReturnsNullOrAPositiveGrid) plus the D3DKMT open path of WindowsDisplayClock.
#[test]
#[ignore = "native: requires a Windows desktop with DWM composition and a WDDM display adapter"]
fn real_dwm_timing_and_display_adapter_chain_succeeds_on_the_primary_monitor() {
    let driver = NativeVBlankDriver::INSTANCE;
    let timing = driver
        .dwm_timing()
        .expect("DWM reports a timing grid on a composed desktop");
    assert!(timing.refresh_period > 0 && timing.last_vblank > 0);

    let monitor = driver.monitor_from_window(0);
    assert_ne!(monitor, 0, "MONITOR_DEFAULTTONEAREST always yields a monitor");
    let (adapter, source) = driver
        .try_open(monitor)
        .expect("D3DKMT imports resolve")
        .expect("the primary monitor opens a D3DKMT adapter");
    assert_ne!(adapter, 0);
    let _ = source;
    driver.close(adapter);
}
