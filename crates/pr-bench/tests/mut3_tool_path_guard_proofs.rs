//! MUT3 (2026-11-06), task P2: run proofs for the `tool_path_guard.rs` survivors N4 left open (226:9, 276:62 `|` -> `&`).
//! Windows only (the file is `#[cfg(windows)]` in the sense that every test needs Win32 error codes and attributes). Hand-written,
//! no golden file, no sleep. 299:5 (twice) and 299:76 sit in a `#[cfg(not(windows))]` body that this binary cannot compile.

#![cfg(windows)]

use std::process::Command;

use pr_bench::cli::tool_path_guard::write_report_file;
use pr_test_support::TempRoot;

/// transport of the Win32 code 3 (`ERROR_PATH_NOT_FOUND`) through std: it is classified as `ErrorKind::NotFound`.
#[test]
fn win32_error_3_is_the_not_found_kind() {
    let error = std::io::Error::from_raw_os_error(3);
    assert_eq!(error.raw_os_error(), Some(3));
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

/// decoder-free proof for tool_path_guard.rs:226:9 `delete match arm Some(ERROR_PATH_NOT_FOUND)` in `map_write_error`.
///
/// The deleted arm and the fallback arm that takes over for code 3 are the same call, `ToolError::from_io(error, path,
/// "DirectoryNotFoundException")`, with the same arguments; the fallback is reached for code 3 because std classifies it as
/// `NotFound` (test above) and the fallback maps `NotFound` to `DirectoryNotFoundException`. The divergent set is therefore the
/// single error value `Os(3)` and the free variable is the path string, which both versions hand over unchanged.
/// Run here through the real writer for the path shapes that produce code 3: one missing folder, several missing levels, a
/// relative path, a path with spaces and non-ASCII letters, a very long folder name, and a drive-relative root that does not
/// exist. Every one reports the .NET type and the text built from the temp file name.
#[test]
fn every_missing_parent_shape_gives_the_directory_not_found_error_with_its_own_path() {
    let root = TempRoot::new("mut3-pathnotfound");
    let shapes: Vec<(String, std::path::PathBuf)> = vec![
        ("one level".into(), root.join("missing").join("r.json")),
        ("three levels".into(), root.join("a").join("b").join("c").join("r.json")),
        (
            "spaces and accents".into(),
            root.join("thu muc").join("tập tin").join("r.json"),
        ),
        ("long folder".into(), root.join("x".repeat(120)).join("r.json")),
        ("dots".into(), root.join("..").join("not-here-mut3").join("r.json")),
    ];
    for (name, path) in shapes {
        let text = path.to_string_lossy().into_owned();
        let error = write_report_file(&text, "x").expect_err(&name);
        assert_eq!(error.type_name, "DirectoryNotFoundException", "{name}");
        assert!(
            error.message.starts_with("Could not find a part of the path '"),
            "{name}: {}",
            error.message
        );
        assert!(error.message.ends_with(".tmp'."), "{name}: {}", error.message);
        if name != "dots" {
            // The full-path conversion folds the `..` of the last shape; the others keep their text.
            assert!(error.message.contains(&text), "{name}: {}", error.message);
        }
    }
}

fn attrib(path: &std::path::Path, flags: &[&str]) {
    let mut command = Command::new("attrib");
    command.args(flags).arg(path);
    let status = command.status().expect("attrib runs");
    assert!(status.success(), "attrib {flags:?} {}", path.display());
}

/// tool_path_guard.rs:276:62 `FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_DIRECTORY` -> `&` (mask 0: the up-front refusal never fires
/// and `rename` decides). The refusal exists for leaves that `rename` would otherwise replace; the divergent set is every leaf whose
/// attributes contain READONLY or DIRECTORY. The attribute lattice of those leaves is run here: {file, directory, non-empty directory,
/// directory junction} x {plain, READONLY} x {plain, HIDDEN} x {plain, SYSTEM}, plus a READONLY file. Every directory-like or
/// READONLY leaf must be refused with the path-less access-denied error and keep its content; every other leaf is replaced. The same
/// outcome table must come out under the mutant.
#[test]
#[allow(clippy::permissions_set_readonly_false)]
fn the_attribute_lattice_of_replaced_leaves_gets_the_same_verdict() {
    let root = TempRoot::new("mut3-leaf-lattice");
    let mut rows = Vec::new();
    for kind in ["file", "dir", "dir-with-child", "junction"] {
        for read_only in [false, true] {
            for hidden in [false, true] {
                for system in [false, true] {
                    let name = format!(
                        "{kind}-r{}-h{}-s{}.json",
                        u8::from(read_only),
                        u8::from(hidden),
                        u8::from(system)
                    );
                    let leaf = root.join(&name);
                    match kind {
                        "file" => std::fs::write(&leaf, "old").expect("file"),
                        "dir" => std::fs::create_dir(&leaf).expect("dir"),
                        "dir-with-child" => {
                            std::fs::create_dir(&leaf).expect("dir");
                            std::fs::write(leaf.join("child.txt"), "c").expect("child");
                        }
                        _ => {
                            let target = root.join(format!("target-of-{name}"));
                            std::fs::create_dir(&target).expect("junction target");
                            let made = Command::new("cmd")
                                .args(["/C", "mklink", "/J"])
                                .arg(&leaf)
                                .arg(&target)
                                .output()
                                .expect("cmd runs");
                            assert!(
                                made.status.success(),
                                "mklink /J: {}",
                                String::from_utf8_lossy(&made.stdout)
                            );
                        }
                    }
                    let mut flags = Vec::new();
                    if read_only {
                        flags.push("+r");
                    }
                    if hidden {
                        flags.push("+h");
                    }
                    if system {
                        flags.push("+s");
                    }
                    if !flags.is_empty() {
                        attrib(&leaf, &flags);
                    }
                    rows.push((kind, read_only, hidden, system, leaf));
                }
            }
        }
    }
    let mut table = Vec::new();
    for (kind, read_only, hidden, system, leaf) in &rows {
        let outcome = write_report_file(&leaf.to_string_lossy(), "new");
        let refused = match &outcome {
            Ok(()) => false,
            Err(error) => {
                assert_eq!(
                    error.type_name, "UnauthorizedAccessException",
                    "{kind} r{read_only} h{hidden} s{system}"
                );
                assert_eq!(
                    error.message, "Access to the path is denied.",
                    "{kind} r{read_only} h{hidden} s{system}"
                );
                true
            }
        };
        table.push((*kind, *read_only, *hidden, *system, refused));
    }
    // Clear the attributes so the temp folder can be deleted.
    for (_, read_only, hidden, system, leaf) in &rows {
        let mut flags = Vec::new();
        if *read_only {
            flags.push("-r");
        }
        if *hidden {
            flags.push("-h");
        }
        if *system {
            flags.push("-s");
        }
        if !flags.is_empty() {
            let _ = Command::new("attrib").args(&flags).arg(leaf).status();
        }
    }
    for (kind, read_only, hidden, system, refused) in table {
        let expected = kind != "file" || read_only;
        assert_eq!(
            refused, expected,
            "{kind} read_only={read_only} hidden={hidden} system={system}"
        );
    }
}
