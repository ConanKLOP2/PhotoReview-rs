//! Port of ShellStartupTests.cs startup-phase-one native tests (W8-17d).
#![cfg(windows)]
#[path = "common/w8_20_process_fixture.rs"]
mod fixture;

// Startup_EarlyDecode_StartsOnlyAfterTheInstanceLockIsHeld moved to startup_early_decode_native.rs (own binary, in native-nightly).

// C#: ShellStartupTests.SecondInstance_ForwardsItsPathToTheRunningOneAndExitsZero.
#[test]
#[ignore = "native: forwarding to a real exe window needs a desktop"]
fn second_instance_forwards_its_path_to_the_running_one_and_exits_zero() {
    let (_photos, a, b) = fixture::photos();
    let mut first = fixture::ShellProcess::start(&[a.to_string_lossy().into_owned()], None);
    first.wait_for("first assignment", |p| p.count_events("Assign") == 1);
    let mut second = fixture::ShellProcess::start(&[b.to_string_lossy().into_owned()], Some(&first.prefix));
    assert_eq!(second.wait_exit().code(), Some(0));
    first.wait_for("forwarded path", |p| p.count_events("Assign") == 2);
    first.close();
}

// C#: ShellStartupTests.Startup_WithLoggingOff_WritesNoLogFile.
#[test]
#[ignore = "native: normal exe startup needs a desktop"]
fn startup_with_logging_off_writes_no_log_file() {
    let (_photos, a, _) = fixture::photos();
    let mut process = fixture::ShellProcess::start(&[a.to_string_lossy().into_owned()], None);
    process.wait_for("first frame", |p| p.count_events("RenderedFrame") >= 1);
    process.close();
    assert!(!process.root.path().join("data/logs/app.log").exists());
}
