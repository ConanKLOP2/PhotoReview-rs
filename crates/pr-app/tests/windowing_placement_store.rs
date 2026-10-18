//! Port of the store-level parts of `PhotoReview.App.Tests` (baseline d92d6aae): `JsonWindowPlacementStoreRoundTripTests`,
//! `WindowPlacementPrefetchTests`, `WindowPlacementServiceFailureTests` and the `WriteAtomically` pair of
//! `WindowPlacementServiceTests`, plus golden `window-placement-json` (oracle-cs, 111 cases). Contract: `contracts/W8-03a.md`.
//!
//! Test double caveat (contract): `FakeFileSystem::write_all_text_atomic` never creates a temp file and its `read_all_text`
//! decodes UTF-8 only, so UTF-16 golden cases run only on the real file system (`golden_..._read_raw_files_on_disk`). The
//! "no temp file left" guards are the real-file tests below.

use std::ffi::OsString;
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pr_app::windowing::contracts::{SHOW_MAXIMIZED, WindowPlacementData, WindowPlacementStore};
use pr_app::windowing::monitor_layout::{FixedMonitorLayout, MonitorLayout, ScreenRect};
use pr_app::windowing::placement_store::{JsonWindowPlacementStore, PlacementReadError};
use pr_core::abstractions::FileSystem;
use pr_core::io::physical::PhysicalFileSystem;
use pr_test_support::golden::{self, i64_of, str_of};
use pr_test_support::raw::from_hex;
use pr_test_support::{FakeFileSystem, FsOp, HookFileSystem, TempRoot};
use serde_json::Value;

/// Port of the C# `Sample` of the round-trip tests (`ShowCommand` 3).
const SAMPLE: WindowPlacementData = WindowPlacementData {
    length: 44,
    flags: 2,
    show_command: SHOW_MAXIMIZED,
    min_x: -1,
    min_y: -2,
    max_x: -8,
    max_y: -31,
    normal_left: 100,
    normal_top: 120,
    normal_right: 1300,
    normal_bottom: 920,
};

/// The C# `GoldenFile` constant (LF line ends); the bytes `Save` must write are the same text with CRLF.
const GOLDEN_LF: &str = "{\n  \"Length\": 44,\n  \"Flags\": 2,\n  \"ShowCommand\": 3,\n  \"MinPosition\": {\n    \"X\": -1,\n    \"Y\": -2\n  },\n  \"MaxPosition\": {\n    \"X\": -8,\n    \"Y\": -31\n  },\n  \"NormalPosition\": {\n    \"Left\": 100,\n    \"Top\": 120,\n    \"Right\": 1300,\n    \"Bottom\": 920\n  }\n}";

const PRIMARY_WORK: ScreenRect = ScreenRect::new(0, 0, 1920, 1040);

fn crlf(lf: &str) -> String {
    lf.replace('\n', "\r\n")
}

fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn layout_primary() -> Arc<dyn MonitorLayout> {
    Arc::new(FixedMonitorLayout::new(vec![PRIMARY_WORK]))
}

fn store_with(fs: Arc<dyn FileSystem>) -> JsonWindowPlacementStore {
    JsonWindowPlacementStore::new(fs, layout_primary())
}

/// The sample moved fully off the primary monitor (a Maximized window saved off-screen still reopens Maximized).
fn offscreen() -> WindowPlacementData {
    WindowPlacementData {
        normal_left: 5000,
        normal_right: 6000,
        ..SAMPLE
    }
}

fn i32_of(case: &Value, key: &str) -> i32 {
    i32::try_from(i64_of(case, key)).unwrap_or_else(|_| panic!("'{key}' does not fit an i32: {case}"))
}

/// The eleven fields of a golden `value` object (snake_case names, see the contract).
fn data_of(value: &Value) -> WindowPlacementData {
    WindowPlacementData {
        length: i32_of(value, "length"),
        flags: i32_of(value, "flags"),
        show_command: i32_of(value, "show_command"),
        min_x: i32_of(value, "min_x"),
        min_y: i32_of(value, "min_y"),
        max_x: i32_of(value, "max_x"),
        max_y: i32_of(value, "max_y"),
        normal_left: i32_of(value, "normal_left"),
        normal_top: i32_of(value, "normal_top"),
        normal_right: i32_of(value, "normal_right"),
        normal_bottom: i32_of(value, "normal_bottom"),
    }
}

