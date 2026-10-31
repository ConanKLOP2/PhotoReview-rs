//! Lane st-dialog-scenarios: C# `BenchmarkDialogTests` scenarios that had no Rust twin yet (QuickCheck, a second run clears the old rows,
//! header sort ascending then descending, sorting keeps the selected row). Fake sessions and a fake scan: no codec, no disk benchmark.
#![cfg(windows)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;

use fixture::{activate_visual_styles, command, create_owner, destroy_owner, item, on_sta};
use pr_app::presenters::benchmark::{
    BenchmarkProfileSession, BenchmarkSessionFactory, QUICK_CHECK_ITERATIONS, QUICK_CHECK_PROFILE_ID,
};
use pr_bench::engine::{BenchmarkError, BenchmarkProgressSink};
use pr_bench::models::{
    BenchmarkPhaseResult, BenchmarkProfile, BenchmarkReport, BenchmarkResultStatus, BenchmarkTimestamp,
};
use pr_core::sync::CancelToken;
use pr_platform_win::dialogs::benchmark::layout::{
    PROFILES_ID, QUICK_CHECK_ID, RESULTS_ID, RUN_ID, SELECT_ALL_ID, SELECT_NONE_ID,
};
use pr_platform_win::dialogs::benchmark::{BenchmarkDialog, BenchmarkOptions};
use pr_platform_win::dialogs::native::controls::basic::ListBoxControl;
use pr_platform_win::dialogs::native::controls::list_tree::ListViewControl;

type Seen = Arc<Mutex<Vec<(String, i32)>>>;

struct FakeSession {
    profile: BenchmarkProfile,
    gate: Option<Arc<AtomicBool>>,
}

impl BenchmarkProfileSession for FakeSession {
    fn run(
        &mut self,
        folder: &str,
        _: &dyn BenchmarkProgressSink,
        _: &CancelToken,
    ) -> Result<BenchmarkReport, BenchmarkError> {
        if let Some(gate) = &self.gate {
            let deadline = Instant::now() + Duration::from_secs(20);
            while !gate.load(Ordering::Acquire) {
                if Instant::now() >= deadline {
                    return Err(BenchmarkError::Operation("test did not release the session".into()));
                }
                std::thread::yield_now();
            }
        }
        // A sample that differs per profile, so the P95 column has distinct values.
        let base = f64::from(self.profile.id.bytes().map(u32::from).sum::<u32>() % 997) + 1.0;
        Ok(BenchmarkReport {
            run_id: "fake".to_owned(),
            started_utc: BenchmarkTimestamp::unix_epoch(),
            folder: folder.to_owned(),
            phases: vec![BenchmarkPhaseResult::new(
                self.profile.id.clone(),
                self.profile.workload,
                vec![base, base * 2.0, base * 3.0],
                BenchmarkResultStatus::Pass,
                None,
                None,
            )],
            metrics: None,
            machine: None,
        })
    }

    fn dispose(&mut self) {}
}

fn dialog(seen: Seen, gate: Option<Arc<AtomicBool>>) -> BenchmarkDialog {
    let factory: BenchmarkSessionFactory = Arc::new(move |profile, _files| {
        seen.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((profile.id.clone(), profile.iterations));
        Ok(Box::new(FakeSession {
            profile: profile.clone(),
            gate: gate.clone(),
        }))
    });
    BenchmarkDialog::new(
        Some("fake-folder"),
        factory,
        BenchmarkOptions {
            pictures_folder: Some("fake-pictures".to_owned()),
            scan: Some(Arc::new(|_, _, _| Ok(vec!["fake.jpg".to_owned()]))),
            save: Some(Arc::new(|_| Ok("summary.json".to_owned()))),
            ..Default::default()
        },
    )
}

fn wait_idle(d: &BenchmarkDialog) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while d.presenter().borrow().is_running() {
        assert!(Instant::now() < deadline, "benchmark run did not complete");
        fixture::drain(d.native());
        std::thread::yield_now();
    }
    fixture::drain(d.native());
}

fn column(d: &BenchmarkDialog, column: i32) -> Vec<String> {
    let grid = item(d.native(), RESULTS_ID);
    (0..ListViewControl::count(grid))
        .map(|row| ListViewControl::cell_text(grid, row, column))
        .collect()
}

fn digits(text: &str) -> f64 {
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(-1.0)
}

