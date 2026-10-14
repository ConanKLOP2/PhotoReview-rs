//! Tests for `pr_perf::nav` (unit W1-19), port of `PhotoReview.PerfAnalysis/PerfAnalyzeNav.cs`.
//!
//! Ported C# tests (each carries its `// C#:` twin): `PerfAnalyzeTests` (navigation, folder, startup, preload and
//! dispatcher parts), `PerfAnalyzeHardeningTests` (PA-01), `PerfAnalyzeKeyInputMatchTests` (brute-force agreement) and
//! `PerfAnalyzeBuilderBenchmark` (manual). The `perf-nav` golden loop checks every case of `tests/golden/perf-nav.json`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use pr_perf::csv::{DiagFlags, PerfCsvFile, PerfRow, parse_text};
use pr_perf::nav::{KeyInputIndex, NavRecord, build, classify_kind};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

// ---------------------------------------------------------------- helpers

/// A hand-built row: `nav` text, `a` text and `text`; `b`, `c`, `d` empty; path `p` (the C# `PerfRow` ctor shape).
fn row(utc: i64, qpc: i64, thread: i32, event: &str, nav: &str, a: &str, text: &str) -> PerfRow {
    PerfRow {
        utc_ticks: utc,
        qpc_ticks: qpc,
        thread,
        event: event.to_owned(),
        nav: nav.to_owned(),
        path_id: "p".to_owned(),
        a: a.to_owned(),
        text: text.to_owned(),
        ..PerfRow::default()
    }
}

/// The `PerfRow` built by `PerfAnalyzeHardeningTests.R(t, ev, a, text)`: thread 1, nav `7`.
fn hrow(t: i64, event: &str, a: &str, text: &str) -> PerfRow {
    row(t, t, 1, event, "7", a, text)
}

fn file_of(rows: Vec<PerfRow>, qpc_frequency: i64) -> PerfCsvFile {
    PerfCsvFile {
        path: PathBuf::from("perf-x.csv"),
        diag_flags: DiagFlags::new(),
        qpc_frequency,
        dropped_rows: 0,
        rows,
    }
}

/// The text of the golden build case `name` (`content`).
fn golden_build_content(name: &str) -> String {
    let golden = golden::load("perf-nav");
    let case = golden
        .cases
        .iter()
        .find(|c| c["kind"].as_str() == Some("build") && c["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("no perf-nav build case '{name}'"));
    str_of(case, "content").to_owned()
}

/// `tools/diag/samples/perf-sample.csv` as the golden `sample` case carries it.
fn sample_file() -> PerfCsvFile {
    parse_text(Path::new("perf-sample.csv"), &golden_build_content("sample"))
}

/// C# `Assert.Equal(expected, actual, 3)`: equal after rounding to three decimals (checked as a 0.0005 window).
fn assert_close3(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 0.0005,
        "{what}: {actual} != {expected} (3 decimals)"
    );
}

fn some_close3(actual: Option<f64>, expected: f64, what: &str) {
    match actual {
        Some(a) => assert_close3(a, expected, what),
        None => panic!("{what}: None, expected {expected}"),
    }
}

fn opt_f64(v: &Value) -> Option<f64> {
    if v.is_null() {
        None
    } else {
        Some(
            v.as_f64()
                .unwrap_or_else(|| panic!("golden value is not a number: {v}")),
        )
    }
}

fn f64_of(v: &Value, key: &str) -> f64 {
    v[key]
        .as_f64()
        .unwrap_or_else(|| panic!("case has no number '{key}': {v}"))
}

fn arr<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_else(|| panic!("case has no array '{key}': {v}"))
}

/// `[[key, value], ...]` as the golden writes `post_ms` and `startup`.
fn pairs_f64(v: &Value) -> Vec<(String, f64)> {
    v.as_array()
        .unwrap_or_else(|| panic!("expected pair array: {v}"))
        .iter()
        .map(|p| {
            let key = p[0].as_str().unwrap_or_else(|| panic!("pair key: {p}")).to_owned();
            let value = p[1].as_f64().unwrap_or_else(|| panic!("pair value: {p}"));
            (key, value)
        })
        .collect()
}