/// Checks one golden read outcome: `threw` set means a JSON error, otherwise `null` is `Ok(None)` and an object is `Ok(Some)`.
fn expect_read_outcome(name: &str, got: Result<Option<WindowPlacementData>, PlacementReadError>, case: &Value) {
    if case["threw"].is_null() {
        let want = if case["value"].is_null() {
            None
        } else {
            Some(data_of(&case["value"]))
        };
        match got {
            Ok(value) => assert_eq!(value, want, "{name}"),
            Err(error) => panic!("{name}: expected a value, got error {error}"),
        }
    } else {
        assert!(
            matches!(got, Err(PlacementReadError::Json(_))),
            "{name}: expected a JSON error (threw {}), got {got:?}",
            case["threw"]
        );
    }
}

// C#: golden window-placement-json (oracle-cs window-placement-json: 8 serialize, 90 deserialize,
// 11 read_raw_file, 1 read_raw_missing, 1 read_raw_directory)
#[test]
fn golden_window_placement_json() {
    let golden = golden::load("window-placement-json");
    assert_eq!(golden.cases.len(), 111, "the golden has 111 cases");
    for case in &golden.cases {
        let name = str_of(case, "name");
        match str_of(case, "kind") {
            "serialize" => {
                let text = JsonWindowPlacementStore::serialize(&data_of(&case["data"]));
                assert_eq!(text, str_of(case, "json"), "serialize {name}");
                assert!(text.contains("\r\n"), "serialize {name}: CRLF line ends");
                assert!(
                    !text.replace("\r\n", "").contains('\n'),
                    "serialize {name}: a bare LF is never written"
                );
            }
            "deserialize" => {
                let got = JsonWindowPlacementStore::deserialize(str_of(case, "text"));
                expect_read_outcome(&format!("deserialize {name}"), got, case);
            }
            "read_raw_file" => {
                // The fake decodes UTF-8 only (contract, test-double caveat): UTF-16 cases run in the on-disk test below.
                if name.contains("utf16") {
                    continue;
                }
                let fs = FakeFileSystem::new();
                let path = r"C:\pr-test\golden\placement.json";
                fs.add_file(path, &from_hex(str_of(case, "bytes_hex")));
                let got = JsonWindowPlacementStore::read_raw(&fs, path);
                expect_read_outcome(&format!("read_raw_file {name}"), got, case);
            }
            "read_raw_missing" => {
                let fs = FakeFileSystem::new();
                let got = JsonWindowPlacementStore::read_raw(&fs, r"C:\pr-test\golden\never-written.json");
                expect_read_outcome(&format!("read_raw_missing {name}"), got, case);
            }
            "read_raw_directory" => {
                let fs = FakeFileSystem::new();
                let folder = r"C:\pr-test\golden\folder.json";
                fs.add_dir(folder);
                let got = JsonWindowPlacementStore::read_raw(&fs, folder);
                expect_read_outcome(&format!("read_raw_directory {name}"), got, case);
            }
            other => panic!("unknown kind '{other}' in {name}"),
        }
    }
}

/// The same `read_raw_file` cases on the real file system, including the UTF-16 byte-order marks the fake cannot decode.
// C#: golden window-placement-json, kind read_raw_file (on disk, byte-order marks)
#[test]
#[ignore = "native: real files, byte-order marks"]
fn golden_window_placement_json_read_raw_files_on_disk() {
    let root = TempRoot::new("placement-golden-raw");
    let fs = PhysicalFileSystem;
    let golden = golden::load("window-placement-json");
    for case in golden.cases.iter().filter(|c| str_of(c, "kind") == "read_raw_file") {
        let name = str_of(case, "name");
        let path = root.join(format!("{name}.json"));
        std::fs::write(&path, from_hex(str_of(case, "bytes_hex"))).expect("write the golden bytes");
        let text = path.to_str().expect("utf-8 test path");
        expect_read_outcome(
            &format!("read_raw_file on disk {name}"),
            JsonWindowPlacementStore::read_raw(&fs, text),
            case,
        );
    }

    let missing = root.join("never-written.json");
    let got = JsonWindowPlacementStore::read_raw(&fs, missing.to_str().expect("utf-8 test path"));
    assert!(matches!(got, Ok(None)), "a missing file is Ok(None), got {got:?}");

    let folder = root.join("folder.json");
    std::fs::create_dir(&folder).expect("create the folder");
    let got = JsonWindowPlacementStore::read_raw(&fs, folder.to_str().expect("utf-8 test path"));
    assert!(matches!(got, Ok(None)), "a directory is Ok(None), got {got:?}");
}

