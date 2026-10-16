//! Port of the C# `AtomicCacheFile` tests (`tests/PhotoReview.Imaging.Tests/Caching/`, baseline d92d6aae):
//! `CacheTempFileAndDiskProbeTests`, `ErrorHandlingReviewCachingTests` and the two `WP-04` tests of
//! `WicCacheImageIoTests` that only use `AtomicCacheFile`. The WIC tests of that file are W4-11e, not here.
//! Golden checks run against `tests/golden/atomic-cache-file.json` (oracle command `atomic-cache-file`, 17 cases) with
//! the setup steps read from `tests/golden-inputs/atomic-cache-file.json`.
//!
//! Port of `PhotoReview.Imaging/Caching/AtomicCacheFile.cs` (C# baseline d92d6aae).
//!
//! Red phase (W4-08a): the bodies in `crates/pr-imaging/src/cache/atomic_file.rs` are `todo!()`, so these tests compile
//! against the stub and FAIL until the porter implements the unit. They use real files inside a `TempRoot` (removed on
//! drop), so none is `#[ignore]`d.
//!
//! Documented mappings (contract W4-08a, "Type mapping"):
//! - C# `OperationCanceledException` is `AtomicWriteError::Canceled`.
//! - C# `IOException` (and `UnauthorizedAccessException`) is any `AtomicWriteError::Io`.
//! - C# `InvalidOperationException("boom")` from the payload is `Io` with a message of `boom`; the Rust payload returns
//!   `io::Error::other("boom")`.
//! - Not ported: the `Delete failed` log assertion of `TryDelete` (no portable way to lock a temp file in a unit test;
//!   the cleanup is covered by the golden tree comparison instead).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use pr_core::sync::CancelToken;
use pr_imaging::cache::atomic_file::{
    AtomicWriteError, TEMP_FILE_SUFFIX, WRITE_BUFFER_SIZE, temporary_path_for, write_atomic, write_atomic_with_temp,
};
use pr_test_support::TempRoot;
use pr_test_support::golden::{self, str_of};
use serde_json::Value;

const GOLDEN: &str = "atomic-cache-file";

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

/// Lower-case hex of `bytes` (the golden tree format).
fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    out
}

/// Bytes of a hex string as written in the golden input (`"010203"`, `""`).
fn from_hex(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0, "odd hex length in {text:?}");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex byte"))
        .collect()
}

/// Golden tree name of one path component: `entry.pv4.<32 hex>.tmp` becomes `entry.pv4.<GUID>.tmp`.
fn normalise_name(name: &str) -> String {
    let Some(stem) = name.strip_suffix(TEMP_FILE_SUFFIX) else {
        return name.to_owned();
    };
    let Some(dot) = stem.rfind('.') else {
        return name.to_owned();
    };
    let guid = &stem[dot + 1..];
    if guid.len() == 32 && guid.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        format!("{}.<GUID>{TEMP_FILE_SUFFIX}", &stem[..dot])
    } else {
        name.to_owned()
    }
}

fn collect_tree(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display())) {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        let rel = path.strip_prefix(root).expect("path under the golden root");
        let key = rel
            .components()
            .map(|c| normalise_name(&c.as_os_str().to_string_lossy()))
            .collect::<Vec<_>>()
            .join("/");
        if entry.file_type().expect("file type").is_dir() {
            out.insert(format!("{key}/"), "<dir>".to_owned());
            collect_tree(root, &path, out);
        } else {
            let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let value = if bytes.len() > 32 {
                format!("<{} bytes>", bytes.len())
            } else {
                to_hex(&bytes)
            };
            out.insert(key, value);
        }
    }
}

/// The final tree of `root` in the golden format: `rel/` for a folder, hex for a file of at most 32 bytes,
/// `<N bytes>` for a bigger one, `.<GUID>.tmp` names normalised.
fn snapshot(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    collect_tree(root, root, &mut out);
    out
}

/// Expected tree of a golden case.
fn expected_tree(tree: &Value) -> BTreeMap<String, String> {
    tree.as_object()
        .expect("golden tree is an object")
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().expect("golden tree value is a string").to_owned()))
        .collect()
}

