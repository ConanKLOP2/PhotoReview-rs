//! Slice S1 of unit W8-20b (`contracts/W8-20b.md`): the UI parity smoke named by the plan row, whole shell, hidden window, fakes. NEW tests (no C# twin; the C# whole-shell tests are in the other
//! `ui_smoke_*.rs` files). Each test starts `ShellStartup::run` through `ui_smoke_fixture::UiSmokeRun` over a temp folder of 3 to 6 copies of one fixture JPEG, drives it with real
//! window messages posted from the test thread, waits for conditions (never sleeps) and checks what the window reported after the frame.
//!
//! Native: every test is `#[ignore = "native: needs desktop"]` (a window, WARP, a message loop). The lead runs them once with `--include-ignored` on an interactive session before any row they
//! prove becomes `Done`.
#![cfg(windows)]
use std::sync::Mutex;
static SERIAL: Mutex<()> = Mutex::new(());

#[path = "common/ui_smoke_fixture.rs"]
mod ui_smoke_fixture;

use ui_smoke_fixture::{OpenTarget, SmokeConfig, UiSmokeRun, isolated_test, vk};

/// `K-25`/`C-18`/`W-01`: the command line opens the folder at the first photo; the first frame has the image; the title is the folder's.
#[test]
#[ignore = "native: needs desktop"]
fn smoke_open_folder_shows_the_first_image_and_the_folder_title() {
    // new: no C# twin (plan row W8-20b)
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test("smoke_open_folder_shows_the_first_image_and_the_folder_title", || {
        let run = UiSmokeRun::start(SmokeConfig {
            photo_count: 4,
            open: OpenTarget::PhotosFolder,
            ..Default::default()
        })
        .unwrap();
        let snapshot = run.wait_first_image().unwrap();
        assert_eq!(snapshot.current_index, 0, "{}", run.diagnostics());
        assert_eq!(snapshot.total_files, 4);
        assert_name(&snapshot, "s00.jpg");
        assert!(
            snapshot.folder_title.to_lowercase().contains("photos"),
            "{}",
            run.diagnostics()
        );
        let marks = run.marks();
        let shown = marks.iter().position(|s| s == "windowShown").expect("windowShown mark");
        let present = marks
            .iter()
            .position(|s| s == "firstPresent")
            .expect("firstPresent mark");
        assert!(shown < present, "{}", run.diagnostics());
        assert_eq!(run.close_and_wait(), Ok(0));
    });
}

/// `K-01`..`K-04`/`W-01`: Right, Left, End, Home move the index 0 -> 1 -> 0 -> last -> 0 and clamp at both ends (no wrap); each step shows that photo's path.
#[test]
#[ignore = "native: needs desktop"]
fn smoke_arrow_keys_home_and_end_navigate_and_clamp_at_the_ends() {
    // new: no C# twin (plan row W8-20b)
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test("smoke_arrow_keys_home_and_end_navigate_and_clamp_at_the_ends", || {
        let run = UiSmokeRun::start(SmokeConfig {
            photo_count: 5,
            ..Default::default()
        })
        .unwrap();
        run.wait_first_image().unwrap();
        for (key, index) in [(vk::RIGHT, 1), (vk::RIGHT, 2), (vk::LEFT, 1), (vk::END, 4)] {
            run.key(key).unwrap();
            let snapshot = run.wait_until("navigation", |s| s.current_index == index).unwrap();
            assert_name(&snapshot, &format!("s{index:02}.jpg"));
        }
        run.key(vk::RIGHT).unwrap();
        assert_eq!(
            run.on_ui(|p| p.view_model.borrow().current_index()).unwrap(),
            4,
            "{}",
            run.diagnostics()
        );
        run.key(vk::HOME).unwrap();
        run.wait_until("Home", |s| s.current_index == 0).unwrap();
        run.key(vk::LEFT).unwrap();
        assert_eq!(
            run.on_ui(|p| p.view_model.borrow().current_index()).unwrap(),
            0,
            "{}",
            run.diagnostics()
        );
        assert_eq!(run.close_and_wait(), Ok(0));
    });
}

