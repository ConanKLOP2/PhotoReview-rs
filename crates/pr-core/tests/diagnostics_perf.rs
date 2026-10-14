//! W2-17: ports of `PhotoReviewPerfTests` and `PhotoReviewPerfEventTests` plus the golden `perf-path-id`
//! (C# `PhotoReview.Core.Tests/Diagnostics`, baseline d92d6aae).
//!
//! The listener registry is process-wide (like the C# EventSource, `[Collection("GlobalState")]`): every test that
//! attaches a listener holds `LOCK`, because `cargo test` runs tests of one binary on parallel threads.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_core::diagnostics::perf::{
    EVENT_TABLE, PerfEvent, PerfListener, PerfValue, PhotoReviewPerf, STOPWATCH_FREQUENCY, ShellPerfMarks, ms,
    ms_since_process_start, path_id, startup_mark, timestamp,
};
use pr_core::text::dotnet_path;
use pr_test_support::golden;

static LOCK: Mutex<()> = Mutex::new(());

fn global_state() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// C# `CaptureListener`: collects every delivered event.
#[derive(Default)]
struct Capture {
    events: Mutex<Vec<PerfEvent>>,
}

impl PerfListener for Capture {
    fn on_event_written(&self, event: &PerfEvent) {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event.clone());
    }
}

impl Capture {
    fn all(&self) -> Vec<PerfEvent> {
        self.events.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// `true` when the capture holds an event with exactly this id, name, payload names and payload; the id/name pair
/// must also be a row of [`EVENT_TABLE`] (the wire contract).
fn delivered(
    capture: &Capture,
    id: u32,
    name: &'static str,
    payload_names: &'static [&'static str],
    payload: Vec<PerfValue>,
) -> bool {
    assert!(EVENT_TABLE.contains(&(id, name)), "{id}:{name} is not in EVENT_TABLE");
    let expected = PerfEvent {
        id,
        name,
        payload_names,
        payload,
    };
    capture.all().contains(&expected)
}

fn diagnosis(capture: &Capture) -> String {
    format!(
        "enabled={}; captured: {:?}",
        PhotoReviewPerf::log().is_enabled(),
        capture.all()
    )
}

fn s(text: &str) -> PerfValue {
    PerfValue::Str(text.to_owned())
}

// 7_700_001 is a nav value no production code path uses, so concurrent unrelated events cannot satisfy an expectation.
const N: i64 = 7_700_001;

// C#: PhotoReviewPerfTests.PathId_StableAcrossCulturesAndCase (one run: Rust has no culture)
#[test]
fn path_id_stable_across_cultures_and_case() {
    let a = path_id(r"C:\Photos\Ilhan\img.jpg");
    let b = path_id(r"c:\PHOTOS\ILHAN\IMG.JPG");
    let c = path_id(r"C:\Photos\Ilhan\img2.jpg");

    assert_eq!(a.len(), 16);
    assert!(
        a.chars().all(|ch| ch.is_ascii_digit() || ('a'..='f').contains(&ch)),
        "{a}"
    );
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// C#: PhotoReviewPerfTests.PathId_RelativeEqualsFull
#[test]
fn path_id_relative_equals_full() {
    let full = dotnet_path::get_full_path("some.jpg").expect("relative path resolves");

    assert_eq!(path_id(&full), path_id("some.jpg"));
}

// DIVERGENCE (contract note 18): a path `get_full_path` rejects is hashed as given; C# throws ArgumentException.
#[test]
fn path_id_never_panics_on_paths_the_full_path_rejects() {
    assert_eq!(path_id("").len(), 16);
    assert_eq!(path_id("a\0b").len(), 16);
}

// Golden: perf-path-id (30 absolute paths: separators, `..`, UNC, `\\?\`, sharp s, dotted/dotless i, long s, ...).
#[test]
fn golden_perf_path_id_matches_csharp() {
    let golden = golden::load("perf-path-id");
    assert_eq!(golden.cases.len(), 30);
    for case in &golden.cases {
        let path = golden::str_of(case, "path");
        assert_eq!(path_id(path), golden::str_of(case, "id"), "path {path:?}");
    }
}

// C#: PhotoReviewPerfTests.EventIds_UniqueAndInformational
#[test]
fn event_ids_unique_and_informational() {
    assert_eq!(EVENT_TABLE.len(), 26);
    let mut ids: Vec<u32> = EVENT_TABLE.iter().map(|(id, _)| *id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), EVENT_TABLE.len(), "ids are unique");
    assert!(EVENT_TABLE.iter().all(|(id, _)| *id > 0));
    assert!(!ids.contains(&10), "id 10 never existed and stays unused");
    let mut names: Vec<&str> = EVENT_TABLE.iter().map(|(_, name)| *name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), EVENT_TABLE.len(), "names are unique");
}