fn post_ms_pairs(nav: &NavRecord) -> Vec<(String, f64)> {
    nav.post_ms.iter().map(|(k, v)| (k.clone(), *v)).collect()
}

fn cases_of_kind<'a>(cases: &'a [Value], kind: &str) -> Vec<&'a Value> {
    cases.iter().filter(|c| c["kind"].as_str() == Some(kind)).collect()
}

fn assert_nav(actual: &NavRecord, expected: &Value, case: &str) {
    let id = i64_of(expected, "nav");
    let ctx = format!("{case}: nav {id}");
    assert_eq!(actual.nav, id, "{ctx}: nav id");
    assert_eq!(actual.mode, str_of(expected, "mode"), "{ctx}: mode");
    assert_eq!(actual.t_input_ms, opt_f64(&expected["t_input"]), "{ctx}: t_input");
    assert_eq!(actual.t_pre_ms, opt_f64(&expected["t_pre"]), "{ctx}: t_pre");
    assert_eq!(actual.t_stat_ms, opt_f64(&expected["t_stat"]), "{ctx}: t_stat");
    assert_eq!(
        actual.lookup_result,
        str_of(expected, "lookup_result"),
        "{ctx}: lookup_result"
    );
    assert_eq!(actual.t_thumb_ms, opt_f64(&expected["t_thumb"]), "{ctx}: t_thumb");
    assert_eq!(
        actual.has_thumbnail,
        bool_of(expected, "has_thumbnail"),
        "{ctx}: has_thumbnail"
    );
    assert_eq!(actual.t_join_ms, opt_f64(&expected["t_join"]), "{ctx}: t_join");
    assert_eq!(actual.t_disk_ms, opt_f64(&expected["t_disk"]), "{ctx}: t_disk");
    assert_eq!(actual.t_read_ms, opt_f64(&expected["t_read"]), "{ctx}: t_read");
    assert_eq!(actual.t_decode_ms, opt_f64(&expected["t_decode"]), "{ctx}: t_decode");
    assert_eq!(actual.t_verify_ms, opt_f64(&expected["t_verify"]), "{ctx}: t_verify");
    assert_eq!(actual.t_assign_ms, opt_f64(&expected["t_assign"]), "{ctx}: t_assign");
    assert_eq!(actual.t_render_ms, opt_f64(&expected["t_render"]), "{ctx}: t_render");
    assert_eq!(
        actual.t_render_frame_ms,
        opt_f64(&expected["t_render_frame"]),
        "{ctx}: t_render_frame"
    );
    assert_eq!(post_ms_pairs(actual), pairs_f64(&expected["post_ms"]), "{ctx}: post_ms");
    assert_eq!(
        actual.first_visual_ms,
        opt_f64(&expected["first_visual"]),
        "{ctx}: first_visual"
    );
    assert_eq!(
        actual.final_visual_ms,
        opt_f64(&expected["final_visual"]),
        "{ctx}: final_visual"
    );
    assert_eq!(actual.incomplete, bool_of(expected, "incomplete"), "{ctx}: incomplete");
    assert_eq!(actual.kind, str_of(expected, "kind"), "{ctx}: kind");
}