// C#: JsonWindowPlacementStoreRoundTripTests.LegacyFile_ReadThenSave_IsByteIdentical (WP-08)
// The legacy text is the GoldenFile constant (the WPF class that wrote it is gone in Rust).
#[test]
fn legacy_file_read_then_save_is_byte_identical() {
    let fs = Arc::new(FakeFileSystem::new());
    let legacy = r"C:\pr-test\placement\legacy.json";
    let again = r"C:\pr-test\placement\again.json";
    fs.add_file(legacy, GOLDEN_LF.as_bytes());
    let original = fs.read_all_text(Path::new(legacy)).expect("the legacy file reads");

    let store = store_with(fs.clone());
    let read = JsonWindowPlacementStore::read_raw(&*fs, legacy)
        .expect("the legacy document is valid")
        .expect("the legacy file exists");
    assert_eq!(read, SAMPLE);

    store.save(again, &read).expect("saving the read placement succeeds");
    let saved = fs.read_all_text(Path::new(again)).expect("the saved file reads");
    assert_eq!(
        normalize(&original),
        normalize(&saved),
        "line endings aside, the bytes are the same"
    );
    assert_eq!(saved, crlf(GOLDEN_LF), "the saved file uses exact CRLF line ends");
}

// C#: JsonWindowPlacementStoreRoundTripTests.Save_WritesTheGoldenLayout (WP-08)
#[test]
fn save_writes_the_golden_layout() {
    let fs = Arc::new(FakeFileSystem::new());
    let path = r"C:\pr-test\placement\golden.json";

    store_with(fs.clone()).save(path, &SAMPLE).expect("save");

    assert_eq!(
        fs.read_all_text(Path::new(path)).expect("the saved file reads"),
        crlf(GOLDEN_LF)
    );
    assert_eq!(JsonWindowPlacementStore::serialize(&SAMPLE), crlf(GOLDEN_LF));
}