/// `K-12`..`K-14`/`K-33`: `Add` zooms in one step, `Subtract` out, a wheel notch zooms at the cursor, `D1` shows 100 %, `F` returns to Fit (`is_fit` true).
#[test]
#[ignore = "native: needs desktop"]
fn smoke_plus_minus_and_wheel_zoom_and_f_returns_to_fit() {
    // new: no C# twin (plan row W8-20b)
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test("smoke_plus_minus_and_wheel_zoom_and_f_returns_to_fit", || {
        let run = UiSmokeRun::start(SmokeConfig {
            fixture: "parity-640x480.jpg",
            ..Default::default()
        })
        .unwrap();
        let first = run.wait_first_image().unwrap();
        let fit = first.zoom_percent;
        assert!(first.is_fit);
        run.key(vk::ADD).unwrap();
        let zoom = run
            .wait_until("zoom in", |s| !s.is_fit && s.zoom_percent > fit)
            .unwrap()
            .zoom_percent;
        run.key(vk::SUBTRACT).unwrap();
        run.wait_until("zoom out", |s| s.zoom_percent < zoom).unwrap();
        run.key(vk::D1).unwrap();
        run.wait_until("100 percent", |s| (s.zoom_percent - 100.0).abs() < 0.5)
            .unwrap();
        run.wheel(120).unwrap();
        run.wait_until("wheel", |s| s.zoom_percent > 100.0).unwrap();
        run.key(vk::F).unwrap();
        run.wait_until("Fit", |s| s.is_fit && (s.zoom_percent - fit).abs() < 0.5)
            .unwrap();
        assert_eq!(run.close_and_wait(), Ok(0));
    });
}

/// `K-08`: `Delete` (no confirmation by default) sends the current photo to the temp-folder bin, the file is gone from the folder, the view shows the next photo and the total drops by one.
#[test]
#[ignore = "native: needs desktop"]
fn smoke_delete_sends_the_image_to_the_bin_and_shows_the_next_one() {
    // new: no C# twin (plan row W8-20b)
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test("smoke_delete_sends_the_image_to_the_bin_and_shows_the_next_one", || {
        let run = UiSmokeRun::start(SmokeConfig {
            photo_count: 4,
            ..Default::default()
        })
        .unwrap();
        run.wait_first_image().unwrap();
        run.key(vk::RIGHT).unwrap();
        run.wait_until("second photo", |s| s.current_index == 1).unwrap();
        run.key(vk::DELETE).unwrap();
        run.wait_until("recycle", |s| s.total_files == 3 && has_name(s, "s02.jpg"))
            .unwrap();
        // The view changes first; the file moves on a worker: wait for the bin instead of reading it at once.
        let sent = run.bin().wait_for_sent(1).unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].file_name().unwrap(), "s01.jpg");
        assert!(!run.photos()[1].exists());
        assert!(run.root().join("bin/0-s01.jpg").exists());
        for index in [0, 2, 3] {
            assert!(run.photos()[index].exists());
        }
        assert_eq!(run.close_and_wait(), Ok(0));
    });
}

/// `K-10`: `Space` skips: the index advances and the session file lists the skipped path.
#[test]
#[ignore = "native: needs desktop"]
fn smoke_space_skips_to_the_next_image() {
    // new: no C# twin (plan row W8-20b)
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test("smoke_space_skips_to_the_next_image", || {
        let run = UiSmokeRun::start(SmokeConfig::default()).unwrap();
        run.wait_first_image().unwrap();
        run.key(vk::SPACE).unwrap();
        run.wait_until("skip", |s| s.current_index == 1).unwrap();
        assert_eq!(run.close_and_wait(), Ok(0));
        let sessions = std::fs::read_dir(run.root().join("data/Sessions"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect::<Vec<_>>();
        assert_eq!(sessions.len(), 1);
        let json = std::fs::read_to_string(&sessions[0]).unwrap();
        assert!(json.contains("\"Skipped\"") && json.contains("s00.jpg"), "{json}");
    });
}

/// `K-22`/`W-01`: `Escape` closes the window; `ShellStartup::run` returns 0; the placement file was written.
#[test]
#[ignore = "native: needs desktop"]
fn smoke_escape_closes_the_window_and_the_run_returns_zero() {
    // new: no C# twin (plan row W8-20b)
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test("smoke_escape_closes_the_window_and_the_run_returns_zero", || {
        let run = UiSmokeRun::start(SmokeConfig::default()).unwrap();
        run.wait_first_image().unwrap();
        run.key(vk::ESCAPE).unwrap();
        assert_eq!(run.wait_exit(), Ok(0));
        assert!(run.root().join("data/window-placement.json").is_file());
        assert!(run.marks().iter().any(|m| m == "windowShown"));
    });
}

fn has_name(snapshot: &ui_smoke_fixture::UiSnapshot, expected: &str) -> bool {
    snapshot
        .current_path
        .as_deref()
        .and_then(|p| std::path::Path::new(p).file_name())
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(expected))
}
fn assert_name(snapshot: &ui_smoke_fixture::UiSnapshot, expected: &str) {
    assert!(has_name(snapshot, expected), "expected {expected}: {snapshot:?}");
}