/// Compares one `build` golden case field by field, in the golden's order.
fn assert_build_case(case: &Value) {
    let name = str_of(case, "name");
    let file = parse_text(Path::new("perf-x.csv"), str_of(case, "content"));
    let analysis = build(&file);

    let navs = arr(case, "navs");
    assert_eq!(analysis.navs.len(), navs.len(), "{name}: nav count");
    for (actual, expected) in analysis.navs.iter().zip(navs) {
        assert_nav(actual, expected, name);
    }

    let preload = arr(case, "preload_items");
    assert_eq!(
        analysis.preload_items.len(),
        preload.len(),
        "{name}: preload_items count"
    );
    for (actual, expected) in analysis.preload_items.iter().zip(preload) {
        let ctx = format!("{name}: preload item slot {}", actual.slot);
        assert_eq!(i64::from(actual.slot), i64_of(expected, "slot"), "{ctx}: slot");
        assert_eq!(actual.path_id, str_of(expected, "path_id"), "{ctx}: path_id");
        assert_eq!(
            actual.queue_wait_ms,
            f64_of(expected, "queue_wait_ms"),
            "{ctx}: queue_wait_ms"
        );
        assert_eq!(actual.kind, str_of(expected, "kind"), "{ctx}: kind");
        assert_eq!(actual.ms, f64_of(expected, "ms"), "{ctx}: ms");
    }
    assert_eq!(
        i64::from(analysis.preload_paused_count),
        i64_of(case, "preload_paused_count"),
        "{name}: preload_paused_count"
    );
    assert_eq!(
        i64::from(analysis.preload_cancel_count),
        i64_of(case, "preload_cancel_count"),
        "{name}: preload_cancel_count"
    );

    let gens = arr(case, "folder_gens");
    assert_eq!(analysis.folder_gens.len(), gens.len(), "{name}: folder_gens count");
    for (actual, expected) in analysis.folder_gens.iter().zip(gens) {
        let ctx = format!("{name}: folder gen {}", actual.generation);
        assert_eq!(actual.generation, i64_of(expected, "gen"), "{ctx}: gen");
        assert_eq!(actual.t1_catalog_ready_ms, opt_f64(&expected["t1"]), "{ctx}: t1");
        assert_eq!(actual.t2_ms, opt_f64(&expected["t2"]), "{ctx}: t2");
        assert_eq!(actual.t3_ms, opt_f64(&expected["t3"]), "{ctx}: t3");
        assert_eq!(actual.t3_phase, str_of(expected, "t3_phase"), "{ctx}: t3_phase");
    }

    let ops = arr(case, "dispatcher_long_ops");
    assert_eq!(
        analysis.dispatcher_long_ops.len(),
        ops.len(),
        "{name}: dispatcher_long_ops count"
    );
    for (actual, expected) in analysis.dispatcher_long_ops.iter().zip(ops) {
        assert_eq!(actual.ms, f64_of(expected, "ms"), "{name}: dispatcher ms");
        assert_eq!(
            actual.priority,
            str_of(expected, "priority"),
            "{name}: dispatcher priority"
        );
        assert_eq!(actual.name, str_of(expected, "name"), "{name}: dispatcher name");
    }
    assert_eq!(
        i64::from(analysis.dispatcher_long_ops_before_start_count),
        i64_of(case, "dispatcher_long_ops_before_start_count"),
        "{name}: dispatcher_long_ops_before_start_count"
    );

    let startup: Vec<(String, f64)> = analysis.startup.iter().map(|(k, v)| (k.clone(), *v)).collect();
    assert_eq!(startup, pairs_f64(&case["startup"]), "{name}: startup");
}

// ---------------------------------------------------------------- golden loop (perf-nav)

#[test]
fn golden_perf_nav_build_cases_match_csharp() {
    let golden = golden::load("perf-nav");
    let builds = cases_of_kind(&golden.cases, "build");
    assert_eq!(builds.len(), 21, "perf-nav build case count");
    for case in builds {
        assert_build_case(case);
    }
}

#[test]
fn golden_perf_nav_classify_cases_match_csharp() {
    let golden = golden::load("perf-nav");
    let classify = cases_of_kind(&golden.cases, "classify");
    assert_eq!(classify.len(), 15, "perf-nav classify case count");
    for case in classify {
        let nav = NavRecord {
            lookup_result: str_of(case, "lookup").to_owned(),
            has_thumbnail: bool_of(case, "has_thumbnail"),
            t_disk_ms: opt_f64(&case["t_disk"]),
            t_decode_ms: opt_f64(&case["t_decode"]),
            ..NavRecord::default()
        };
        assert_eq!(classify_kind(&nav), str_of(case, "result"), "classify {case}");
    }
}