// C#: JsonWindowPlacementStoreRoundTripTests.ReadRaw_MissingShowCommand_DefaultsToMaximized (WP-08)
#[test]
fn read_raw_missing_show_command_defaults_to_maximized() {
    let fs = FakeFileSystem::new();
    let path = r"C:\pr-test\placement\partial.json";
    fs.add_file(path, br#"{"NormalPosition":{"Left":1,"Top":2,"Right":3,"Bottom":4}}"#);

    let read = JsonWindowPlacementStore::read_raw(&fs, path)
        .expect("a partial document is valid")
        .expect("the file exists");

    assert_eq!(read.show_command, SHOW_MAXIMIZED);
    assert_eq!(
        (read.normal_left, read.normal_top, read.normal_right, read.normal_bottom),
        (1, 2, 3, 4)
    );
}

// C#: JsonWindowPlacementStoreRoundTripTests.Save_CreatesDirectory_LeavesNoTempFile (WP-08)
#[test]
fn save_creates_directory_and_leaves_no_temp_file() {
    let fs = Arc::new(FakeFileSystem::new());
    let nested = r"C:\pr-test\placement\a\b";
    let path = format!(r"{nested}\window-placement.json");

    store_with(fs.clone())
        .save(&path, &SAMPLE)
        .expect("save creates the folder");

    let listing = fs.enumerate_files(Path::new(nested), "*").expect("the folder lists");
    assert_eq!(listing, vec![PathBuf::from(&path)]);
}

// C#: JsonWindowPlacementStoreRoundTripTests.Read_FiltersMissingDamagedAndInvisible (WP-08)
#[test]
fn read_filters_missing_damaged_and_invisible() {
    let fs = Arc::new(FakeFileSystem::new());
    let store = store_with(fs.clone());
    let visible = r"C:\pr-test\placement\visible.json";
    let offscreen_path = r"C:\pr-test\placement\offscreen.json";
    let damaged = r"C:\pr-test\placement\damaged.json";
    store.save(visible, &SAMPLE).expect("save visible");
    store.save(offscreen_path, &offscreen()).expect("save off-screen");
    fs.add_file(damaged, b"{ not json");

    assert_eq!(store.read(visible), Some(SAMPLE));
    assert_eq!(store.read(offscreen_path), None);
    assert_eq!(store.read(damaged), None);
    assert_eq!(store.read(r"C:\pr-test\placement\never-written.json"), None);
}

// C#: JsonWindowPlacementStoreRoundTripTests.ReadRaw_Damaged_Throws (WP-08)
#[test]
fn read_raw_damaged_throws() {
    let fs = FakeFileSystem::new();
    let damaged = r"C:\pr-test\placement\damaged.json";
    fs.add_file(damaged, b"{ not json");

    let got = JsonWindowPlacementStore::read_raw(&fs, damaged);

    assert!(matches!(got, Err(PlacementReadError::Json(_))), "got {got:?}");
}

// C#: WindowPlacementPrefetchTests.RestoreBeforeShow_AfterPrefetch_UsesPrefetchedPlacementExactlyOnce
// and JsonWindowPlacementStoreRoundTripTests.Prefetch_IsUnfilteredAndTakenOnceForTheSamePath (WP-08)
#[test]
fn prefetch_then_take_prefetched_is_taken_once_and_the_next_call_reads_nothing() {
    let fs = Arc::new(FakeFileSystem::new());
    let store = store_with(fs.clone());
    let path = r"C:\pr-test\placement\prefetch.json";
    let other = r"C:\pr-test\placement\other.json";
    store.save(path, &offscreen()).expect("save");

    let loaded = store.prefetch(path).join();
    assert_eq!(loaded, Some(offscreen()), "the prefetch is not filtered by visibility");

    // The file changed after the prefetch must not matter: the prefetched value is what the next take returns.
    fs.add_file(path, JsonWindowPlacementStore::serialize(&SAMPLE).as_bytes());
    assert_eq!(store.take_prefetched(other), None);
    assert_eq!(store.take_prefetched(&path.to_uppercase()), Some(offscreen()));
    assert_eq!(store.take_prefetched(path), None, "consumed by the first take");
}

// C#: WindowPlacementPrefetchTests.RestoreBeforeShow_AfterPrefetch_..._PrefetchOfAnotherPath_IsIgnoredButKept
#[test]
fn take_prefetched_other_path_keeps_the_slot() {
    let fs = Arc::new(FakeFileSystem::new());
    let store = store_with(fs.clone());
    let first = r"C:\pr-test\placement\first.json";
    let second = r"C:\pr-test\placement\second.json";
    store.save(first, &SAMPLE).expect("save");

    store.prefetch(first).join();

    assert_eq!(store.take_prefetched(second), None, "another path finds nothing");
    assert_eq!(
        store.take_prefetched(first),
        Some(SAMPLE),
        "the slot was kept for its own path"
    );
}

// C#: WindowPlacementPrefetchTests.RestoreBeforeShow_AfterPrefetch_..._PrefetchedPathDifferingOnlyByCase_IsUsed
#[test]
fn take_prefetched_matches_the_path_ignoring_case() {
    let fs = Arc::new(FakeFileSystem::new());
    let store = store_with(fs.clone());
    let path = r"C:\pr-test\placement\case.json";
    store.save(path, &SAMPLE).expect("save");

    store.prefetch(path).join();

    assert_eq!(store.take_prefetched(&path.to_uppercase()), Some(SAMPLE));
}

// C#: WindowPlacementPrefetchTests.RestoreBeforeShow_AfterPrefetch_..._PrefetchOfMissingFile_AppliesNothing
#[test]
fn prefetch_of_a_missing_file_joins_to_none_and_take_prefetched_is_none() {
    let fs = Arc::new(FakeFileSystem::new());
    let store = store_with(fs.clone());
    let missing = r"C:\pr-test\placement\missing.json";

    assert_eq!(store.prefetch(missing).join(), None);
    assert_eq!(store.take_prefetched(missing), None);
}

// C#: JsonWindowPlacementStoreRoundTripTests.Prefetch_DamagedFile_YieldsNullAndNothingToTake (WP-08)
#[test]
fn prefetch_damaged_file_yields_none_and_nothing_to_take() {
    let fs = Arc::new(FakeFileSystem::new());
    let store = store_with(fs.clone());
    let damaged = r"C:\pr-test\placement\damaged.json";
    fs.add_file(damaged, b"{ not json");

    assert_eq!(store.prefetch(damaged).join(), None);
    assert_eq!(store.take_prefetched(damaged), None);
}

// new (no C# twin): the handle's join answers the read value, unfiltered
#[test]
fn prefetch_handle_join_returns_the_value() {
    let fs = Arc::new(FakeFileSystem::new());
    let store = store_with(fs.clone());
    let path = r"C:\pr-test\placement\join.json";
    store.save(path, &SAMPLE).expect("save");

    assert_eq!(store.prefetch(path).join(), Some(SAMPLE));
}

// new (no C# twin): a take while the read is still running gives nothing, and the read still completes (no sleeps:
// a gate in the open-read hook holds the read until the test releases it)
#[test]
fn take_prefetched_before_the_read_finished_is_none() {
    let disk = Arc::new(FakeFileSystem::new());
    let path = r"C:\pr-test\placement\gated.json";
    disk.add_file(path, JsonWindowPlacementStore::serialize(&SAMPLE).as_bytes());
    let hooked = Arc::new(HookFileSystem::new(disk.clone()));
    let (release, gate) = mpsc::channel::<()>();
    let gate = Mutex::new(gate);
    hooked.set_open_read_hook(Some(Arc::new(move |_path: &Path| -> Option<io::Error> {
        // The timeout only stops a wrong implementation from hanging the run; the test releases the read itself.
        let _ = gate.lock().expect("gate").recv_timeout(Duration::from_secs(10));
        None
    })));
    let store = JsonWindowPlacementStore::new(hooked.clone(), layout_primary());

    let handle = store.prefetch(path);
    assert_eq!(store.take_prefetched(path), None, "the read has not finished");

    release.send(()).expect("the read is waiting at the gate");
    assert_eq!(handle.join(), Some(SAMPLE), "the read completes after the release");
}

// new (no C# twin): a second prefetch replaces the first slot
#[test]
fn second_prefetch_replaces_the_first() {
    let fs = Arc::new(FakeFileSystem::new());
    let store = store_with(fs.clone());
    let first = r"C:\pr-test\placement\first.json";
    let second = r"C:\pr-test\placement\second.json";
    store.save(first, &SAMPLE).expect("save first");
    store.save(second, &offscreen()).expect("save second");

    store.prefetch(first).join();
    store.prefetch(second).join();

    assert_eq!(store.take_prefetched(first), None, "the first slot was replaced");
    assert_eq!(store.take_prefetched(second), Some(offscreen()));
}

// C#: WindowPlacementServiceFailureTests (store part): a file that is null, empty, an array or garbage is not a placement
#[test]
fn read_of_null_empty_array_and_garbage_files_is_none() {
    let fs = Arc::new(FakeFileSystem::new());
    let store = store_with(fs.clone());
    // (path, text, read_raw is Ok(None))
    let cases = [
        (r"C:\pr-test\placement\garbage.json", "{ this is not json", false),
        (r"C:\pr-test\placement\null.json", "null", true),
        (r"C:\pr-test\placement\empty.json", "", false),
        (r"C:\pr-test\placement\array.json", "[1,2,3]", false),
    ];
    for (path, text, raw_is_none) in cases {
        fs.add_file(path, text.as_bytes());

        assert_eq!(store.read(path), None, "read {path}");
        let raw = JsonWindowPlacementStore::read_raw(&*fs, path);
        if raw_is_none {
            assert!(matches!(raw, Ok(None)), "read_raw {path} is Ok(None), got {raw:?}");
        } else {
            assert!(raw.is_err(), "read_raw {path} is an error, got {raw:?}");
        }
    }
}

// C#: WindowPlacementServiceTests.WriteAtomically_TempWriteFailsMidway_LeavesNoTempFile (RV-A15, fake part)
#[test]
fn write_atomically_temp_write_fails_midway_leaves_no_temp_file() {
    let fs = FakeFileSystem::new();
    let path = r"C:\pr-test\placement\placement.json";
    fs.fail(FsOp::WriteAtomic, path, io::ErrorKind::Other);

    let err =
        JsonWindowPlacementStore::write_atomically(&fs, path, "{}").expect_err("the disk failure reaches the caller");

    assert_eq!(err.kind(), io::ErrorKind::Other);
    assert!(
        fs.all_files().iter().all(|p| !p.to_string_lossy().ends_with(".tmp")),
        "no temp file is left behind"
    );
}

// C#: WindowPlacementServiceTests.WriteAtomically_SharedLegacyTempLocked_StillWrites
#[test]
#[ignore = "native: exclusive handle on <path>.tmp"]
fn write_atomically_shared_legacy_temp_locked_still_writes() {
    let root = TempRoot::new("placement-legacy-temp");
    let path = root.join("placement.json");
    let legacy_temp = root.join("placement.json.tmp");
    // A second window mid-write holds the old shared temp name without sharing it.
    let _holder = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .share_mode(0)
        .open(&legacy_temp)
        .expect("hold the legacy temp name");

    JsonWindowPlacementStore::write_atomically(
        &PhysicalFileSystem,
        path.to_str().expect("utf-8 test path"),
        r#"{"ok":1}"#,
    )
    .expect("the write succeeds beside the locked legacy name");

    assert_eq!(
        std::fs::read_to_string(&path).expect("read the placement"),
        r#"{"ok":1}"#
    );
}

// new (no C# twin): contract note 18, a bare file name or a root has no folder to create
#[test]
fn save_with_bare_file_name_or_root_is_invalid_input() {
    let fs = Arc::new(FakeFileSystem::new());
    let store = store_with(fs.clone());
    for path in ["", "placement.json", r"C:\"] {
        let err = store.save(path, &SAMPLE).expect_err("no folder to create");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "save {path:?}");
    }
    assert!(fs.all_files().is_empty(), "nothing is written");
}