// C#: PhotoReviewPerfTests.MsSinceProcessStart_Plausible_UnderContention
#[test]
fn ms_since_process_start_plausible_under_contention() {
    let mut values = [0.0f64; 64];
    std::thread::scope(|scope| {
        for slot in values.iter_mut() {
            scope.spawn(move || *slot = ms_since_process_start());
        }
    });

    // A torn cached start time would give ~6e13 ms (year 0001); the test host has been running for well under a day.
    for v in values {
        assert!((0.0..=86_400_000.0).contains(&v), "{v}");
    }
}

// C#: PhotoReviewPerfTests.Ms_NonNegative
#[test]
fn ms_non_negative() {
    let start = timestamp();

    assert!(ms(start) >= 0.0);
    assert!(ms(start - STOPWATCH_FREQUENCY) >= 999.0);
}

// C#: PhotoReviewPerfEventTests.Ms_ConvertsTicksToMilliseconds
#[test]
fn ms_converts_ticks_to_milliseconds() {
    let now = timestamp();
    assert!((0.0..=10_000.0).contains(&ms(now)));

    let half_second_ago = now - STOPWATCH_FREQUENCY / 2;
    assert!((499.0..=10_500.0).contains(&ms(half_second_ago)));

    let ten_seconds_ago = now - 10 * STOPWATCH_FREQUENCY;
    assert!((9_999.0..=20_000.0).contains(&ms(ten_seconds_ago)));
}

#[test]
fn timestamp_is_never_zero_and_never_goes_back() {
    let first = timestamp();
    let second = timestamp();

    assert!(first > 0);
    assert!(second >= first);
}

// C#: PhotoReviewPerfEventTests.InputAndLookupEvents
#[test]
fn input_and_lookup_events() {
    let _guard = global_state();
    let capture = Arc::new(Capture::default());
    let _subscription = PhotoReviewPerf::log().subscribe(capture.clone());
    let log = PhotoReviewPerf::log();

    log.key_input(N, "Right", 1.5);
    log.show_start(N, 3, "mode");
    log.stat(N, 2.5);
    log.lookup(N, "pid", "hit");
    log.thumb_start(N, "pid");
    log.thumb_end(N, "pid", "disk", 4.5);

    let d = |id, name, names, payload| delivered(&capture, id, name, names, payload);
    assert!(
        d(
            1,
            "KeyInput",
            &["nav", "key", "inputDelayMs"],
            vec![PerfValue::Int(N), s("Right"), PerfValue::Double(1.5)]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            2,
            "ShowStart",
            &["nav", "index", "mode"],
            vec![PerfValue::Int(N), PerfValue::Int(3), s("mode")]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            3,
            "Stat",
            &["nav", "ms"],
            vec![PerfValue::Int(N), PerfValue::Double(2.5)]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            4,
            "Lookup",
            &["nav", "pathId", "result"],
            vec![PerfValue::Int(N), s("pid"), s("hit")]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(5, "ThumbStart", &["nav", "pathId"], vec![PerfValue::Int(N), s("pid")]),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            6,
            "ThumbEnd",
            &["nav", "pathId", "source", "ms"],
            vec![PerfValue::Int(N), s("pid"), s("disk"), PerfValue::Double(4.5)]
        ),
        "{}",
        diagnosis(&capture)
    );
}