#[test]
fn golden_perf_nav_covers_all_36_cases() {
    assert_eq!(golden::load("perf-nav").cases.len(), 36);
}

// ---------------------------------------------------------------- PerfAnalyzeTests (navigation, sample CSV)

// C#: PerfAnalyzeTests.ClassifiesEveryNavInTheSampleCsv
#[test]
fn classifies_every_nav_in_the_sample_csv() {
    let analysis = build(&sample_file());

    assert_eq!(analysis.navs.len(), 8);
    let by_nav = |id: i64| {
        analysis
            .navs
            .iter()
            .find(|n| n.nav == id)
            .unwrap_or_else(|| panic!("no nav {id}"))
    };
    assert_eq!(by_nav(1).kind, "RamHit");
    assert_eq!(by_nav(2).kind, "RamHit");
    assert_eq!(by_nav(3).kind, "RamHit");
    assert_eq!(by_nav(4).kind, "SourceMiss");
    assert!(
        !by_nav(4).has_thumbnail,
        "nav 4 (Fast mode) must not show a thumbnail first"
    );
    assert_eq!(by_nav(5).kind, "Thumbnail+SourceMiss");
    assert!(by_nav(5).has_thumbnail);
    assert_eq!(by_nav(6).kind, "DiskCacheHit");
    assert_eq!(by_nav(7).kind, "InflightJoin");

    // nav 8 is superseded mid-decode: no Verify/Assign/Presented ever arrives for it.
    assert!(by_nav(8).incomplete);
    assert_eq!(analysis.navs.iter().filter(|n| n.incomplete).count(), 1);
}

// C#: PerfAnalyzeTests.PrefersTheSpecificThumbnailCacheSourceOverMainWindowsUnknownTotal
#[test]
fn prefers_the_specific_thumbnail_cache_source_over_main_windows_unknown_total() {
    let analysis = build(&sample_file());
    let nav5 = analysis.navs.iter().find(|n| n.nav == 5).expect("nav 5");

    // ThumbEnd(source=decode, ms=20.0) from ThumbnailCache, then ThumbEnd(source=unknown, ms=20.5) from MainWindow:
    // the specific one must be kept.
    assert_eq!(nav5.t_thumb_ms, Some(20.0));
}

// C#: PerfAnalyzeTests.MatchesKeyInputToShowStartAndComputesFirstAndFinalVisual
#[test]
fn matches_key_input_to_show_start_and_computes_first_and_final_visual() {
    let analysis = build(&sample_file());
    let nav1 = analysis.navs.iter().find(|n| n.nav == 1).expect("nav 1");

    assert_eq!(nav1.t_input_ms, Some(5.0));
    some_close3(nav1.t_pre_ms, 2.0, "nav1 TPreMs");
    some_close3(nav1.final_visual_ms, 10.1, "nav1 FinalVisualMs");
    assert_eq!(nav1.final_visual_ms, nav1.first_visual_ms);

    let nav5 = analysis.navs.iter().find(|n| n.nav == 5).expect("nav 5");
    some_close3(nav5.first_visual_ms, 6.0, "nav5 FirstVisualMs");
    some_close3(nav5.final_visual_ms, 63.05, "nav5 FinalVisualMs");
    assert!(nav5.has_thumbnail, "first visual is the thumbnail Presented row");
}

