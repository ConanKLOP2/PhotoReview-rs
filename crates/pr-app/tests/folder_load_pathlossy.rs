//! FIX-PATHLOSSY: a file whose name holds an unpaired UTF-16 surrogate (legal on Windows) must never reach the catalog,
//! because a `String` path cannot name it and `to_string_lossy` would name ANOTHER file (a sibling really named with
//! U+FFFD), on which every later Move/Copy/Recycle/Undo would act. C# refuses such a path with `UnrepresentablePath`
//! before mutating anything (`OperationJournal.ThrowIfUnrepresentable`); here the entry is not loaded and is reported as
//! skipped. Real files in a `TempRoot`; Windows only.

#![cfg(windows)]

#[path = "common/folder_load_harness.rs"]
mod harness;

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;

use harness::*;

fn surrogate_name(stem: &str, extension: &str) -> OsString {
    let mut units: Vec<u16> = stem.encode_utf16().collect();
    units.push(0xD800);
    units.extend(extension.encode_utf16());
    OsString::from_wide(&units)
}

fn load(env: &mut Env, folder: &Path) {
    env.open(&folder.to_string_lossy(), None);
    env.pump();
}

// Port of nothing (Rust-only boundary): C# test JournalAppendRobustnessTests.Append_PathWithUnpairedSurrogate_ThrowsCodedErrorAndWritesNothing
// is the spec of the refusal.
#[test]
fn unrepresentable_name_is_not_loaded_and_is_reported_skipped() {
    let root = TempRoot::new("pathlossy-load");
    let folder = root.path().join("photos");
    std::fs::create_dir_all(&folder).expect("folder");
    let lone = folder.join(surrogate_name("a", ".jpg"));
    std::fs::write(&lone, b"x").expect("lone-surrogate file");
    std::fs::write(folder.join("ok.jpg"), b"x").expect("ok");
    let mut env = Env::physical(root.path());

    load(&mut env, &folder);

    let paths: Vec<String> = env
        .catalog
        .entries_snapshot()
        .iter()
        .map(|e| e.path().to_owned())
        .collect();
    assert_eq!(paths.len(), 1, "{paths:?}");
    assert!(paths[0].ends_with("ok.jpg"));
    assert!(paths.iter().all(|p| !p.contains('\u{FFFD}')));
    let calls = env.sink.skipped_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1.len(), 1);
    assert_eq!(calls[0].1[0].path, lone, "the exact (lossless) path is reported");
    assert!(!calls[0].1[0].reason.trim().is_empty());
}

// The decoy scenario: before the fix the lone-surrogate file took the decoy's path and the decoy was deduplicated
// away or acted on in its place. Now the catalog holds the decoy exactly once under its own real name.
#[test]
fn unrepresentable_name_never_aliases_a_real_replacement_character_sibling() {
    let root = TempRoot::new("pathlossy-decoy-load");
    let folder = root.path().join("photos");
    std::fs::create_dir_all(&folder).expect("folder");
    let lone = folder.join(surrogate_name("a", ".jpg"));
    std::fs::write(&lone, b"surrogate").expect("lone");
    let decoy = folder.join("a\u{FFFD}.jpg");
    std::fs::write(&decoy, b"decoy").expect("decoy");
    let mut env = Env::physical(root.path());

    load(&mut env, &folder);

    let entries = env.catalog.entries_snapshot();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path(), decoy.to_str().expect("unicode"));
    // The entry the user can act on is the real decoy file, with ITS length, not the surrogate file's.
    assert_eq!(entries[0].length, Some(5));
    let calls = env.sink.skipped_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1[0].path, lone);
}

// An unrepresentable sidecar must not become the sidecar of an image either (a Move would carry the wrong .xmp).
#[test]
fn unrepresentable_sidecar_is_not_loaded() {
    let root = TempRoot::new("pathlossy-xmp");
    let folder = root.path().join("photos");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(folder.join("a.jpg"), b"x").expect("jpg");
    std::fs::write(folder.join(surrogate_name("a", ".xmp")), b"x").expect("xmp");
    let mut env = Env::physical(root.path());
    env.set_raw(true, RawPairMode::PreferJpeg);

    load(&mut env, &folder);

    let entries = env.catalog.entries_snapshot();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].capture_group.as_ref().and_then(|g| g.xmp_path()).is_none());
}
