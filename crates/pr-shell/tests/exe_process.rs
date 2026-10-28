//! Port of ShellSmokeTests.cs and Startup/ShellProcessTests.cs (contracts/W8-20a.md).
#![cfg(windows)]
#[path = "common/w8_20_process_fixture.rs"]
mod fixture;
use fixture::{ShellProcess, photos};

// C#: ShellSmokeTests.Exe_WithSmokeArgument_ShowsWindowAndExitsZero.
#[test]
#[ignore = "native: smoke presents a frame and needs a desktop"]
fn exe_with_smoke_argument_shows_window_and_exits_zero() {
    let mut process = ShellProcess::start(&["--smoke".into()], None);
    assert_eq!(process.wait_exit().code(), Some(0));
    assert!(!process.root.path().join("data/window-placement.json").exists());
    assert!(!process.root.path().join("data/logs/app.log").exists());
}

// C#: ShellProcessTests.Exe_WithJpeg_DrawsTheFirstImageAndExitsZeroOnClose.
#[test]
#[ignore = "native: real exe window and WARP rendering need a desktop"]
fn exe_with_a_jpeg_draws_the_first_image_and_exits_zero_on_close() {
    let (_photos, a, _) = photos();
    let mut process = ShellProcess::start(&[a.to_string_lossy().into_owned()], None);
    process.wait_for("first rendered image", |p| {
        p.count_events("RenderedFrame") >= 1 && p.count_events("Assign") == 1
    });
    // new: live twin of ShellMenuRenderingTests.MainWindow_HasTheAppIcon_BigAndSmall.
    assert_eq!(
        pr_platform_win::ui_driver::main_window_icons_of_process(process.pid),
        Some((true, true))
    );
    process.close();
    assert!(!process.root.path().join("data/logs/app.log").exists());
    assert!(process.root.path().join("data/window-placement.json").exists());
}

// C#: ShellProcessTests.SecondExe_ForwardsItsFileToTheRunningOneAndExitsZero.
#[test]
#[ignore = "native: two real exe instances need a desktop"]
fn second_exe_forwards_its_file_to_the_running_one_and_exits_zero() {
    let (_photos, a, b) = photos();
    let mut first = ShellProcess::start(&[a.to_string_lossy().into_owned()], None);
    first.wait_for("first assignment", |p| p.count_events("Assign") == 1);
    let mut second = ShellProcess::start(&[b.to_string_lossy().into_owned()], Some(&first.prefix));
    assert_ne!(first.root.path(), second.root.path());
    assert_eq!(second.wait_exit().code(), Some(0));
    first.wait_for("forwarded assignment", |p| p.count_events("Assign") == 2);
    first.close();
}
