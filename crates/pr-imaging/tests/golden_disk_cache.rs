//! Golden tests of W4-09 `cache::disk_store` (red, D21). Each case of `tests/golden/disk-cache-*.json` and
//! `cache-entry-failure.json` (C# oracle output at d92d6aae) is replayed against the Rust store. The file-system
//! goldens run on a real temp folder per case (`#[cfg(windows)]`: locks, Read-only and creation times are Windows
//! behaviour), are NOT ignored, and panic at the stub's `todo!()` until the porter lands the bodies.
//!
//! Comparison rules (contract "Rust golden test notes"): `<ROOT>` in logged messages stands for the case folder;
//! a golden exception class maps to an `io::ErrorKind` class (`IOException` = any `io::Error` but
//! `PermissionDenied`, `UnauthorizedAccessException` = `PermissionDenied`, `ArgumentException` = `InvalidInput`);
//! survivors are the regular files directly in the store folder, sorted ordinally.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File, FileTimes, OpenOptions};
use std::io;
use std::os::windows::fs::{FileTimesExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pr_core::abstractions::{Clock, Log};
use pr_core::time::DateTimeUtc;
use pr_imaging::cache::disk_store::{
    DiskCacheStore, clear_directory_matching, delete_stale_temp_files, is_cache_entry_failure, prune_directory,
    try_delete,
};
use pr_imaging::decoder_traits::DecodeError;
use pr_test_support::{FakeClock, TempRoot, golden};
use serde_json::Value;

/// .NET ticks of 1970-01-01T00:00:00Z.
const UNIX_EPOCH_TICKS: i64 = 621_355_968_000_000_000;

// ---- case folders ------------------------------------------------------------------------------------------------

/// A temp folder for one golden case. Read-only attributes are cleared before the folder is removed (contract:
/// "Read-only attributes are cleared before cleanup").
struct CaseFolder {
    root: TempRoot,
}

impl CaseFolder {
    fn new(label: &str) -> Self {
        Self {
            root: TempRoot::new(label),
        }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }
}

impl Drop for CaseFolder {
    fn drop(&mut self) {
        if let Ok(entries) = fs::read_dir(self.root.path()) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    let mut perms = meta.permissions();
                    if perms.readonly() {
                        // Windows-only folder: clearing the Read-only attribute is the intent (the Unix lint does not apply).
                        #[allow(clippy::permissions_set_readonly_false)]
                        perms.set_readonly(false);
                        let _ = fs::set_permissions(entry.path(), perms);
                    }
                }
            }
        }
    }
}

// ---- golden field helpers ----------------------------------------------------------------------------------------

fn utc(text: &str) -> DateTimeUtc {
    DateTimeUtc::parse_json(text).expect("golden timestamp").0
}

fn opt_utc(case: &Value, key: &str) -> Option<DateTimeUtc> {
    case.get(key).and_then(Value::as_str).map(utc)
}

