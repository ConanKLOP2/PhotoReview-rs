//! Slice S1 of unit W8-20b (`contracts/W8-20b.md`): the UI parity smoke named by the plan row, whole shell, hidden window, fakes. NEW tests (no C# twin; the C# whole-shell tests are in the other
//! `ui_smoke_*.rs` files). Each test starts `ShellStartup::run` through `ui_smoke_fixture::UiSmokeRun` over a temp folder of 3 to 6 copies of one fixture JPEG, drives it with real
//! window messages posted from the test thread, waits for conditions (never sleeps) and checks what the window reported after the frame.
//!
//! Native: every test is `#[ignore = "native: needs desktop"]` (a window, WARP, a message loop). The lead runs them once with `--include-ignored` on an interactive session before any row they
//! prove becomes `Done`. STUB: the bodies are `todo!()` (the red-test step writes them from the contract).
#![cfg(windows)]
#![allow(dead_code, unused_imports)] // stub

#[path = "common/ui_smoke_fixture.rs"]
mod ui_smoke_fixture;

use ui_smoke_fixture::{OpenTarget, SmokeConfig, UiSmokeRun, vk};

/// `K-25`/`C-18`/`W-01`: the command line opens the folder at the first photo; the first frame has the image; the title is the folder's.
#[test]
#[ignore = "native: needs desktop"]
fn smoke_open_folder_shows_the_first_image_and_the_folder_title() {
    todo!("W8-20b S1: contracts/W8-20b.md test 1")
}

/// `K-01`..`K-04`/`W-01`: Right, Left, End, Home move the index 0 -> 1 -> 0 -> last -> 0 and clamp at both ends (no wrap); each step shows that photo's path.
#[test]
#[ignore = "native: needs desktop"]
fn smoke_arrow_keys_home_and_end_navigate_and_clamp_at_the_ends() {
    todo!("W8-20b S1: contracts/W8-20b.md test 2")
}

/// `K-12`..`K-14`/`K-33`: `Add` zooms in one step, `Subtract` out, a wheel notch zooms at the cursor, `D1` shows 100 %, `F` returns to Fit (`is_fit` true).
#[test]
#[ignore = "native: needs desktop"]
fn smoke_plus_minus_and_wheel_zoom_and_f_returns_to_fit() {
    todo!("W8-20b S1: contracts/W8-20b.md test 3")
}

/// `K-08`: `Delete` (no confirmation by default) sends the current photo to the temp-folder bin, the file is gone from the folder, the view shows the next photo and the total drops by one.
#[test]
#[ignore = "native: needs desktop"]
fn smoke_delete_sends_the_image_to_the_bin_and_shows_the_next_one() {
    todo!("W8-20b S1: contracts/W8-20b.md test 4")
}

/// `K-10`: `Space` skips: the index advances and the session file lists the skipped path.
#[test]
#[ignore = "native: needs desktop"]
fn smoke_space_skips_to_the_next_image() {
    todo!("W8-20b S1: contracts/W8-20b.md test 5")
}

/// `K-22`/`W-01`: `Escape` closes the window; `ShellStartup::run` returns 0; the placement file was written.
#[test]
#[ignore = "native: needs desktop"]
fn smoke_escape_closes_the_window_and_the_run_returns_zero() {
    todo!("W8-20b S1: contracts/W8-20b.md test 6")
}
