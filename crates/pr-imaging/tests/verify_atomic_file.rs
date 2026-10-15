//! Adversarial verification tests for W4-08a (NOT part of the commit).
use pr_core::sync::CancelToken;
use pr_imaging::cache::atomic_file::{AtomicWriteError, write_atomic};
use pr_test_support::TempRoot;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn tmp_count(dir: &std::path::Path) -> usize {
    fs::read_dir(dir)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
                .count()
        })
        .unwrap_or(0)
}

/// C#: finally{TryDelete(temp)} runs on any exception thrown by the payload; Rust panic leaves temp.
#[test]
fn payload_panic_removes_temp() {
    let root = TempRoot::new("w408v");
    let dir = root.path().to_path_buf();
    let cache = dir.join("c.bin");
    let c2 = cache.clone();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _ = write_atomic(
            &c2,
            |w: &mut dyn Write| {
                w.write_all(b"x")?;
                panic!("boom")
            },
            None,
            &CancelToken::new(),
        );
    }));
    assert!(r.is_err());
    assert_eq!(tmp_count(&dir), 0, "temp file left behind after payload panic");
}

/// C#: GetDirectoryName("root/sub/") == "root/sub" so the directory is created before the move fails.
#[test]
fn trailing_separator_creates_directory_like_csharp() {
    let root = TempRoot::new("w408v");
    let mut p = PathBuf::from(root.path()).join("sub");
    let mut s = p.into_os_string();
    s.push("\\");
    p = PathBuf::from(s);
    let r = write_atomic(&p, |w: &mut dyn Write| w.write_all(b"x"), None, &CancelToken::new());
    assert!(matches!(r, Err(AtomicWriteError::Io(_))));
    assert!(root.path().join("sub").is_dir(), "C# creates root/sub before failing");
}

/// C#: File.Move overwrite fails (access denied) while a reader holds the target open, even with FileShare.Delete.
#[cfg(windows)]
#[test]
fn overwrite_fails_while_target_open_with_share_delete() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = TempRoot::new("w408v");
    let cache = root.path().join("t.bin");
    fs::write(&cache, [1u8]).unwrap();
    let _h = fs::OpenOptions::new().read(true).share_mode(7).open(&cache).unwrap();
    let r = write_atomic(
        &cache,
        |w: &mut dyn Write| w.write_all(b"new"),
        None,
        &CancelToken::new(),
    );
    assert!(r.is_err(), "C# File.Move(overwrite) is denied; Rust replaced the file");
    assert_eq!(tmp_count(root.path()), 0);
}

/// C#: GetDirectoryName("verify_rel.bin") == "" -> CreateDirectory("") throws ArgumentException; nothing written.
#[test]
fn relative_file_name_without_directory_is_an_error() {
    let p = std::path::Path::new("verify_rel_w408.bin");
    let r = write_atomic(p, |w: &mut dyn Write| w.write_all(b"x"), None, &CancelToken::new());
    let created = p.exists();
    let _ = fs::remove_file(p);
    assert!(matches!(r, Err(AtomicWriteError::Io(_))));
    assert!(!created);
}

#[test]
fn buffer_size_constant() {
    assert_eq!(pr_imaging::cache::atomic_file::WRITE_BUFFER_SIZE, 65536);
}
