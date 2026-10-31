//! Port of PhotoReview.Core/IO/PhysicalFileSystem.cs: remaining physical adapter guards.
#![cfg(windows)]

use pr_core::abstractions::FileSystem;
use pr_core::io::physical::{PhysicalFileSystem, follow_link_chain_for_tests, replace_with_retry};
use pr_test_support::TempRoot;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

// C# EnumerateFilesWithStat; physical.rs:1396 three trait body replacements and :1378 || -> &&.
#[test]
#[ignore = "native: only private TempRoot files"]
fn enumerate_files_with_stat_preserves_paths_stats_filter_and_errors() {
    let root = TempRoot::new("physrest2-list");
    let file = root.join("photo.jpg");
    std::fs::write(&file, b"photo").unwrap();
    std::fs::create_dir(root.join("folder")).unwrap();
    let fs = PhysicalFileSystem;
    let rows = fs
        .enumerate_files_with_stat(root.path(), &|_| true, &mut |_| panic!("skip"))
        .unwrap();
    assert_eq!(rows, vec![(file.clone(), fs.get_file_stat(&file))]);
    assert_eq!(rows[0].1.unwrap().length, 5);
    assert!(
        fs.enumerate_files_with_stat(root.path(), &|_| false, &mut |_| panic!("skip"))
            .unwrap()
            .is_empty()
    );
    assert!(
        fs.enumerate_files_with_stat(&root.join("missing"), &|_| true, &mut |_| panic!("skip"))
            .is_err()
    );
    for pattern in ["sub/*.jpg", "sub\\*.jpg", "a\0b", "Q:*.jpg"] {
        assert_eq!(
            fs.enumerate_files(root.path(), pattern).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
    assert_eq!(fs.enumerate_files(root.path(), "*.jpg").unwrap(), [file]);
}

// C# GetFinalLinkTargetSlow; physical.rs:998/:1017 body, :1002 += and :1003 > operators.
#[test]
fn follow_link_chain_returns_last_target_and_rejects_cycles_with_a_bounded_wait() {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        assert_eq!(
            follow_link_chain_for_tests("none", &mut |_| None).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        let mut asked = Vec::new();
        let last = follow_link_chain_for_tests("start", &mut |p| {
            asked.push(p.to_owned());
            match p {
                "start" => Some("middle".into()),
                "middle" => Some("last".into()),
                _ => None,
            }
        })
        .unwrap();
        assert_eq!(last, "last");
        assert_eq!(asked, ["start", "middle", "last"]);
        let mut calls = 0;
        let error = follow_link_chain_for_tests("cycle", &mut |_| {
            calls += 1;
            Some("cycle".into())
        })
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(1921));
        assert_eq!(calls, 64);
        let mut calls = 0;
        assert_eq!(
            follow_link_chain_for_tests("start", &mut |_| {
                calls += 1;
                (calls <= 63).then(|| "target".into())
            })
            .unwrap(),
            "target"
        );
        assert_eq!(calls, 64);
        tx.send(()).unwrap();
    });
    rx.recv_timeout(Duration::from_secs(10))
        .expect("link walk must terminate");
}

// C# CreateDirectory; physical.rs:678/:694/:696 loop arithmetic and guard replacements.
#[test]
#[ignore = "native: only private TempRoot directories"]
fn create_directory_walk_terminates_and_creates_every_missing_parent() {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let root = TempRoot::new("physrest2-create");
        let leaf = root.join("one/two/three");
        PhysicalFileSystem.create_directory(&leaf).unwrap();
        let trailing = root.join("trailing/deeper");
        PhysicalFileSystem
            .create_directory(Path::new(&format!("{}\\", trailing.display())))
            .unwrap();
        assert!(trailing.is_dir());
        assert!(leaf.is_dir());
        PhysicalFileSystem.create_directory(&leaf).unwrap();
        let file = root.join("file");
        std::fs::write(&file, b"x").unwrap();
        assert!(PhysicalFileSystem.create_directory(&file.join("child")).is_err());
        tx.send(()).unwrap();
    });
    rx.recv_timeout(Duration::from_secs(10))
        .expect("directory walk must terminate");
}

// C# ReplaceWithRetry; physical.rs:109/:113/:116 body, guard, delay and attempt arithmetic.
#[test]
fn replace_retry_preserves_final_error_and_uses_exact_attempts_and_delays() {
    for code in [5, 32, 33] {
        let mut attempts = 0;
        let mut sleeps = Vec::new();
        let error = replace_with_retry(
            || {
                attempts += 1;
                assert!(attempts <= 8, "retry cap exceeded");
                Err(io::Error::from_raw_os_error(code))
            },
            |delay| sleeps.push(delay),
        )
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(code));
        assert_eq!(attempts, 8);
        assert_eq!(sleeps, [3, 6, 9, 12, 15, 18, 21]);
    }
    for code in [2, 87, 1314] {
        let mut attempts = 0;
        let error = replace_with_retry(
            || {
                attempts += 1;
                Err(io::Error::from_raw_os_error(code))
            },
            |_| panic!("nontransient retry"),
        )
        .unwrap_err();
        assert_eq!(attempts, 1);
        assert_eq!(error.raw_os_error(), Some(code));
    }
    let mut attempts = 0;
    let mut sleeps = Vec::new();
    replace_with_retry(
        || {
            attempts += 1;
            if attempts < 3 {
                Err(io::ErrorKind::PermissionDenied.into())
            } else {
                Ok(())
            }
        },
        |delay| sleeps.push(delay),
    )
    .unwrap();
    assert_eq!(attempts, 3);
    assert_eq!(sleeps, [3, 6]);
}