// C#: PerfAnalyzeTests.CapturesRenderedFrameAsTheFrameAccurateRenderProxyAlongsideRendered
#[test]
fn captures_rendered_frame_as_the_frame_accurate_render_proxy_alongside_rendered() {
    let analysis = build(&sample_file());
    let by_nav = |id: i64| analysis.navs.iter().find(|n| n.nav == id).expect("nav");

    assert_eq!(by_nav(1).t_render_ms, Some(4.0));
    assert_eq!(by_nav(1).t_render_frame_ms, Some(20.0));
    assert_eq!(by_nav(2).t_render_frame_ms, Some(19.5));
    assert_eq!(by_nav(3).t_render_frame_ms, Some(21.0));

    // nav 8 is superseded before it ever renders: no RenderedFrame event exists for it.
    assert_eq!(by_nav(8).t_render_frame_ms, None);
}

// C#: PerfAnalyzeTests.BuildsFolderT0ToT2FromTheEarliestPresentedAfterFolderStart
#[test]
fn builds_folder_t0_to_t2_from_the_earliest_presented_after_folder_start() {
    let analysis = build(&sample_file());
    let gen1: Vec<_> = analysis.folder_gens.iter().filter(|g| g.generation == 1).collect();
    assert_eq!(gen1.len(), 1, "exactly one generation 1");
    let gen1 = gen1[0];

    assert_eq!(gen1.t1_catalog_ready_ms, Some(120.0));
    assert_eq!(gen1.t3_phase, "explorerApplied");
    assert_eq!(gen1.t3_ms, Some(180.0));
    some_close3(gen1.t2_ms, 210.1, "gen1 T2Ms");
}

// C#: PerfAnalyzeTests.BuildsStartupMilestonesAndMapsFirstPresentedOntoTheProcessStartTimeline
#[test]
fn builds_startup_milestones_and_maps_first_presented_onto_the_process_start_timeline() {
    // qpcFrequency 10 MHz: 10000 ticks = 1 ms. appStartup at 250 ms since process start.
    let text = "# commit=test diag= qpcFrequency=10000000\n\
utcTicks,qpcTicks,thread,event,nav,pathId,a,b,c,d,text\n\
1,2500000,1,Startup,,,250,,,,appStartup\n\
1,3000000,1,Startup,,,300,,,,servicesBuilt\n\
1,9000000,1,Startup,,,900,,,,openPathBegin\n\
1,9000000,1,Folder,1,,0,,,,start\n\
1,9100000,4,FolderInfo,1,,1841,,,,scanned;\n\
1,14000000,1,Folder,1,,500,,,,catalogReady\n\
1,15000000,1,Presented,1,,,,,,final\n\
1,16000000,1,Presented,2,,,,,,final\n";

    let analysis = build(&parse_text(Path::new("perf-2-x.csv"), text));

    let startup = |key: &str| {
        analysis
            .startup
            .get(key)
            .copied()
            .unwrap_or_else(|| panic!("no startup key {key}"))
    };
    assert_eq!(startup("appStartup"), 250.0);
    assert_eq!(startup("openPathBegin"), 900.0);
    // First Presented is 1250 ms after the appStartup row, which itself is 250 ms in.
    assert_close3(startup("firstPresented"), 1500.0, "firstPresented");
    assert_eq!(analysis.folder_gens.len(), 1);
    assert_eq!(analysis.folder_gens[0].t1_catalog_ready_ms, Some(500.0));
}

// C#: PerfAnalyzeTests.AggregatesPreloadItemsDispatcherAndPausedCounters
#[test]
fn aggregates_preload_items_dispatcher_and_paused_counters() {
    let analysis = build(&sample_file());

    assert_eq!(analysis.preload_items.len(), 4);
    assert_eq!(analysis.preload_paused_count, 1);
    assert_eq!(analysis.preload_cancel_count, 0);
    assert_eq!(analysis.dispatcher_long_ops.len(), 1);
    assert_eq!(analysis.dispatcher_long_ops[0].ms, 22.0);
    assert_eq!(analysis.dispatcher_long_ops[0].priority, "Background");
    assert_eq!(analysis.dispatcher_long_ops[0].name, "SessionSave");

    let decoded = analysis
        .preload_items
        .iter()
        .find(|p| p.kind == "decoded")
        .expect("a decoded preload item");
    assert!(decoded.queue_wait_ms > 0.0);
}