// new (no C# twin): the filter applies to read only; read_raw returns the off-screen placement
#[test]
fn read_filters_invisible_but_read_raw_does_not() {
    let fs = Arc::new(FakeFileSystem::new());
    let store = store_with(fs.clone());
    let path = r"C:\pr-test\placement\hidden.json";
    store.save(path, &offscreen()).expect("save");

    assert_eq!(
        JsonWindowPlacementStore::read_raw(&*fs, path).expect("valid"),
        Some(offscreen())
    );
    assert_eq!(store.read(path), None);
}

// new (no C# twin): every field survives a save and read_raw, including the integer extremes
#[test]
fn save_then_read_raw_round_trips_every_field_including_extremes() {
    let fs = Arc::new(FakeFileSystem::new());
    let store = store_with(fs.clone());
    let extremes = WindowPlacementData {
        length: i32::MIN,
        flags: i32::MAX,
        show_command: -1,
        min_x: i32::MIN,
        min_y: i32::MAX,
        max_x: 0,
        max_y: -2,
        normal_left: i32::MIN,
        normal_top: i32::MIN,
        normal_right: i32::MAX,
        normal_bottom: i32::MAX,
    };
    let path = r"C:\pr-test\placement\extremes.json";

    store.save(path, &extremes).expect("save");

    assert_eq!(
        JsonWindowPlacementStore::read_raw(&*fs, path).expect("valid"),
        Some(extremes)
    );
}