fn flag(case: &Value, key: &str) -> bool {
    case.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn name_of(case: &Value) -> &str {
    case["name"].as_str().expect("case name")
}

fn size_of(entry: &Value) -> usize {
    usize::try_from(entry.get("size").and_then(Value::as_u64).unwrap_or(0)).expect("size fits")
}

/// Sorted string array from a golden field (`survivors`, `reads`, ...).
fn string_array(value: &Value) -> Vec<String> {
    let mut names: Vec<String> = value
        .as_array()
        .expect("string array")
        .iter()
        .map(|v| v.as_str().expect("string").to_owned())
        .collect();
    names.sort();
    names
}

// ---- file construction -------------------------------------------------------------------------------------------

/// `DateTimeUtc` as a `SystemTime` (ticks since 0001 less the Unix epoch's ticks).
fn to_system_time(t: DateTimeUtc) -> SystemTime {
    let ticks = t.ticks() - UNIX_EPOCH_TICKS;
    let magnitude = ticks.unsigned_abs();
    let unit = 10_000_000_u64;
    let d = Duration::new(magnitude / unit, u32::try_from(magnitude % unit).expect("< 1e7") * 100);
    if ticks >= 0 { UNIX_EPOCH + d } else { UNIX_EPOCH - d }
}

/// Sets any of the three file times, like C# `SetLastAccessTimeUtc` / `SetCreationTimeUtc` / `SetLastWriteTimeUtc`.
fn stamp(path: &Path, accessed: Option<DateTimeUtc>, created: Option<DateTimeUtc>, modified: Option<DateTimeUtc>) {
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap_or_else(|e| panic!("open {} for times: {e}", path.display()));
    let mut times = FileTimes::new();
    if let Some(t) = accessed {
        times = times.set_accessed(to_system_time(t));
    }
    if let Some(t) = created {
        times = times.set_created(to_system_time(t));
    }
    if let Some(t) = modified {
        times = times.set_modified(to_system_time(t));
    }
    file.set_times(times)
        .unwrap_or_else(|e| panic!("set times on {}: {e}", path.display()));
}

fn set_read_only(path: &Path, read_only: bool) {
    let mut perms = fs::metadata(path).expect("metadata").permissions();
    perms.set_readonly(read_only);
    fs::set_permissions(path, perms).expect("set attribute");
}

/// Builds one golden entry (file or directory) under `dir`, with its times and attributes. A `locked` entry is
/// NOT opened here: the caller keeps the handle (see [`lock_exclusive`]).
fn build_entry(dir: &Path, entry: &Value) -> PathBuf {
    let name = entry["name"].as_str().expect("entry name");
    let path = dir.join(name);
    if flag(entry, "dir") {
        fs::create_dir_all(&path).unwrap_or_else(|e| panic!("mkdir {}: {e}", path.display()));
        return path;
    }
    fs::write(&path, vec![0u8; size_of(entry)]).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    let access = opt_utc(entry, "access");
    let creation = opt_utc(entry, "creation");
    let write = opt_utc(entry, "write");
    if access.is_some() || creation.is_some() || write.is_some() {
        stamp(&path, access, creation, write);
    }
    if flag(entry, "readonly") {
        set_read_only(&path, true);
    }
    path
}

/// Opens `path` so no other handle can open, delete or rename it (C# `FileShare.None`).
fn lock_exclusive(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .unwrap_or_else(|e| panic!("lock {}: {e}", path.display()))
}

/// Regular files directly in `dir`, sorted ordinally (directories never count as survivors). A folder that does not
/// exist has none (golden `missing-directory-every-pass-scans`: the oracle reports `[]`, and the pass must not create
/// the folder).
fn survivors(dir: &Path) -> Vec<String> {
    let listing = match fs::read_dir(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Vec::new(),
        other => other,
    };
    let mut names: Vec<String> = listing
        .expect("list store folder")
        .map(|entry| entry.expect("entry"))
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The last access time of `path`, as the golden's System.Text.Json form (`DateTimeUtc::to_json`).
fn access_json(path: &Path) -> String {
    let accessed = fs::metadata(path).expect("metadata").accessed().expect("accessed");
    DateTimeUtc::from_system_time(accessed)
        .expect("representable")
        .to_json()
}

// ---- logs --------------------------------------------------------------------------------------------------------

/// Records `(normalized message, error kind)` for each `error` call (the golden's `log` shape).
#[derive(Debug, Default)]
struct KindLog {
    entries: Mutex<Vec<(String, Option<io::ErrorKind>)>>,
}

impl KindLog {
    fn entries(&self) -> Vec<(String, Option<io::ErrorKind>)> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Log for KindLog {
    fn enabled(&self) -> bool {
        true
    }

    fn info(&self, _message: &str) {}

    fn warn(&self, _message: &str) {}

    fn error(&self, message: &str, error: Option<&(dyn std::error::Error + 'static)>) {
        let kind = error.and_then(|e| e.downcast_ref::<io::Error>()).map(io::Error::kind);
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((message.to_owned(), kind));
    }
}

/// `Delete failed: C:\...\a.png` becomes `Delete failed: <ROOT>\a.png` (the golden writes the case folder as `<ROOT>`).
fn normalize(message: &str) -> String {
    let Some((head, path)) = message.split_once(": ") else {
        return message.to_owned();
    };
    match path.rfind('\\') {
        Some(i) => format!("{head}: <ROOT>{}", &path[i..]),
        None => message.to_owned(),
    }
}

/// Golden exception class against an `io::ErrorKind` (contract "Rust golden test notes").
fn class_matches(expected: &str, kind: Option<io::ErrorKind>) -> bool {
    match expected {
        "IOException" => kind.is_some_and(|k| k != io::ErrorKind::PermissionDenied),
        "UnauthorizedAccessException" => kind == Some(io::ErrorKind::PermissionDenied),
        "ArgumentException" => kind == Some(io::ErrorKind::InvalidInput),
        other => panic!("golden exception class {other} has no io::ErrorKind mapping"),
    }
}

/// Asserts the logged entries equal the golden `log` array (same length, order, normalized text and class).
fn check_log(name: &str, actual: &[(String, Option<io::ErrorKind>)], expected: &[Value]) {
    assert_eq!(actual.len(), expected.len(), "{name}: log entries {actual:?}");
    for ((message, kind), want) in actual.iter().zip(expected) {
        let want_message = want["message"].as_str().expect("golden log message");
        let want_class = want["exception"].as_str().expect("golden log exception");
        assert_eq!(normalize(message), want_message, "{name}: log message");
        assert!(
            class_matches(want_class, *kind),
            "{name}: log class {kind:?} is not {want_class}"
        );
    }
}

/// `"log": false` in a case input: the call gets no log.
fn wants_log(input: &Value) -> bool {
    input.get("log").and_then(Value::as_bool).unwrap_or(true)
}

// ---- disk-cache-prune --------------------------------------------------------------------------------------------

#[cfg(windows)]
#[test]
fn disk_cache_prune_golden_matches_csharp() {
    // Golden: tests/golden/disk-cache-prune.json (27 cases, oracle-cs disk-cache-prune). Static PruneDirectory.
    let golden = golden::load("disk-cache-prune");
    for case in &golden.cases {
        let name = name_of(case);
        let input = &case["input"];
        let folder = CaseFolder::new("disk-cache-prune");
        let pattern = input["pattern"].as_str().expect("pattern");
        let max_bytes = input["max_bytes"].as_i64().expect("max_bytes");

        let mut locks = Vec::new();
        let mut first_file = None;
        for entry in input["files"].as_array().expect("files") {
            let path = build_entry(folder.path(), entry);
            if flag(entry, "locked") {
                locks.push(lock_exclusive(&path));
            }
            if first_file.is_none() {
                first_file = Some(path);
            }
        }
        let target = match input.get("target").and_then(Value::as_str) {
            None => folder.path().to_path_buf(),
            Some("missing") => folder.path().join("missing"),
            Some("file") => first_file.expect("a file target needs a file entry"),
            Some(other) => panic!("{name}: unknown target {other}"),
        };

        let log = KindLog::default();
        let log_arg: Option<&dyn Log> = if wants_log(input) { Some(&log) } else { None };
        let result = prune_directory(&target, pattern, max_bytes, log_arg);

        let expected_remaining = &case["remaining"];
        if let Some(class) = expected_remaining.get("throws").and_then(Value::as_str) {
            let err = result.expect_err(name);
            assert!(class_matches(class, Some(err.kind())), "{name}: {err:?} is not {class}");
        } else {
            let remaining = result.unwrap_or_else(|e| panic!("{name}: prune failed: {e}"));
            assert_eq!(Some(remaining), expected_remaining.as_i64(), "{name}: remaining");
        }

        assert_eq!(
            target.exists(),
            flag(case, "target_exists_after"),
            "{name}: target_exists_after"
        );
        drop(locks);
        assert_eq!(
            survivors(folder.path()),
            string_array(&case["survivors"]),
            "{name}: survivors"
        );
        check_log(name, &log.entries(), case["log"].as_array().expect("log array"));
    }
}

// ---- disk-cache-passes -------------------------------------------------------------------------------------------

#[cfg(windows)]
#[test]
fn disk_cache_passes_golden_matches_csharp() {
    // Golden: tests/golden/disk-cache-passes.json (17 cases). An instance driven step by step; the trace checks
    // FullScanCount and the survivors after the steps listed in `trace` (pass_n runs `count` passes).
    let golden = golden::load("disk-cache-passes");
    for case in &golden.cases {
        let name = name_of(case);
        let input = &case["input"];
        let folder = CaseFolder::new("disk-cache-passes");
        let store_dir = if flag(input, "dir_missing") {
            folder.path().join("missing")
        } else {
            folder.path().to_path_buf()
        };
        let pattern = input["pattern"].as_str().expect("pattern");
        let max_bytes = input["max_bytes"].as_i64().expect("max_bytes");
        let log = Arc::new(KindLog::default());
        let store = DiskCacheStore::new(
            store_dir.clone(),
            pattern,
            max_bytes,
            Some(log.clone() as Arc<dyn Log>),
            None,
        )
        .unwrap_or_else(|e| panic!("{name}: store: {e:?}"));

        let trace: HashMap<u64, &Value> = case["trace"]
            .as_array()
            .expect("trace")
            .iter()
            .map(|t| (t["step"].as_u64().expect("trace step"), t))
            .collect();
        let mut locks: BTreeMap<String, File> = BTreeMap::new();

        for (index, step) in input["steps"].as_array().expect("steps").iter().enumerate() {
            let index = u64::try_from(index).expect("index fits");
            let op = step["op"].as_str().expect("op");
            let expected = trace.get(&index).copied();
            match op {
                "file" => {
                    build_entry(&store_dir, step);
                }
                "pass" => {
                    let result = store.run_prune_pass();
                    match expected.and_then(|t| t.get("throws")).and_then(Value::as_str) {
                        Some(class) => {
                            let err = result.expect_err(name);
                            assert!(class_matches(class, Some(err.kind())), "{name}: step {index}: {err:?}");
                        }
                        None => result.unwrap_or_else(|e| panic!("{name}: step {index}: pass failed: {e}")),
                    }
                }
                "pass_n" => {
                    for _ in 0..step["count"].as_u64().expect("count") {
                        store
                            .run_prune_pass()
                            .unwrap_or_else(|e| panic!("{name}: step {index}: pass_n failed: {e}"));
                    }
                }
                "note_written" => store.note_written(&store_dir.join(step["name"].as_str().expect("name"))),
                "clear" => store
                    .clear_directory()
                    .unwrap_or_else(|e| panic!("{name}: step {index}: clear failed: {e}")),
                "lock" => {
                    let file_name = step["name"].as_str().expect("name").to_owned();
                    let handle = lock_exclusive(&store_dir.join(&file_name));
                    locks.insert(file_name, handle);
                }
                "unlock" => {
                    locks.remove(step["name"].as_str().expect("name"));
                }
                "delete" => fs::remove_file(store_dir.join(step["name"].as_str().expect("name")))
                    .unwrap_or_else(|e| panic!("{name}: step {index}: delete failed: {e}")),
                other => panic!("{name}: unknown op {other}"),
            }
            if let Some(t) = expected {
                assert_eq!(
                    i64::from(store.full_scan_count()),
                    t["full_scan_count"].as_i64().expect("full_scan_count"),
                    "{name}: step {index} full_scan_count"
                );
                assert_eq!(
                    survivors(&store_dir),
                    string_array(&t["survivors"]),
                    "{name}: step {index} survivors"
                );
            }
        }
        drop(locks);
        check_log(name, &log.entries(), case["log"].as_array().expect("log array"));
    }
}

// ---- disk-cache-note-accessed ------------------------------------------------------------------------------------

#[cfg(windows)]
#[test]
fn disk_cache_note_accessed_golden_matches_csharp() {
    // Golden: tests/golden/disk-cache-note-accessed.json (12 cases). Injected clock; access times read back.
    let golden = golden::load("disk-cache-note-accessed");
    for case in &golden.cases {
        let name = name_of(case);
        let folder = CaseFolder::new("disk-cache-note-accessed");
        let clock = Arc::new(FakeClock::at(utc("2020-01-02T03:04:05Z")));
        let store = DiskCacheStore::new(
            folder.path().to_path_buf(),
            "*.png",
            1_000_000,
            None,
            Some(Arc::clone(&clock) as Arc<dyn Clock>),
        )
        .unwrap_or_else(|e| panic!("{name}: store: {e:?}"));

        let mut reads: Vec<(u64, String, String)> = Vec::new();
        for (index, step) in case["input"]["steps"].as_array().expect("steps").iter().enumerate() {
            let index = u64::try_from(index).expect("index fits");
            match step["op"].as_str().expect("op") {
                "file" => {
                    build_entry(folder.path(), step);
                }
                "dir" => fs::create_dir_all(folder.path().join(step["name"].as_str().expect("name")))
                    .unwrap_or_else(|e| panic!("{name}: mkdir: {e}")),
                "access" => store.note_accessed(&folder.path().join(step["name"].as_str().expect("name"))),
                "access_many" => {
                    let prefix = step["prefix"].as_str().expect("prefix");
                    for i in 0..step["count"].as_u64().expect("count") {
                        store.note_accessed(&folder.path().join(format!("{prefix}{i}.png")));
                    }
                }
                "stamp" => stamp(
                    &folder.path().join(step["name"].as_str().expect("name")),
                    opt_utc(step, "access"),
                    None,
                    None,
                ),
                "clock" => clock.set(utc(step["now"].as_str().expect("now"))),
                "read" => {
                    let file_name = step["name"].as_str().expect("name").to_owned();
                    let json = access_json(&folder.path().join(&file_name));
                    reads.push((index, file_name, json));
                }
                other => panic!("{name}: unknown op {other}"),
            }
        }

        let expected = case["reads"].as_array().expect("reads");
        assert_eq!(reads.len(), expected.len(), "{name}: reads {reads:?}");
        for ((step, file_name, json), want) in reads.iter().zip(expected) {
            assert_eq!(*step, want["step"].as_u64().expect("step"), "{name}: read step");
            assert_eq!(file_name, want["name"].as_str().expect("name"), "{name}: read name");
            assert_eq!(
                json,
                want["access"].as_str().expect("access"),
                "{name}: access of {file_name}"
            );
        }
    }
}

// ---- disk-cache-stale-temp ---------------------------------------------------------------------------------------

#[cfg(windows)]
#[test]
fn disk_cache_stale_temp_golden_matches_csharp() {
    // Golden: tests/golden/disk-cache-stale-temp.json (15 cases). Static DeleteStaleTempFiles with explicit write
    // times and utc_now.
    let golden = golden::load("disk-cache-stale-temp");
    for case in &golden.cases {
        let name = name_of(case);
        let input = &case["input"];
        let folder = CaseFolder::new("disk-cache-stale-temp");
        for entry in input["files"].as_array().expect("files") {
            build_entry(folder.path(), entry);
        }
        let dir = match input.get("target").and_then(Value::as_str) {
            None => folder.path().to_path_buf(),
            Some("missing") => folder.path().join("missing"),
            Some(other) => panic!("{name}: unknown target {other}"),
        };
        let utc_now = utc(input["utc_now"].as_str().expect("utc_now"));
        let max_files = i32::try_from(input["max_files"].as_i64().expect("max_files")).expect("max_files fits");

        let log = KindLog::default();
        let removed = delete_stale_temp_files(&dir, utc_now, max_files, Some(&log));

        assert_eq!(
            i64::from(removed),
            case["removed"].as_i64().expect("removed"),
            "{name}: removed"
        );
        assert_eq!(
            survivors(folder.path()),
            string_array(&case["survivors"]),
            "{name}: survivors"
        );
        check_log(name, &log.entries(), case["log"].as_array().expect("log array"));
    }
}

// ---- disk-cache-delete -------------------------------------------------------------------------------------------

#[cfg(windows)]
#[test]
fn disk_cache_delete_golden_matches_csharp() {
    // Golden: tests/golden/disk-cache-delete.json (15 cases). try_delete, static and instance ClearDirectory.
    let golden = golden::load("disk-cache-delete");
    for case in &golden.cases {
        let name = name_of(case);
        let input = &case["input"];
        let folder = CaseFolder::new("disk-cache-delete");
        let mut locks = Vec::new();
        for entry in input["files"].as_array().expect("files") {
            let path = build_entry(folder.path(), entry);
            if flag(entry, "locked") {
                locks.push(lock_exclusive(&path));
            }
        }
        let log = Arc::new(KindLog::default());
        let log_arg: Option<&dyn Log> = if wants_log(input) {
            Some(log.as_ref() as &dyn Log)
        } else {
            None
        };
        let op = input["op"].as_str().expect("op");

        match op {
            "try_delete" => {
                let target = input["target"].as_str().expect("target");
                let result = try_delete(&folder.path().join(target), log_arg);
                assert_eq!(Some(result), case["result"].as_bool(), "{name}: result");
            }
            "clear_static" => {
                let pattern = input["pattern"].as_str().expect("pattern");
                let result = clear_directory_matching(folder.path(), pattern, log_arg);
                match case.get("throws").and_then(Value::as_str) {
                    Some(class) => {
                        let err = result.expect_err(name);
                        assert!(class_matches(class, Some(err.kind())), "{name}: {err:?}");
                    }
                    None => result.unwrap_or_else(|e| panic!("{name}: clear failed: {e}")),
                }
            }
            "clear_instance" => {
                // The instance always logs through its own log (no `log` flag in these cases).
                let pattern = input["pattern"].as_str().expect("pattern");
                let store = DiskCacheStore::new(
                    folder.path().to_path_buf(),
                    pattern,
                    0,
                    Some(log.clone() as Arc<dyn Log>),
                    None,
                )
                .unwrap_or_else(|e| panic!("{name}: store: {e:?}"));
                let result = store.clear_directory();
                assert!(result.is_ok(), "{name}: clear_directory: {result:?}");
            }
            other => panic!("{name}: unknown op {other}"),
        }
        drop(locks);
        assert_eq!(
            survivors(folder.path()),
            string_array(&case["survivors"]),
            "{name}: survivors"
        );
        check_log(name, &log.entries(), case["log"].as_array().expect("log array"));
    }
}

// ---- cache-entry-failure -----------------------------------------------------------------------------------------

/// The `DecodeError` variant each C# exception type maps to (contract semantics note 9; the golden is keyed by the
/// C# type name). An unknown type panics, so a new golden row must be mapped on purpose.
fn decode_error_for(type_name: &str) -> DecodeError {
    let msg = type_name.to_owned();
    match type_name {
        "Exception" | "NullReferenceException" | "TimeoutException" => DecodeError::Other { message: msg },
        "IOException" => DecodeError::Io(io::Error::other(msg)),
        "FileNotFoundException" => DecodeError::FileNotFound { message: msg },
        "DirectoryNotFoundException" => DecodeError::DirectoryNotFound { message: msg },
        "EndOfStreamException" => DecodeError::Io(io::Error::from(io::ErrorKind::UnexpectedEof)),
        "PathTooLongException" => DecodeError::Io(io::Error::other(msg)),
        "DecoderMemoryAdmissionException" => DecodeError::MemoryAdmission { message: msg },
        "UnauthorizedAccessException" => DecodeError::Io(io::Error::from(io::ErrorKind::PermissionDenied)),
        "NotSupportedException" | "PlatformNotSupportedException" => DecodeError::NotSupported {
            message: msg,
            user_text: None,
            inner_hresult: None,
        },
        "MissingImageCodecException" => DecodeError::MissingCodec {
            message: msg,
            user_text: None,
        },
        "FormatException" => DecodeError::Format { message: msg },
        "InvalidDataException" => DecodeError::InvalidData { message: msg },
        "ArgumentException" | "ArgumentNullException" | "ArgumentOutOfRangeException" => {
            DecodeError::InvalidArgument { message: msg }
        }
        "InvalidOperationException" => DecodeError::InvalidOperation { message: msg },
        "ObjectDisposedException" => DecodeError::Disposed { message: msg },
        "OverflowException" => DecodeError::Overflow { message: msg },
        "InvalidCastException" => DecodeError::InvalidCast { message: msg },
        "COMException" => DecodeError::Com {
            hresult: 0x8000_4005_u32 as i32,
            message: msg,
        },
        "COMException-E_OUTOFMEMORY" => DecodeError::Com {
            hresult: 0x8007_000E_u32 as i32,
            message: msg,
        },
        "OperationCanceledException" | "TaskCanceledException" => DecodeError::Canceled,
        "OutOfMemoryException" | "InsufficientMemoryException" => DecodeError::OutOfMemory { message: msg },
        "DecoderBusyException" => DecodeError::Busy { message: msg },
        "DllNotFoundException" => DecodeError::DllNotFound { message: msg },
        "BadImageFormatException" => DecodeError::BadImageFormat { message: msg },
        "EntryPointNotFoundException" => DecodeError::EntryPointNotFound { message: msg },
        other => panic!("golden exception type {other} has no DecodeError mapping"),
    }
}

#[test]
fn cache_entry_failure_golden_matches_csharp() {
    // Golden: tests/golden/cache-entry-failure.json (32 cases, internal IsCacheEntryFailure by reflection). Pure: runs
    // on every platform.
    let golden = golden::load("cache-entry-failure");
    for case in &golden.cases {
        let name = name_of(case);
        let type_name = case["type"].as_str().expect("type");
        let error = decode_error_for(type_name);
        let expected = case["is_failure"].as_bool().expect("is_failure");
        assert_eq!(is_cache_entry_failure(&error), expected, "{name} ({type_name})");
    }
}