// C# FileExists/OpenReadShared/ResolveRealPath; physical.rs:1209 bool, :1139/:1325/:1436 body,
// :1143 share bits and :1326 > operators (buffering-only changes can remain unobservable).
#[test]
#[ignore = "native: private TempRoot reads and shared handles"]
fn shared_reads_seek_and_survive_rename_and_exists_rejects_invalid_inputs() {
    let root = TempRoot::new("physrest2-read");
    let path = root.join("photo.bin");
    std::fs::write(&path, b"0123456789").unwrap();
    let fs = PhysicalFileSystem;
    assert!(fs.file_exists(&path));
    assert!(!fs.file_exists(root.path()));
    assert!(!fs.file_exists(&root.join("missing")));
    for input in ["", "   ", "a\0b"] {
        assert!(!fs.file_exists(Path::new(input)));
    }
    assert!(!fs.file_exists(Path::new(&format!("{}\\", path.display()))));
    for capacity in [0, 1, 2, 64] {
        let mut reader = fs.open_read_shared(&path, capacity).unwrap();
        reader.seek(SeekFrom::Start(4)).unwrap();
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"456789");
    }
    let mut raw = fs.open_read_shared_file(&path).unwrap();
    let renamed = root.join("renamed.bin");
    std::fs::rename(&path, &renamed).unwrap();
    std::fs::OpenOptions::new().write(true).open(&renamed).unwrap();
    let mut bytes = Vec::new();
    raw.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"0123456789");
    assert_eq!(fs.resolve_real_path(&renamed).unwrap(), renamed);
    let missing = root.join("missing/deeper/file");
    assert_eq!(fs.resolve_real_path(&missing).unwrap(), missing);
    assert!(fs.open_read_shared_file(Path::new(" ")).is_err());
    assert!(fs.resolve_real_path(Path::new(" ")).is_err());
}

// C# PathInternal.EnsureExtendedPrefix; physical.rs:501/:513 !, >= and || operators.
#[test]
#[ignore = "native: long and device paths inside private TempRoot"]
fn long_paths_and_extended_dot_names_preserve_the_requested_entry() {
    let root = TempRoot::new("physrest2-prefix");
    let base = root.path().to_string_lossy();
    for length in [259, 260, 261] {
        let parent = root.join("p".repeat(100));
        std::fs::create_dir_all(&parent).unwrap();
        let padding = length - parent.to_string_lossy().encode_utf16().count() - 1;
        let path = parent.join("x".repeat(padding));
        assert_eq!(path.to_string_lossy().encode_utf16().count(), length);
        PhysicalFileSystem.create_directory(&path).unwrap();
        assert!(path.is_dir());
    }
    let extended = format!(r"\\?\{base}\kept.");
    PhysicalFileSystem.create_directory(Path::new(&extended)).unwrap();
    assert!(
        std::fs::read_dir(root.path())
            .unwrap()
            .any(|entry| entry.unwrap().file_name() == "kept.")
    );
    assert!(!root.join("kept").exists());
}

// C# ResolveRealPath/GetFinalLinkTargetSlow; physical.rs:950-1049 resolver body and reparse guards.
#[test]
#[ignore = "native: junctions created only inside private TempRoot"]
fn resolve_real_path_follows_live_and_dangling_private_junctions() {
    let root = TempRoot::new("physrest2-link");
    let target = root.join("target");
    std::fs::create_dir(&target).unwrap();
    let junction = |name: &str, target: &Path| {
        let link = root.join(name);
        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(target)
            .output()
            .unwrap();
        assert!(output.status.success(), "junction failed: {:?}", output);
        link
    };
    let live = junction("live", &target);
    assert_eq!(PhysicalFileSystem.resolve_real_path(&live).unwrap(), target);
    assert_eq!(
        PhysicalFileSystem.resolve_real_path(&live.join("missing.jpg")).unwrap(),
        target.join("missing.jpg")
    );
    let missing = root.join("absent");
    let dangling = junction("dangling", &missing);
    assert_eq!(PhysicalFileSystem.resolve_real_path(&dangling).unwrap(), missing);
    assert_eq!(
        PhysicalFileSystem.resolve_real_path(&dangling.join("file")).unwrap(),
        missing.join("file")
    );
}