// C#: BenchmarkDialogTests.QuickCheck_RunsTheFixedProfile_WhateverIsSelected
#[test]
#[ignore = "native: needs desktop"]
fn quick_check_runs_the_fixed_profile_whatever_is_selected() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let seen: Seen = Arc::default();
        let owner = create_owner();
        let d = dialog(seen.clone(), None);
        d.native()
            .show_modeless(owner, None, None, None)
            .expect("show benchmark");
        command(d.native(), SELECT_ALL_ID, 0);
        command(d.native(), QUICK_CHECK_ID, 0);
        wait_idle(&d);
        assert_eq!(
            *seen.lock().unwrap(),
            [(QUICK_CHECK_PROFILE_ID.to_owned(), QUICK_CHECK_ITERATIONS)]
        );
        d.native().close();
        destroy_owner(owner);
    });
}

// C#: BenchmarkDialogTests.ASecondRun_ClearsTheOldRows_BeforeItsFirstResult
#[test]
#[ignore = "native: needs desktop"]
fn a_second_run_clears_the_old_rows_before_its_first_result() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let seen: Seen = Arc::default();
        let gate = Arc::new(AtomicBool::new(true));
        let owner = create_owner();
        let d = dialog(seen.clone(), Some(gate.clone()));
        d.native()
            .show_modeless(owner, None, None, None)
            .expect("show benchmark");
        command(d.native(), SELECT_ALL_ID, 0);
        command(d.native(), RUN_ID, 0);
        wait_idle(&d);
        let first_rows = ListViewControl::count(item(d.native(), RESULTS_ID));
        assert!(first_rows >= 2, "select all must run several profiles");

        gate.store(false, Ordering::Release); // the next session blocks until released
        command(d.native(), SELECT_NONE_ID, 0);
        ListBoxControl::set_selected(item(d.native(), PROFILES_ID), 0, true);
        command(d.native(), RUN_ID, 0);
        let deadline = Instant::now() + Duration::from_secs(20);
        while seen.lock().unwrap().len() as i32 == first_rows {
            assert!(Instant::now() < deadline, "the second session did not start");
            fixture::drain(d.native());
            std::thread::yield_now();
        }
        assert_eq!(
            ListViewControl::count(item(d.native(), RESULTS_ID)),
            0,
            "old rows are cleared before the first new result"
        );
        gate.store(true, Ordering::Release);
        wait_idle(&d);
        assert_eq!(ListViewControl::count(item(d.native(), RESULTS_ID)), 1);
        d.native().close();
        destroy_owner(owner);
    });
}

// C#: BenchmarkDialogTests.ClickingAColumnHeader_SortsAscendingThenDescending_NumericColumnsByValue / SortingKeepsTheSelectedRow
#[test]
#[ignore = "native: needs desktop"]
fn header_sort_is_ascending_then_descending_by_value_and_keeps_the_selected_row() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let owner = create_owner();
        let d = dialog(Arc::default(), None);
        d.native()
            .show_modeless(owner, None, None, None)
            .expect("show benchmark");
        command(d.native(), SELECT_ALL_ID, 0);
        command(d.native(), RUN_ID, 0);
        wait_idle(&d);

        d.sort_by(6); // P95
        let ascending: Vec<f64> = column(&d, 6).iter().map(|t| digits(t)).collect();
        d.sort_by(6);
        let descending: Vec<f64> = column(&d, 6).iter().map(|t| digits(t)).collect();
        let mut sorted = ascending.clone();
        sorted.sort_by(f64::total_cmp);
        assert_eq!(ascending, sorted);
        sorted.reverse();
        assert_eq!(descending, sorted);
        assert_eq!(d.sort(), (6, 2));

        d.sort_by(1);
        assert_eq!(d.sort(), (1, 1));
        let by_profile = column(&d, 1);
        let mut expected = by_profile.clone();
        expected.sort_by(|a, b| pr_platform_win::dialogs::benchmark::state::culture_compare(a, b));
        assert_eq!(by_profile, expected);

        let grid = item(d.native(), RESULTS_ID);
        ListViewControl::select_row(grid, 0);
        let name = ListViewControl::cell_text(grid, 0, 1);
        d.sort_by(1); // descending: the first row moves
        let row = ListViewControl::selected_row(grid);
        assert_eq!(ListViewControl::cell_text(grid, row, 1), name);
        d.native().close();
        destroy_owner(owner);
    });
}