// new (no C# twin): contract note 20, the store never touches the C# app's file (R0002)
#[test]
fn read_never_reads_the_csharp_app_folder() {
    let disk = Arc::new(FakeFileSystem::new());
    let rs = r"C:\Users\pr-test\AppData\Local\PhotoReviewRs\window-placement.json";
    let csharp = r"C:\Users\pr-test\AppData\Local\PhotoReview\window-placement.json";
    disk.add_file(rs, JsonWindowPlacementStore::serialize(&SAMPLE).as_bytes());
    disk.add_file(csharp, b"{ not json");

    let reads = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
    let recorder = Arc::clone(&reads);
    let hooked = Arc::new(HookFileSystem::new(disk.clone()));
    hooked.set_open_read_hook(Some(Arc::new(move |path: &Path| -> Option<io::Error> {
        recorder.lock().expect("reads").push(path.to_path_buf());
        None
    })));
    let store = JsonWindowPlacementStore::new(hooked.clone(), layout_primary());

    assert_eq!(store.read(rs), Some(SAMPLE));

    let touched = reads.lock().expect("reads").clone();
    assert!(!touched.is_empty(), "the Rust placement file was read");
    assert!(
        touched
            .iter()
            .all(|p| p.ends_with(Path::new(r"PhotoReviewRs\window-placement.json"))),
        "only the Rust placement file is read, got {touched:?}"
    );
}

