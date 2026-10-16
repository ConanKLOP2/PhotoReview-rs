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

// ---- W4-08a verify: long paths ----
fn long_dir(base: &std::path::Path, total: usize) -> PathBuf {
    let mut p = base.to_path_buf();
    let mut i = 0;
    while p.as_os_str().len() < total {
        p.push(format!("segment_{:03}_xxxxxxxxxxxxxxxxxxxxxxxx", i));
        i += 1;
    }
    p
}

#[test]
fn long_path_rename_over_plain() {
    let root = TempRoot::new("w408vl");
    let dir = long_dir(root.path(), 270);
    let cache = dir.join("c.bin");
    let r = write_atomic(
        &cache,
        |w: &mut dyn Write| w.write_all(b"abc"),
        None,
        &CancelToken::new(),
    );
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(fs::read(&cache).unwrap(), b"abc");
    assert_eq!(tmp_count(&dir), 0);
}

#[test]
fn long_path_rename_over_verbatim_input() {
    let root = TempRoot::new("w408vl");
    let dir = long_dir(root.path(), 270);
    let verb = PathBuf::from(format!(r"\\?\{}", dir.display())).join("c.bin");
    let r = write_atomic(
        &verb,
        |w: &mut dyn Write| w.write_all(b"abc"),
        None,
        &CancelToken::new(),
    );
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(fs::read(&verb).unwrap(), b"abc");
}

#[test]
fn long_path_only_temp_over_limit() {
    // destination < 260 but destination + ".<32hex>.tmp" >= 260
    let root = TempRoot::new("w408vl");
    let mut dir = long_dir(root.path(), 200);
    // shrink so that dir + "\\" + name is exactly 240 chars
    let name_len = 240usize.saturating_sub(dir.as_os_str().len() + 1);
    let name = "n".repeat(name_len.max(1));
    let cache = dir.join(&name);
    dir = cache.parent().unwrap().to_path_buf();
    assert!(cache.as_os_str().len() < 260 && cache.as_os_str().len() + 37 >= 260 - 20);
    let r = write_atomic(
        &cache,
        |w: &mut dyn Write| w.write_all(b"abc"),
        None,
        &CancelToken::new(),
    );
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(tmp_count(&dir), 0);
}

#[test]
fn long_path_unc_admin_share() {
    let root = TempRoot::new("w408vl");
    let s = root.path().display().to_string();
    let rest = &s[3..];
    let unc = PathBuf::from(format!(r"\\127.0.0.1\{}$\{}", &s[..1], rest));
    if fs::metadata(&unc).is_err() {
        eprintln!("admin share unavailable");
        return;
    }
    let dir = long_dir(&unc, 290);
    let cache = dir.join("c.bin");
    let r = write_atomic(
        &cache,
        |w: &mut dyn Write| w.write_all(b"abc"),
        None,
        &CancelToken::new(),
    );
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(fs::read(&cache).unwrap(), b"abc");
}

#[test]
fn long_path_unc_which_step() {
    let root = TempRoot::new("w408vl");
    let s = root.path().display().to_string();
    let unc = PathBuf::from(format!(r"\\127.0.0.1\{}$\{}", &s[..1], &s[3..]));
    if fs::metadata(&unc).is_err() {
        return;
    }
    let dir = long_dir(&unc, 290);
    eprintln!("len {}", dir.as_os_str().len());
    eprintln!("create_dir_all: {:?}", fs::create_dir_all(&dir));
    let f = dir.join("a.tmp");
    eprintln!("write: {:?}", fs::write(&f, b"x"));
    eprintln!("rename: {:?}", fs::rename(&f, dir.join("b.bin")));
}