// C#: PerfAnalyzeTests.ExcludesDispatcherLongOpsBeforeTheFirstShowStartOrFolderStart
#[test]
fn excludes_dispatcher_long_ops_before_the_first_show_start_or_folder_start() {
    // --perf-session's own STA harness creating the WPF Window, well before Folder(start).
    let text = "# commit=test diag= qpcFrequency=10000000\n\
utcTicks,qpcTicks,thread,event,nav,pathId,a,b,c,d,text\n\
1,1000,1,DispatcherLongOp,,,700.0,,,,Normal;WindowCreate\n\
1,500000,1,Folder,1,,0,,,,start\n\
1,510000,1,DispatcherLongOp,,,20.0,,,,Background;SessionSave\n";

    let analysis = build(&parse_text(Path::new("perf-1-x.csv"), text));

    assert_eq!(analysis.dispatcher_long_ops.len(), 1);
    assert_eq!(analysis.dispatcher_long_ops[0].name, "SessionSave");
    assert_eq!(analysis.dispatcher_long_ops_before_start_count, 1);
}

// ---------------------------------------------------------------- PerfAnalyzeHardeningTests (PA-01)

// C#: PerfAnalyzeHardeningTests.DecodeEvent_IsNotReducedByTheSourceReadEvent
#[test]
fn decode_event_is_not_reduced_by_the_source_read_event() {
    // 1 tick = 1 us (1 MHz).
    let file = file_of(
        vec![
            hrow(0, "ShowStart", "", "Fast"),
            hrow(1, "Lookup", "", "miss"),
            hrow(2_000, "SourceRead", "30", ""),
            hrow(3_000, "Decode", "40", ""),
            hrow(90_000, "Presented", "", "final"),
        ],
        1_000_000,
    );

    let navs = build(&file).navs;
    assert_eq!(navs.len(), 1);
    assert_eq!(navs[0].t_read_ms, Some(30.0));
    assert_eq!(navs[0].t_decode_ms, Some(40.0));
}

// C#: PerfAnalyzeHardeningTests.DecodeEvent_ShorterThanTheRead_IsKept
#[test]
fn decode_event_shorter_than_the_read_is_kept() {
    let file = file_of(
        vec![
            hrow(0, "ShowStart", "", "Fast"),
            hrow(1, "Lookup", "", "miss"),
            hrow(2_000, "SourceRead", "50", ""),
            hrow(3_000, "Decode", "20", ""),
            hrow(90_000, "Presented", "", "final"),
        ],
        1_000_000,
    );

    let navs = build(&file).navs;
    assert_eq!(navs.len(), 1);
    assert_eq!(navs[0].t_decode_ms, Some(20.0));
}

// ---------------------------------------------------------------- KeyInput matching

/// xorshift64: a deterministic sequence standing in for `new Random(12345)` (its sequence cannot be reproduced).
struct XorShift(u64);

impl XorShift {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// An integer in `lo..hi` (`hi` exclusive).
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next_u64() % ((hi - lo) as u64)) as i64
    }
}

