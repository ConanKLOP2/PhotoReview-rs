//! Port of ShellStartupTests.cs startup-phase-one native tests (W8-17d).
#![cfg(windows)]
#[path = "common/w8_20_process_fixture.rs"]
mod fixture;
#[path = "common/ui_smoke_fixture.rs"]
mod rig;

// C#: ShellStartupTests.Startup_EarlyDecode_StartsOnlyAfterTheInstanceLockIsHeld.
#[test]
#[ignore = "native: startup window needs a desktop"]
fn startup_early_decode_starts_only_after_the_instance_lock_is_held() {
    rig::isolated_test(
        "startup_early_decode_starts_only_after_the_instance_lock_is_held",
        || {
            let run = rig::UiSmokeRun::start(rig::SmokeConfig::default()).unwrap();
            run.wait_first_image().unwrap();
            let marks = run.marks();
            let lock = marks.iter().position(|s| s == "instanceLock").expect("lock mark");
            let decode = marks
                .iter()
                .position(|s| s == "earlyDecodeStarted")
                .expect("early decode mark");
            assert!(lock < decode, "{marks:?}");
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

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
// Needs libraw.dll and turbojpeg.dll staged (tools/dev/stage-native.ps1): when a probe fails the app writes a FORCED error
// (AppLog.ErrorForced, as C# does), which legitimately creates app.log even with logging off. Not an INV-10 violation.
#[test]
#[ignore = "native: normal exe startup needs a desktop"]
fn startup_with_logging_off_writes_no_log_file() {
    let (_photos, a, _) = fixture::photos();
    let mut process = fixture::ShellProcess::start(&[a.to_string_lossy().into_owned()], None);
    process.wait_for("first frame", |p| p.count_events("RenderedFrame") >= 1);
    process.close();
    let log = process.root.path().join("data/logs/app.log");
    let content = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !log.exists(),
        "log off (INV-10): the exe wrote {}:\n{content}",
        log.display()
    );
}