// C#: PhotoReviewPerfEventTests.LoadPipelineEvents
#[test]
fn load_pipeline_events() {
    let _guard = global_state();
    let capture = Arc::new(Capture::default());
    let _subscription = PhotoReviewPerf::log().subscribe(capture.clone());
    let log = PhotoReviewPerf::log();

    log.join_start(N, "pid");
    log.join_end(N, "pid", 5.5);
    log.disk_cache_read(N, "pid", 6.5, 1000);
    log.source_read(N, "pid", 7.5, 2000);
    log.decode(N, "pid", 8.5, 1920, true, false);
    log.verify(N, "pid", 9.5);

    let d = |id, name, names, payload| delivered(&capture, id, name, names, payload);
    assert!(
        d(7, "JoinStart", &["nav", "pathId"], vec![PerfValue::Int(N), s("pid")]),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            8,
            "JoinEnd",
            &["nav", "pathId", "ms"],
            vec![PerfValue::Int(N), s("pid"), PerfValue::Double(5.5)]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            9,
            "DiskCacheRead",
            &["nav", "pathId", "ms", "bytes"],
            vec![
                PerfValue::Int(N),
                s("pid"),
                PerfValue::Double(6.5),
                PerfValue::Int(1000)
            ]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            11,
            "SourceRead",
            &["nav", "pathId", "ms", "bytes"],
            vec![
                PerfValue::Int(N),
                s("pid"),
                PerfValue::Double(7.5),
                PerfValue::Int(2000)
            ]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            12,
            "Decode",
            &["nav", "pathId", "ms", "targetWidth", "downscaled", "fallback"],
            vec![
                PerfValue::Int(N),
                s("pid"),
                PerfValue::Double(8.5),
                PerfValue::Int(1920),
                PerfValue::Bool(true),
                PerfValue::Bool(false)
            ]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            13,
            "Verify",
            &["nav", "pathId", "ms"],
            vec![PerfValue::Int(N), s("pid"), PerfValue::Double(9.5)]
        ),
        "{}",
        diagnosis(&capture)
    );
}

// C#: PhotoReviewPerfEventTests.PresentationEvents
#[test]
fn presentation_events() {
    let _guard = global_state();
    let capture = Arc::new(Capture::default());
    let _subscription = PhotoReviewPerf::log().subscribe(capture.clone());
    let log = PhotoReviewPerf::log();

    log.assign(N, 10.5, 800, 600);
    log.rendered(N, 11.5);
    log.presented(N, "kind");
    log.post_start(N, "part");
    log.post_end(N, "part", 12.5);
    log.rendered_frame(N, 17.5);

    let d = |id, name, names, payload| delivered(&capture, id, name, names, payload);
    assert!(
        d(
            14,
            "Assign",
            &["nav", "ms", "pixelWidth", "pixelHeight"],
            vec![
                PerfValue::Int(N),
                PerfValue::Double(10.5),
                PerfValue::Int(800),
                PerfValue::Int(600)
            ]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            15,
            "Rendered",
            &["nav", "msSinceAssign"],
            vec![PerfValue::Int(N), PerfValue::Double(11.5)]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(16, "Presented", &["nav", "kind"], vec![PerfValue::Int(N), s("kind")]),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(17, "PostStart", &["nav", "part"], vec![PerfValue::Int(N), s("part")]),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            18,
            "PostEnd",
            &["nav", "part", "ms"],
            vec![PerfValue::Int(N), s("part"), PerfValue::Double(12.5)]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            25,
            "RenderedFrame",
            &["nav", "msSinceAssign"],
            vec![PerfValue::Int(N), PerfValue::Double(17.5)]
        ),
        "{}",
        diagnosis(&capture)
    );
}