// C#: PerfAnalyzeKeyInputMatchTests.KeyInputMatchingAgreesWithTheBruteForceDefinition
#[test]
fn key_input_matching_agrees_with_the_brute_force_definition() {
    let mut rng = XorShift(12_345);
    let mut rows = Vec::new();
    let mut key_inputs = Vec::new();
    // Frequency 1 MHz: 1000 ticks = 1 ms. Timestamps collide often (multiples of 500) and arrive out of order.
    for i in 0..300 {
        let qpc = rng.range(0, 400) * 500;
        let thread = rng.range(1, 4) as i32;
        let k = row(0, qpc, thread, "KeyInput", "", &(i + 1).to_string(), "");
        key_inputs.push(k.clone());
        rows.push(k);
    }
    let mut starts = Vec::new();
    for n in 1..=200 {
        let qpc = rng.range(0, 500) * 500;
        let thread = rng.range(1, 4) as i32;
        let s = row(0, qpc, thread, "ShowStart", &n.to_string(), "0", "Preview");
        rows.push(s.clone());
        rows.push(row(0, s.qpc_ticks + 100, s.thread, "Presented", &s.nav, "1", "final"));
        starts.push(s);
    }
    let file = file_of(rows, 1_000_000);

    let analysis = build(&file);
    assert_eq!(analysis.navs.len(), 200);

    let mut matched = 0;
    for nav in &analysis.navs {
        let start = starts
            .iter()
            .find(|s| s.nav == nav.nav.to_string())
            .expect("a ShowStart for every nav");
        // Brute force: nearest KeyInput on the same thread at or before, within 500 ms; first in file order on ties.
        let mut expected: Option<&PerfRow> = None;
        for k in &key_inputs {
            if k.thread != start.thread || k.qpc_ticks > start.qpc_ticks {
                continue;
            }
            if (start.qpc_ticks - k.qpc_ticks) as f64 * 1000.0 / 1_000_000.0 > 500.0 {
                continue;
            }
            match expected {
                Some(best) if best.qpc_ticks >= k.qpc_ticks => {}
                _ => expected = Some(k),
            }
        }
        assert_eq!(nav.t_input_ms, expected.and_then(|k| k.a_num()), "nav {}", nav.nav);
        if expected.is_some() {
            matched += 1;
        }
    }
    // The scenario exercises both matching and non-matching navs.
    assert!((20..=199).contains(&matched), "matched {matched}");
}

// C#: PerfAnalyzeNavBuilder KeyInputIndex.NearestAtOrBefore (internal; no C# test of its own)
#[test]
fn key_input_index_picks_first_in_file_order_among_equal_timestamps() {
    let rows = vec![
        row(0, 500, 2, "KeyInput", "", "a1", ""),
        row(0, 500, 2, "KeyInput", "", "a2", ""),
        row(0, 100, 2, "KeyInput", "", "a0", ""),
        row(0, 700, 3, "KeyInput", "", "t3", ""),
    ];
    let index = KeyInputIndex::new(&rows);

    assert_eq!(index.nearest_at_or_before(2, 600).map(|r| r.a.as_str()), Some("a1"));
    assert_eq!(index.nearest_at_or_before(2, 10_000).map(|r| r.a.as_str()), Some("a1"));
    assert_eq!(index.nearest_at_or_before(2, 499).map(|r| r.a.as_str()), Some("a0"));
    assert!(index.nearest_at_or_before(2, 50).is_none(), "nothing at or before 50");
    assert!(index.nearest_at_or_before(9, 600).is_none(), "thread without KeyInputs");
    assert!(
        index.nearest_at_or_before(3, 600).is_none(),
        "its only KeyInput is later"
    );
}

// ---------------------------------------------------------------- PerfAnalyzeBuilderBenchmark (manual)

/// `PerfAnalyzeBuilderBenchmark.Synthetic`: N navs, one KeyInput each, 10 ms apart at 1 MHz.
fn synthetic(navs: usize) -> PerfCsvFile {
    let mut rows = Vec::with_capacity(navs * 3);
    for n in 1..=navs {
        let q = n as i64 * 10_000;
        let nav = n.to_string();
        rows.push(row(0, q, 1, "KeyInput", "", "1.5", ""));
        rows.push(row(0, q + 2_000, 1, "ShowStart", &nav, &nav, "Preview"));
        rows.push(row(0, q + 5_000, 1, "Presented", &nav, "1", "final"));
    }
    file_of(rows, 1_000_000)
}

/// Median of seven timed `build` runs after a warm-up, printed like the C# `WriteLine`.
fn run_build_timings(navs: usize) {
    let file = synthetic(navs);
    let _ = build(&file); // warm-up
    let mut samples = Vec::new();
    for _ in 0..7 {
        let started = Instant::now();
        let analysis = build(&file);
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(analysis.navs.len(), navs);
        assert!(analysis.navs.iter().all(|n| n.t_input_ms == Some(1.5)));
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "navs={navs} median={:.1} ms min={:.1} max={:.1}",
        samples[3], samples[0], samples[6]
    );
}