/// `tests/golden-inputs/<name>.json`: the setup of every golden case.
fn load_input(name: &str) -> Value {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// Bytes a golden `write` step puts into its payload (`repeat` copies of `hex`).
fn payload_bytes(step: &Value) -> Vec<u8> {
    let repeat = step
        .get("repeat")
        .and_then(Value::as_u64)
        .map_or(1, |n| usize::try_from(n).expect("repeat fits in usize"));
    from_hex(step.get("hex").and_then(Value::as_str).unwrap_or("")).repeat(repeat)
}

/// Runs one golden `write` step on `root`. The payload kinds are `ok`, `throw`, `throw_after_write` and
/// `cancel_after_write` (the golden input's `payload` field).
fn run_write(root: &Path, step: &Value) -> Result<(), AtomicWriteError> {
    let target = root.join(str_of(step, "target"));
    let kind = str_of(step, "payload");
    let bytes = payload_bytes(step);
    let token = CancelToken::new();
    if step.get("cancel_before").and_then(Value::as_bool).unwrap_or(false) {
        token.cancel();
    }
    let payload_token = token.clone();
    let payload = move |w: &mut dyn Write| -> io::Result<()> {
        match kind {
            "ok" => w.write_all(&bytes),
            "throw" => Err(io::Error::other("boom")),
            "throw_after_write" => {
                w.write_all(&bytes)?;
                Err(io::Error::other("boom"))
            }
            "cancel_after_write" => {
                w.write_all(&bytes)?;
                payload_token.cancel();
                Ok(())
            }
            other => panic!("unknown golden payload kind {other}"),
        }
    };
    match step.get("temp").and_then(Value::as_str) {
        Some(temp) => {
            let temp_path = target.parent().expect("target has a folder").join(temp);
            write_atomic_with_temp(&target, payload, None, Some(&temp_path), &token)
        }
        None => write_atomic(&target, payload, None, &token),
    }
}

/// Compares one step outcome with the golden `outcome` / `type` fields.
fn check_outcome(case: &str, golden_step: &Value, result: &Result<(), AtomicWriteError>) {
    match str_of(golden_step, "outcome") {
        "ok" => assert!(result.is_ok(), "case {case}: expected ok, got {result:?}"),
        "throws" => {
            let ty = str_of(golden_step, "type");
            let matched = match (ty, result) {
                ("OperationCanceledException", Err(AtomicWriteError::Canceled)) => true,
                ("IOException", Err(AtomicWriteError::Io(_))) => true,
                ("InvalidOperationException", Err(AtomicWriteError::Io(e))) => e.to_string() == "boom",
                _ => false,
            };
            assert!(matched, "case {case}: expected {ty}, got {result:?}");
        }
        other => panic!("case {case}: unknown golden outcome {other}"),
    }
}

/// Runs one golden step: setup (`precreate`, `mkdir`) or a `write` with its outcome check.
fn run_step(root: &TempRoot, golden_step: &Value, input_step: &Value, case: &str) {
    match str_of(golden_step, "op") {
        "precreate" => {
            let path = root.join(str_of(input_step, "path"));
            fs::create_dir_all(path.parent().expect("precreate path has a folder")).expect("create folder");
            fs::write(&path, from_hex(str_of(input_step, "hex"))).expect("precreate file");
        }
        "mkdir" => fs::create_dir_all(root.join(str_of(input_step, "path"))).expect("mkdir"),
        "write" => {
            let result = run_write(root.path(), input_step);
            check_outcome(case, golden_step, &result);
        }
        other => panic!("case {case}: unknown golden op {other}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Ports of the C# tests
// ---------------------------------------------------------------------------------------------------------------------

// C#: CacheTempFileAndDiskProbeTests.AtomicCacheFileWriteAsync_PreExistingTempPath_FailsAndLeavesNoCacheEntry
#[test]
fn write_atomic_pre_existing_temp_path_fails_and_leaves_no_cache_entry() {
    let root = TempRoot::new("pre-existing-temp");
    let cache = root.join("entry.pv4");
    let temp = root.join("entry.pv4.fixed.tmp");
    fs::write(&temp, [9u8, 9, 9]).expect("pre-create the temp file");

    let result = write_atomic_with_temp(
        &cache,
        |w| w.write_all(&[1, 2, 3]),
        None,
        Some(&temp),
        &CancelToken::new(),
    );

    match result {
        Err(AtomicWriteError::Io(e)) => assert_eq!(e.kind(), io::ErrorKind::AlreadyExists, "error: {e}"),
        other => panic!("expected Io(AlreadyExists), got {other:?}"),
    }
    assert!(!cache.exists(), "no cache entry may be published");
    assert_eq!(fs::read(&temp).expect("pre-existing temp still there"), vec![9, 9, 9]);
}

// C#: CacheTempFileAndDiskProbeTests.AtomicCacheFileWriteAsync_PayloadThrows_CleansUpTempFile
#[test]
fn write_atomic_payload_throws_cleans_up_temp_file() {
    let root = TempRoot::new("payload-throws");
    let cache = root.join("entry.pv4");

    let result = write_atomic(&cache, |_w| Err(io::Error::other("boom")), None, &CancelToken::new());

    match result {
        Err(AtomicWriteError::Io(e)) => assert_eq!(e.to_string(), "boom"),
        other => panic!("expected Io(boom), got {other:?}"),
    }
    assert!(!cache.exists(), "no cache entry may be published");
    assert!(snapshot(root.path()).is_empty(), "the temp file must be removed");
}

// C#: ErrorHandlingReviewCachingTests.AtomicWrite_CancelledUpFront_WritesNothing
#[test]
fn write_atomic_cancelled_up_front_writes_nothing() {
    let root = TempRoot::new("cancelled-up-front");
    let cache = root.join("sub").join("entry.pv4");
    let token = CancelToken::new();
    token.cancel();

    let result = write_atomic(&cache, |w| w.write_all(&[1]), None, &token);

    assert!(matches!(result, Err(AtomicWriteError::Canceled)), "got {result:?}");
    assert!(!cache.exists(), "no cache file");
    assert!(!root.join("sub").exists(), "not even the folder may be created");
}

// C#: ErrorHandlingReviewCachingTests.AtomicWrite_CancelledBeforeMove_DoesNotPublish
#[test]
fn write_atomic_cancelled_before_move_does_not_publish() {
    let root = TempRoot::new("cancelled-before-move");
    let cache = root.join("entry.pv4");
    let token = CancelToken::new();
    let payload_token = token.clone();

    let result = write_atomic(
        &cache,
        move |w| {
            w.write_all(&[1])?;
            payload_token.cancel();
            Ok(())
        },
        None,
        &token,
    );

    assert!(matches!(result, Err(AtomicWriteError::Canceled)), "got {result:?}");
    assert!(!cache.exists(), "a superseded write must not publish");
    assert!(snapshot(root.path()).is_empty(), "the folder is empty");
}

// C#: WicCacheImageIoTests.AtomicWrite_ReplacesExistingEntry (WP-04)
#[test]
fn write_atomic_replaces_existing_entry() {
    let root = TempRoot::new("replaces-existing");
    let cache = root.join("entry.pv4");

    write_atomic(&cache, |w| w.write_all(&[1, 2, 3]), None, &CancelToken::new()).expect("first write");
    write_atomic(&cache, |w| w.write_all(&[9, 8]), None, &CancelToken::new()).expect("second write");

    assert_eq!(fs::read(&cache).expect("read entry"), vec![9, 8]);
    assert_eq!(
        fs::read_dir(root.path()).expect("list folder").count(),
        1,
        "exactly one file, no temp left"
    );
}

// C#: WicCacheImageIoTests.AtomicWrite_CancelledDuringPayload_PublishesNothing (WP-04)
#[test]
fn write_atomic_cancelled_during_payload_publishes_nothing() {
    let root = TempRoot::new("cancelled-during-payload");
    let cache = root.join("entry.pv4");
    let token = CancelToken::new();
    let payload_token = token.clone();

    let result = write_atomic(
        &cache,
        move |w| {
            payload_token.cancel();
            w.write_all(&[7])
        },
        None,
        &token,
    );

    assert!(matches!(result, Err(AtomicWriteError::Canceled)), "got {result:?}");
    assert!(!cache.exists(), "nothing published");
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden-driven tests (tests/golden/atomic-cache-file.json)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn golden_atomic_cache_file_every_case_matches_csharp() {
    let expected = golden::load(GOLDEN);
    assert_eq!(expected.cases.len(), 17, "the golden has 17 cases");
    let input = load_input(GOLDEN);
    let inputs: BTreeMap<&str, &Value> = input["cases"]
        .as_array()
        .expect("input cases")
        .iter()
        .map(|c| (str_of(c, "name"), c))
        .collect();

    for case in &expected.cases {
        let name = str_of(case, "name");
        let setup = inputs
            .get(name)
            .unwrap_or_else(|| panic!("no golden input for case {name}"));
        let golden_steps = case["steps"].as_array().expect("golden steps");
        let input_steps = setup["steps"].as_array().expect("input steps");
        assert_eq!(golden_steps.len(), input_steps.len(), "case {name}: step count");

        let root = TempRoot::new(name);
        for (golden_step, input_step) in golden_steps.iter().zip(input_steps) {
            run_step(&root, golden_step, input_step, name);
        }
        assert_eq!(
            snapshot(root.path()),
            expected_tree(&case["tree"]),
            "case {name}: final tree"
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Extra tests (contract W4-08a, "Done when")
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn temporary_path_for_appends_guid_and_tmp_suffix() {
    assert_eq!(temporary_path_for(r"C:\c\e.pv4", "ab"), r"C:\c\e.pv4.ab.tmp");
    assert_eq!(TEMP_FILE_SUFFIX, ".tmp");
}

#[test]
fn write_atomic_temp_name_is_guid_tmp_in_the_target_folder() {
    let root = TempRoot::new("temp-name");
    let cache = root.join("entry.pv4");
    let folder: PathBuf = root.path().to_path_buf();
    let seen: RefCell<Vec<String>> = RefCell::new(Vec::new());

    write_atomic(
        &cache,
        |w| {
            for entry in fs::read_dir(&folder).expect("list the folder") {
                let name = entry.expect("entry").file_name().to_string_lossy().into_owned();
                seen.borrow_mut().push(name);
            }
            w.write_all(&[1, 2, 3])
        },
        None,
        &CancelToken::new(),
    )
    .expect("write");

    let names = seen.into_inner();
    let temps: Vec<&String> = names.iter().filter(|n| n.ends_with(TEMP_FILE_SUFFIX)).collect();
    assert_eq!(temps.len(), 1, "one temp file while the payload runs: {names:?}");
    let guid = temps[0]
        .strip_prefix("entry.pv4.")
        .and_then(|s| s.strip_suffix(TEMP_FILE_SUFFIX));
    assert!(
        matches!(guid, Some(g) if g.len() == 32 && g.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))),
        "temp name {:?} is not <cache>.<guid N>.tmp",
        temps[0]
    );
    assert_eq!(
        snapshot(root.path()).into_keys().collect::<Vec<_>>(),
        ["entry.pv4".to_owned()],
        "the temp is gone after the rename"
    );
}

#[test]
fn write_atomic_payload_larger_than_buffer_is_written_intact() {
    let root = TempRoot::new("large-payload");
    let cache = root.join("entry.pv4");
    let len = WRITE_BUFFER_SIZE * 3 + 7;
    let expected: Vec<u8> = (0..len).map(|i| u8::try_from(i % 251).expect("below 251")).collect();
    let payload = expected.clone();

    write_atomic(&cache, |w| w.write_all(&payload), None, &CancelToken::new()).expect("write");

    assert_eq!(fs::read(&cache).expect("read entry"), expected);
}

#[test]
fn write_atomic_path_without_directory_part_is_invalid_input() {
    // C# ArgumentException from Path.GetDirectoryName: a path with no folder part (empty parent) is refused.
    let result = write_atomic(Path::new(""), |w| w.write_all(&[1]), None, &CancelToken::new());

    match result {
        Err(AtomicWriteError::Io(e)) => assert_eq!(e.kind(), io::ErrorKind::InvalidInput, "error: {e}"),
        other => panic!("expected Io(InvalidInput), got {other:?}"),
    }
}

#[test]
fn write_atomic_temp_override_in_a_missing_folder_fails_not_found() {
    // The override is used verbatim: only the cache path's folder is created, never the temp override's folder.
    let root = TempRoot::new("override-missing-folder");
    let cache = root.join("entry.pv4");
    let temp = root.join("missing").join("entry.pv4.tmp");

    let result = write_atomic_with_temp(&cache, |w| w.write_all(&[1]), None, Some(&temp), &CancelToken::new());

    match result {
        Err(AtomicWriteError::Io(e)) => assert_eq!(e.kind(), io::ErrorKind::NotFound, "error: {e}"),
        other => panic!("expected Io(NotFound), got {other:?}"),
    }
    assert!(!cache.exists(), "no cache entry");
    assert!(
        !root.join("missing").exists(),
        "the override's folder must not be created"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Repair round 1 (verifier findings)
// ---------------------------------------------------------------------------------------------------------------------

fn tmp_files(dir: &Path) -> usize {
    fs::read_dir(dir)
        .map(|d| {
            d.filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().ends_with(TEMP_FILE_SUFFIX))
                .count()
        })
        .unwrap_or(0)
}

// C#: `finally { TryDelete(temp) }` runs when the payload throws; a Rust panic must not leave the temp behind.
#[test]
fn write_atomic_payload_panic_removes_temp_file() {
    let root = TempRoot::new("w408a-panic");
    let cache = root.path().join("c.bin");
    let target = cache.clone();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _ = write_atomic(
            &target,
            |w: &mut dyn Write| {
                w.write_all(b"x")?;
                panic!("boom")
            },
            None,
            &CancelToken::new(),
        );
    }));
    assert!(caught.is_err());
    assert_eq!(tmp_files(root.path()), 0, "temp file left behind after a payload panic");
    assert!(!cache.exists());
}

// C#: `GetDirectoryName("root\sub\")` is `root\sub`, created before the move fails.
#[cfg(windows)]
#[test]
fn write_atomic_trailing_separator_creates_the_directory_then_fails() {
    let root = TempRoot::new("w408a-trail");
    let mut name = root.path().join("sub").into_os_string();
    name.push("\\");
    let path = PathBuf::from(name);
    let result = write_atomic(&path, |w: &mut dyn Write| w.write_all(b"x"), None, &CancelToken::new());
    assert!(matches!(result, Err(AtomicWriteError::Io(_))));
    assert!(root.path().join("sub").is_dir());
    assert_eq!(tmp_files(&root.path().join("sub")), 0);
}

// C#: `File.Move(overwrite)` is denied while a reader holds the target open, even with FileShare.Delete.
#[cfg(windows)]
#[test]
fn write_atomic_overwrite_fails_while_target_is_open_with_share_delete() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = TempRoot::new("w408a-share");
    let cache = root.path().join("t.bin");
    fs::write(&cache, [1u8]).unwrap();
    let held = fs::OpenOptions::new().read(true).share_mode(7).open(&cache).unwrap();
    let result = write_atomic(
        &cache,
        |w: &mut dyn Write| w.write_all(b"new"),
        None,
        &CancelToken::new(),
    );
    drop(held);
    assert!(matches!(result, Err(AtomicWriteError::Io(_))));
    assert_eq!(fs::read(&cache).unwrap(), vec![1u8]);
    assert_eq!(tmp_files(root.path()), 0);
}

// Kills the "relative file name without directory" guard mutant: nothing may be written in the current folder.
#[test]
fn write_atomic_relative_file_name_without_directory_is_an_error() {
    let name = "w408a_rel_name_check.bin";
    let result = write_atomic(
        Path::new(name),
        |w: &mut dyn Write| w.write_all(b"x"),
        None,
        &CancelToken::new(),
    );
    let created = Path::new(name).exists();
    let _ = fs::remove_file(name);
    match result {
        Err(AtomicWriteError::Io(e)) => assert_eq!(e.kind(), io::ErrorKind::InvalidInput),
        other => panic!("expected InvalidInput, got {other:?}"),
    }
    assert!(!created);
}

#[test]
fn write_buffer_size_is_64_kib() {
    assert_eq!(WRITE_BUFFER_SIZE, 65536);
}

#[test]
fn atomic_write_error_display_and_source() {
    use std::error::Error;
    assert_eq!(AtomicWriteError::Canceled.to_string(), "operation canceled");
    assert!(AtomicWriteError::Canceled.source().is_none());
    let io_error = AtomicWriteError::Io(io::Error::other("boom"));
    assert_eq!(io_error.to_string(), "boom");
    assert_eq!(io_error.source().unwrap().to_string(), "boom");
}

// ---------------------------------------------------------------------------------------------------------------------
// Repair round 3: long paths (C# `Path.GetFullPath` makes any path long-path capable; `MoveFileExW` alone is not)
// ---------------------------------------------------------------------------------------------------------------------

fn tmp_count_in(dir: &Path) -> usize {
    tmp_files(dir)
}

fn long_dir(base: &Path, total: usize) -> PathBuf {
    let mut p = base.to_path_buf();
    let mut i = 0;
    while p.as_os_str().len() < total {
        p.push(format!("segment_{i:03}_xxxxxxxxxxxxxxxxxxxxxxxx"));
        i += 1;
    }
    p
}

fn write_abc(path: &Path) -> Result<(), AtomicWriteError> {
    write_atomic(path, |w: &mut dyn Write| w.write_all(b"abc"), None, &CancelToken::new())
}

/// Removes a folder created relative to the current directory when dropped (also when the test fails).
struct RelativeCleanup(PathBuf);

impl Drop for RelativeCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn write_atomic_long_absolute_path_is_published() {
    let root = TempRoot::new("w408a-long");
    let dir = long_dir(root.path(), 270);
    let cache = dir.join("c.bin");

    write_abc(&cache).expect("write");

    assert_eq!(fs::read(&cache).unwrap(), b"abc");
    assert_eq!(tmp_count_in(&dir), 0);
}

#[cfg(windows)]
#[test]
fn write_atomic_long_verbatim_path_is_published() {
    let root = TempRoot::new("w408a-long-verbatim");
    let dir = long_dir(root.path(), 270);
    let cache = PathBuf::from(format!("{}{}", VERBATIM_PREFIX, dir.display())).join("c.bin");

    write_abc(&cache).expect("write");

    assert_eq!(fs::read(&cache).unwrap(), b"abc");
}

// Destination below 260 units, destination plus `.<32 hex>.tmp` at or over it.
#[test]
fn write_atomic_only_the_temp_name_pushes_the_path_over_260() {
    let root = TempRoot::new("w408a-long-temp");
    let dir = long_dir(root.path(), 200);
    let name_len = 240usize.saturating_sub(dir.as_os_str().len() + 1).max(1);
    let cache = dir.join("n".repeat(name_len));

    write_abc(&cache).expect("write");

    assert_eq!(fs::read(&cache).unwrap(), b"abc");
    assert_eq!(tmp_count_in(&dir), 0);
}

// C# `Path.GetFullPath` resolves a relative path against the current directory before any long-path handling. Folders
// are created under the current directory (the package folder under `cargo test`); no `set_current_dir` (process-wide).
#[test]
fn write_atomic_relative_path_of_260_plus_units_is_published() {
    let top = format!("w408a_rel_long_{}", std::process::id());
    let _cleanup = RelativeCleanup(PathBuf::from(&top));
    let dir = long_dir(Path::new(&top), 270);
    let cache = dir.join("c.bin");
    assert!(cache.is_relative() && cache.as_os_str().len() >= 260);

    write_abc(&cache).expect("write");

    assert_eq!(fs::read(&cache).unwrap(), b"abc");
    assert_eq!(tmp_count_in(&dir), 0);
}

// The relative text is short, only the absolute form (current directory + relative) is over the limit.
#[test]
fn write_atomic_short_relative_path_with_long_absolute_form_is_published() {
    let cwd = std::env::current_dir().expect("cwd");
    let top = format!("w408a_rel_abs_{}", std::process::id());
    let _cleanup = RelativeCleanup(PathBuf::from(&top));
    // Aim at an absolute form of about 275 units while the relative text stays at most 259.
    let wanted_relative = 275usize.saturating_sub(cwd.as_os_str().len() + 1);
    let dir = long_dir(Path::new(&top), wanted_relative.max(top.len() + 1));
    let cache = dir.join("c.bin");
    assert!(cache.as_os_str().len() < 260);
    assert!(
        cwd.join(&cache).as_os_str().len() >= 260,
        "cwd is too short for this test"
    );

    write_abc(&cache).expect("write");

    assert_eq!(fs::read(&cache).unwrap(), b"abc");
    assert_eq!(tmp_count_in(&dir), 0);
}

// Overwrite of an existing entry on the long relative path (rename-over, not only create).
#[test]
fn write_atomic_relative_long_path_replaces_existing_entry() {
    let top = format!("w408a_rel_repl_{}", std::process::id());
    let _cleanup = RelativeCleanup(PathBuf::from(&top));
    let dir = long_dir(Path::new(&top), 270);
    let cache = dir.join("c.bin");

    write_abc(&cache).expect("first write");
    write_atomic(
        &cache,
        |w: &mut dyn Write| w.write_all(b"zz"),
        None,
        &CancelToken::new(),
    )
    .expect("second write");

    assert_eq!(fs::read(&cache).unwrap(), b"zz");
    assert_eq!(tmp_count_in(&dir), 0);
}

#[cfg(windows)]
const VERBATIM_PREFIX: &str = r"\\?\";
