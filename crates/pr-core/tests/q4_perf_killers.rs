//! Q4 killer tests (parity close 2026-11-03) for `diagnostics/metrics.rs`.

use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::model::enums::DecoderBackend;

// metrics.rs:353:51 `member.value() == value` -> `!=` in `ReviewMetrics::snapshot` was caught only by the golden test.
// C#: ReviewMetricsTests.StatSessionWriteAndFallbackCountsAccumulate (the C# test reads the per-backend counts).
#[test]
fn snapshot_maps_each_fallback_count_to_its_own_backend() {
    let metrics = ReviewMetrics::new();
    metrics.record_decoder_fallback(DecoderBackend::WicDirect);
    metrics.record_decoder_fallback(DecoderBackend::TurboJpeg);
    metrics.record_decoder_fallback(DecoderBackend::WicDirect);

    let snapshot = metrics.snapshot();

    assert_eq!(snapshot.decoder_fallbacks.len(), 2, "{:?}", snapshot.decoder_fallbacks);
    let count_of = |backend: DecoderBackend| {
        snapshot
            .decoder_fallbacks
            .iter()
            .find(|(b, _)| *b == backend)
            .map(|(_, n)| *n)
    };
    assert_eq!(count_of(DecoderBackend::WicDirect), Some(2));
    assert_eq!(count_of(DecoderBackend::TurboJpeg), Some(1));
}

// perf.rs:115 `to_upper_invariant_char` (`-> Default::default()` and `== -> !=`) were caught only by the golden test.
// Literals: first 8 bytes of SHA-256 over "C:\AB" and "C:\S" (hashlib, Python 3.12).
// C#: PhotoReviewPerf.PathId (ToUpperInvariant, then SHA-256, first 8 bytes as lower-case hex).
#[test]
fn path_id_uppercases_ordinary_letters_and_maps_long_s_to_s() {
    use pr_core::diagnostics::perf::path_id;
    assert_eq!(path_id(r"C:\ab"), "3adfb61c9a163e84");
    assert_eq!(path_id(r"C:\AB"), "3adfb61c9a163e84");
    assert_eq!(
        path_id(&format!("C:{}{}", char::from(92), char::from_u32(0x17F).unwrap_or('?'))),
        "5666e40f33f61384"
    ); // U+017F long s -> S
    assert_eq!(path_id(r"C:\s"), "5666e40f33f61384");
}

// perf_csv.rs:439 `create_trace_file`: the extended-length prefix rule (`>= 260`, fully qualified, not already
// extended). Plain std would add the prefix itself for a long plain path, so only a path that is ALREADY
// extended and long can show a wrong condition: the mutants `&& -> ||` (col 43, col 90) and `delete !` (col 93)
// would prefix it a second time and the open fails. Native (real file system, long paths).
// C#: PerfCsvListenerStartupAndFaultCoverageTests (TryStart with a trace directory).
#[cfg(windows)]
mod long_trace_dir {
    use pr_core::diagnostics::perf_csv::{LocalTimeParts, PerfCsvListener, TRACE_ENV_VAR};

    fn start_in(dir: &str) -> PerfCsvListener {
        let lookup = |name: &str| (name == TRACE_ENV_VAR).then(|| dir.to_owned());
        let now = || LocalTimeParts {
            year: 2026,
            month: 11,
            day: 3,
            hour: 1,
            minute: 2,
            second: 3,
        };
        PerfCsvListener::try_start(&lookup, Some("q4"), &now)
            .unwrap_or_else(|| panic!("no listener for a {}-char trace directory", dir.len()))
    }

    fn long_dir(root: &pr_test_support::TempRoot) -> String {
        let mut dir = root.path().to_string_lossy().into_owned();
        while dir.encode_utf16().count() < 280 {
            dir.push(char::from(92));
            dir.push_str("long-trace-component-0123456789");
        }
        dir
    }

    fn one_trace_file(dir: &str) -> String {
        let names: Vec<String> = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("read_dir: {e}"))
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        std::fs::read_to_string(std::path::Path::new(dir).join(&names[0])).unwrap_or_else(|e| panic!("read: {e}"))
    }

    #[test]
    #[ignore = "native: creates a directory tree longer than 260 characters on the real file system"]
    fn a_long_plain_trace_directory_gets_its_trace_file() {
        let root = pr_test_support::TempRoot::new("q4-long-plain");
        let dir = long_dir(&root);
        start_in(&dir).dispose();
        assert!(one_trace_file(&dir).contains("# dropped=0"));
    }

    #[test]
    #[ignore = "native: creates a directory tree longer than 260 characters on the real file system"]
    fn an_already_extended_long_trace_directory_is_not_prefixed_twice() {
        let root = pr_test_support::TempRoot::new("q4-long-ext");
        let dir = format!("{0}{0}?{0}{1}", char::from(92), long_dir(&root));
        start_in(&dir).dispose();
        assert!(one_trace_file(&dir).contains("# dropped=0"));
    }
}