// C#: PerfAnalyzeBuilderBenchmark.BuildTimings (InlineData 2000)
#[test]
#[ignore = "manual benchmark"]
fn build_timings_2000_navs() {
    run_build_timings(2_000);
}

// C#: PerfAnalyzeBuilderBenchmark.BuildTimings (InlineData 10000)
#[test]
#[ignore = "manual benchmark"]
fn build_timings_10000_navs() {
    run_build_timings(10_000);
}

// ---------------------------------------------------------------- tie-order hardening (round-2 survivors)

// C#: PerfAnalyzeNavBuilder.Build, `row.QpcTicks < firstRelevantQpc` is strict
#[test]
fn dispatcher_long_op_at_exactly_the_first_show_start_qpc_is_kept() {
    let file = file_of(
        vec![
            row(1, 1_000, 1, "DispatcherLongOp", "", "5", "Normal;Before"),
            row(1, 2_000, 1, "ShowStart", "1", "", "Fast"),
            row(1, 2_000, 1, "DispatcherLongOp", "", "7", "Normal;AtStart"),
        ],
        1_000_000,
    );
    let a = build(&file);
    assert_eq!(a.dispatcher_long_ops_before_start_count, 1);
    assert_eq!(a.dispatcher_long_ops.len(), 1);
    assert_eq!(a.dispatcher_long_ops[0].name, "AtStart");
}

// C#: same guard with a Folder(start) threshold
#[test]
fn dispatcher_long_op_at_exactly_the_folder_start_qpc_is_kept() {
    let file = file_of(
        vec![
            row(1, 3_000, 1, "Folder", "1", "0", "start"),
            row(1, 3_000, 1, "DispatcherLongOp", "", "7", "Normal;AtStart"),
        ],
        1_000_000,
    );
    let a = build(&file);
    assert_eq!(a.dispatcher_long_ops_before_start_count, 0);
    assert_eq!(a.dispatcher_long_ops.len(), 1);
}

// C#: OrderBy(QpcTicks) is stable; equal qpc on different threads keeps file order
#[test]
fn equal_qpc_rows_of_one_nav_on_different_threads_keep_file_order() {
    let file = file_of(
        vec![
            row(1, 100, 1, "Stat", "1", "1", ""),
            row(1, 100, 2, "Stat", "1", "2", ""),
            row(1, 100, 2, "Lookup", "1", "", "ramHit"),
            row(1, 100, 1, "Lookup", "1", "", "miss"),
        ],
        1_000_000,
    );
    let a = build(&file);
    assert_eq!(a.navs.len(), 1);
    assert_eq!(a.navs[0].t_stat_ms, Some(2.0));
    assert_eq!(a.navs[0].lookup_result, "miss");
}

// C#: BuildFolderSummaries sorts by QpcTicks stably; same-text rows with equal qpc pick the first in file order
#[test]
fn equal_qpc_same_text_folder_rows_pick_the_first_in_file_order() {
    let file = file_of(
        vec![
            row(1, 100, 1, "Folder", "3", "1", "catalogReady"),
            row(1, 100, 1, "Folder", "3", "2", "catalogReady"),
            row(1, 200, 1, "Folder", "3", "5", "explorerApplied"),
            row(1, 200, 1, "Folder", "3", "6", "explorerFallback"),
        ],
        1_000_000,
    );
    let a = build(&file);
    assert_eq!(a.folder_gens.len(), 1);
    assert_eq!(a.folder_gens[0].t1_catalog_ready_ms, Some(1.0));
    assert_eq!(a.folder_gens[0].t3_ms, Some(5.0));
    assert_eq!(a.folder_gens[0].t3_phase, "explorerApplied");
}