// new (no C# twin): the error type has a message
#[test]
fn error_display_is_not_empty() {
    assert!(
        !PlacementReadError::Io(io::Error::other("disk gone"))
            .to_string()
            .is_empty()
    );
    assert!(
        !PlacementReadError::Json("unexpected token".to_owned())
            .to_string()
            .is_empty()
    );
}

// C#: WindowPlacementServiceFailureTests (store part): a folder that cannot be created returns Err and writes nothing
#[test]
#[ignore = "native: real files"]
fn save_when_the_folder_cannot_be_created_returns_err_and_writes_nothing() {
    let root = TempRoot::new("placement-no-folder");
    let blocker = root.join("blocker");
    std::fs::write(&blocker, b"a file, not a folder").expect("write the blocking file");
    let path = blocker.join("sub").join("window-placement.json");
    let store = JsonWindowPlacementStore::new(Arc::new(PhysicalFileSystem), layout_primary());

    let got = store.save(path.to_str().expect("utf-8 test path"), &SAMPLE);

    assert!(got.is_err(), "the folder cannot be created");
    assert!(!path.exists(), "nothing is written");
    let names: Vec<OsString> = std::fs::read_dir(root.path())
        .expect("list the test root")
        .map(|entry| entry.expect("directory entry").file_name())
        .collect();
    assert_eq!(
        names,
        vec![OsString::from("blocker")],
        "the blocking file is left as it was"
    );
}

// C#: WindowPlacementServiceFailureTests (store part): a target that is a directory returns Err and leaves no temp file
#[test]
#[ignore = "native: real files"]
fn save_when_the_target_is_a_directory_returns_err_and_leaves_no_temp_file() {
    let root = TempRoot::new("placement-dir-target");
    let target = root.join("window-placement.json");
    std::fs::create_dir(&target).expect("create the directory that is the target");
    let store = JsonWindowPlacementStore::new(Arc::new(PhysicalFileSystem), layout_primary());

    let got = store.save(target.to_str().expect("utf-8 test path"), &SAMPLE);

    assert!(got.is_err(), "a directory cannot be replaced by a file");
    assert!(target.is_dir(), "the directory is left as it was");
    let stray: Vec<OsString> = std::fs::read_dir(root.path())
        .expect("list the test root")
        .map(|entry| entry.expect("directory entry").file_name())
        .filter(|name| name.to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(stray.is_empty(), "no temp file is left: {stray:?}");
}

#[test]
fn error_source_is_the_io_error_for_io_and_none_for_json() {
    use std::error::Error;
    let io_error = PlacementReadError::Io(io::Error::other("x"));
    assert!(io_error.source().is_some());
    assert!(PlacementReadError::Json("x".into()).source().is_none());
}

#[test]
fn deserialize_slash_or_comma_brace_inside_a_string_is_not_a_comment_or_trailing_comma() {
    // C#: strings are opaque to the comment / trailing-comma check.
    assert!(
        JsonWindowPlacementStore::deserialize(r#"{"E":"a//b","Length":1}"#)
            .unwrap()
            .is_some()
    );
    assert!(
        JsonWindowPlacementStore::deserialize(r#"{"E":"x,}","Length":1}"#)
            .unwrap()
            .is_some()
    );
}

#[test]
fn deserialize_escaped_quote_inside_a_string_does_not_end_it() {
    assert!(
        JsonWindowPlacementStore::deserialize(r#"{"E":"a\",}","Length":1}"#)
            .unwrap()
            .is_some()
    );
    assert!(
        JsonWindowPlacementStore::deserialize(r#"{"E":"a\"//","Length":1}"#)
            .unwrap()
            .is_some()
    );
}

#[test]
fn debug_names_the_type() {
    let store = store_with(Arc::new(FakeFileSystem::new()));
    assert!(format!("{store:?}").contains("JsonWindowPlacementStore"));
}
