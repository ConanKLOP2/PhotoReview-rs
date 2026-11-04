//! N4 (2026-11-05): a hang-proof killer and run-based equivalence proofs for `presenters/benchmark.rs`
//! (631:30 `+=` -> `*=`, 390:51 `>` -> `>=`, 832:14, 833:59, 833:17 twice). Fakes and a temp folder only; no sleep, no GUI, no
//! real Recycle Bin.

use std::sync::Arc;

use pr_app::presenters::benchmark::{
    BenchmarkFileScan, BenchmarkPresenter, BenchmarkProfileSession, BenchmarkScanError, BenchmarkSessionFactory,
    BenchmarkStep, BenchmarkSummarySave, BenchmarkView, ENUMERATION_CANCELLATION_CHECK_INTERVAL, enumerate_image_files,
};
use pr_app::presenters::settings_field_map::FieldContext;
use pr_bench::engine::{BenchmarkError, BenchmarkProgressSink};
use pr_bench::models::{BenchmarkProfile, BenchmarkReport};
use pr_bench::profiles;
use pr_core::sync::CancelToken;
use pr_l10n::localizer::built_in_english_localizer;
use pr_l10n::template::NumberCulture;
use pr_test_support::TempRoot;

#[derive(Default)]
struct View;

impl BenchmarkView for View {
    fn set_status(&mut self, _text: &str) {}
    fn set_progress(&mut self, _fraction: f64) {}
    fn set_running(&mut self, _running: bool) {}
    fn refresh_rows(&mut self) {}
    fn open_in_explorer(&mut self, _arguments: &str) {}
}

/// A session whose profile always fails: the run records a failed row and goes on to the next profile.
struct FailingSession;

impl BenchmarkProfileSession for FailingSession {
    fn run(
        &mut self,
        _folder: &str,
        _progress: &dyn BenchmarkProgressSink,
        _cancel: &CancelToken,
    ) -> Result<BenchmarkReport, BenchmarkError> {
        Err(BenchmarkError::Operation("scripted failure".into()))
    }
    fn dispose(&mut self) {}
}

/// benchmark.rs:631:30 (`run.next_profile += 1` -> `*= 1`): the index never moves and the run repeats profile 0 forever. A run of two
/// failing profiles ends in a bounded number of steps with one row per profile. With the mutant the step budget is exhausted and
/// the test FAILS (the existing tests loop until the harness timeout).
#[test]
fn a_two_profile_run_advances_to_the_second_profile_and_ends_within_a_step_budget() {
    let ids = ["fast-sequential", "fast-sequential"];
    let chosen: Vec<BenchmarkProfile> = ids
        .iter()
        .map(|id| profiles::find(id).expect("registered profile").clone())
        .collect();
    let factory: BenchmarkSessionFactory =
        Arc::new(|_profile, _files| Ok(Box::new(FailingSession) as Box<dyn BenchmarkProfileSession>));
    let scan: BenchmarkFileScan = Arc::new(|_folder, _limit, _cancel| Ok(vec!["a.jpg".to_owned()]));
    let save: BenchmarkSummarySave = Arc::new(|_reports| Ok("summary.json".to_owned()));
    let mut presenter = BenchmarkPresenter::new(factory, Some(scan), Some(save));
    let texts = built_in_english_localizer();
    let numbers = NumberCulture::invariant();
    let ctx = FieldContext {
        texts: &texts,
        numbers: &numbers,
    };
    let mut view = View;
    let mut step = presenter.run_selected(&chosen, "bench", Some(""), &mut view, &ctx);
    let mut profile_steps = 0;
    let mut budget = 40;
    while !matches!(step, BenchmarkStep::Idle) {
        assert!(
            budget > 0,
            "the run did not end within 40 steps (the profile index does not advance)"
        );
        budget -= 1;
        step = match step {
            BenchmarkStep::Idle => BenchmarkStep::Idle,
            BenchmarkStep::Scan(job) => presenter.on_scan_finished(job.run(), &mut view, &ctx),
            BenchmarkStep::Profile(job) => {
                profile_steps += 1;
                presenter.on_profile_finished(job.run(&|_message| {}), &mut view, &ctx)
            }
            BenchmarkStep::Save(job) => presenter.on_save_finished(job.run(), &mut view, &ctx),
        };
    }
    assert_eq!(profile_steps, 2, "each of the two profiles runs exactly once");
    assert_eq!(presenter.rows().len(), 2);
}

/// benchmark.rs:390:51 (`apply_image_limit`: `files.len() > limit` -> `>=`): at `len == limit` the prefix of `limit` files IS the
/// whole list, so both branches return the same copy. Proven for every list length 0..=40 and every limit -3..=45.
#[test]
fn applying_an_image_limit_equals_the_prefix_for_every_small_length_and_limit() {
    for len in 0..=40usize {
        let files: Vec<String> = (0..len).map(|i| format!("f{i}.jpg")).collect();
        for limit in -3..=45i32 {
            let got = BenchmarkPresenter::apply_image_limit(&files, limit);
            let want: Vec<String> = if limit > 0 && files.len() > limit as usize {
                files[..limit as usize].to_vec()
            } else {
                files.clone()
            };
            assert_eq!(got, want, "len {len} limit {limit}");
            // The independent statement of the C# rule: min(len, limit) files when the limit is positive, else all.
            let expected_len = if limit > 0 { len.min(limit as usize) } else { len };
            assert_eq!(got.len(), expected_len, "len {len} limit {limit}");
            assert_eq!(&got[..], &files[..expected_len]);
        }
    }
}

/// benchmark.rs:832:14 (`seen += 1` -> `*= 1`), 833:59 (`== 0` -> `!= 0`) and 833:17 (`%` -> `/`, `%` -> `+`): the four mutants change
/// only HOW OFTEN the token is polled inside the loop; the poll after the loop still decides. The returned value is the same for a
/// token that is canceled before the call and for one that is not, over folders of every size around one and two polling
/// intervals (0, 1, 255, 256, 257, 511, 512, 513 and 600 files, half of them not images, plus sub-folders that are not counted).
#[test]
fn enumerating_a_folder_returns_the_same_value_whatever_the_polling_frequency() {
    let interval = ENUMERATION_CANCELLATION_CHECK_INTERVAL;
    assert_eq!(interval, 256, "the sizes below are chosen around this interval");
    for count in [0usize, 1, 255, 256, 257, 511, 512, 513, 600] {
        let root = TempRoot::new("n4-enumerate");
        let folder = root.join("photos");
        std::fs::create_dir(&folder).expect("folder");
        std::fs::create_dir(folder.join("sub")).expect("sub-folder");
        let mut images = 0usize;
        for i in 0..count {
            let name = if i % 2 == 0 {
                images += 1;
                format!("p{i}.jpg")
            } else {
                format!("n{i}.txt")
            };
            std::fs::write(folder.join(name), b"x").expect("file");
        }
        let text = folder.to_string_lossy().into_owned();
        let listed = enumerate_image_files(&text, 0, &CancelToken::new()).expect("not canceled");
        assert_eq!(listed.len(), images, "count {count}");
        let limited = enumerate_image_files(&text, 3, &CancelToken::new()).expect("not canceled");
        assert_eq!(limited.len(), images.min(3), "count {count}");
        let canceled = CancelToken::new();
        canceled.cancel();
        assert_eq!(
            enumerate_image_files(&text, 0, &canceled),
            Err(BenchmarkScanError::Canceled),
            "count {count}"
        );
    }
}
