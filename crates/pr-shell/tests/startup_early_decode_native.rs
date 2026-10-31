//! Port of ShellStartupTests.Startup_EarlyDecode_StartsOnlyAfterTheInstanceLockIsHeld (X-startup-early-decode), in its own binary so
//! the native-nightly job can run it without `startup_native`, whose `startup_with_logging_off_writes_no_log_file` is red (see ST-EARLY-STJ).
//! `rig::isolated_test` re-executes this one test in a child process with a unique per-run instance prefix (named pipe / mutex),
//! `PHOTOREVIEW_RS_DATA_ROOT` + `PHOTOREVIEW_RS_ISOLATE_CONFIG=1` in a temp root, WARP and NOWARM. No fixed sleeps: the rig waits on a condvar.
#![cfg(windows)]
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