// C#: PhotoReviewPerfEventTests.PreloadFolderAndStartupEvents
#[test]
fn preload_folder_and_startup_events() {
    let _guard = global_state();
    let capture = Arc::new(Capture::default());
    let _subscription = PhotoReviewPerf::log().subscribe(capture.clone());
    let log = PhotoReviewPerf::log();

    log.preload_item(4, "pid", 13.5, "kind", 14.5);
    log.preload_paused(85, 512);
    log.preload_cancel("why");
    log.dispatcher_long_op(15.5, "prio", "name");
    log.folder(N, "phase", 16.5);
    log.diag_mode("flags");
    log.startup("phase", 18.5);
    log.folder_info(N, "phase", 99, "detail");

    let d = |id, name, names, payload| delivered(&capture, id, name, names, payload);
    assert!(
        d(
            19,
            "PreloadItem",
            &["slot", "pathId", "queueWaitMs", "kind", "ms"],
            vec![
                PerfValue::Int(4),
                s("pid"),
                PerfValue::Double(13.5),
                s("kind"),
                PerfValue::Double(14.5)
            ]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            20,
            "PreloadPaused",
            &["loadPercent", "availableMb"],
            vec![PerfValue::Int(85), PerfValue::Int(512)]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(21, "PreloadCancel", &["reason"], vec![s("why")]),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            22,
            "DispatcherLongOp",
            &["ms", "priority", "name"],
            vec![PerfValue::Double(15.5), s("prio"), s("name")]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            23,
            "Folder",
            &["gen", "phase", "ms"],
            vec![PerfValue::Int(N), s("phase"), PerfValue::Double(16.5)]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(24, "DiagMode", &["flags"], vec![s("flags")]),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            26,
            "Startup",
            &["phase", "msSinceProcessStart"],
            vec![s("phase"), PerfValue::Double(18.5)]
        ),
        "{}",
        diagnosis(&capture)
    );
    assert!(
        d(
            27,
            "FolderInfo",
            &["gen", "phase", "value", "detail"],
            vec![PerfValue::Int(N), s("phase"), PerfValue::Int(99), s("detail")]
        ),
        "{}",
        diagnosis(&capture)
    );
}

// C#: PhotoReviewPerfEventTests.StartupMark_EmitsStartupEvent
#[test]
fn startup_mark_emits_startup_event() {
    let _guard = global_state();
    let capture = Arc::new(Capture::default());
    let _subscription = PhotoReviewPerf::log().subscribe(capture.clone());

    startup_mark("mt-mark-phase");

    let events = capture.all();
    let event = events
        .iter()
        .find(|e| e.id == 26 && e.payload.first() == Some(&s("mt-mark-phase")))
        .unwrap_or_else(|| panic!("no Startup event: {}", diagnosis(&capture)));
    match event.payload.get(1) {
        Some(PerfValue::Double(v)) => assert!((0.0..=86_400_000.0).contains(v), "{v}"),
        other => panic!("unexpected payload {other:?}"),
    }
}

// C#: IsEnabled() guard: with no listener an event method does nothing; a subscription's drop disables again.
#[test]
fn is_enabled_follows_the_subscriptions() {
    let _guard = global_state();
    let log = PhotoReviewPerf::log();
    assert!(!log.is_enabled());

    let capture = Arc::new(Capture::default());
    let first = log.subscribe(capture.clone());
    let second = log.subscribe(capture.clone());
    assert!(log.is_enabled());

    drop(first);
    assert!(log.is_enabled());
    log.stat(N, 1.0);
    assert_eq!(
        capture.all().len(),
        1,
        "the remaining subscription still receives events"
    );

    drop(second);
    assert!(!log.is_enabled());
    log.stat(N, 2.0);
    assert_eq!(capture.all().len(), 1, "no event after the last unsubscribe");
}

// DIVERGENCE (contract note 15): NavContext is a thread-local, not an AsyncLocal.
#[test]
fn nav_context_is_per_thread_and_defaults_to_zero() {
    assert_eq!(PhotoReviewPerf::nav_context(), 0);
    PhotoReviewPerf::set_nav_context(-1);
    assert_eq!(PhotoReviewPerf::nav_context(), -1);

    let other = std::thread::spawn(PhotoReviewPerf::nav_context)
        .join()
        .expect("thread joins");
    assert_eq!(other, 0);

    PhotoReviewPerf::set_nav_context(0);
}

#[test]
fn shell_perf_marks_keep_the_wpf_mark_names() {
    assert_eq!(ShellPerfMarks::D3D_DEVICE_READY, "d3dDeviceReady");
    assert_eq!(ShellPerfMarks::SWAP_CHAIN_READY, "swapChainReady");
    assert_eq!(ShellPerfMarks::FIRST_PRESENT, "firstPresent");
}
