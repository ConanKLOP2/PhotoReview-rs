//! W2-14 tests for `pr_core::io::counting` (port of `CountingFileSystem.cs`, C# baseline d92d6aae).
//!
//! C# `InMemoryFileSystem` => `FakeFileSystem`; C# `ReviewMetrics.StatCount` => an `AtomicU64` counter closure (W2-17 owns
//! the real `ReviewMetrics`).

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pr_core::abstractions::{CopyCreationProof, FileStat, FileSystem, ReadSeek, SkippedEntry};
use pr_core::io::counting::{CountingFileSystem, StatCounter};
use pr_test_support::FakeFileSystem;

fn counter() -> (Arc<AtomicU64>, Arc<dyn StatCounter>) {
    let count = Arc::new(AtomicU64::new(0));
    let c = Arc::clone(&count);
    let metrics: Arc<dyn StatCounter> = Arc::new(move || {
        c.fetch_add(1, Ordering::SeqCst);
    });
    (count, metrics)
}

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

// C#: CountingFileSystemTests.StatQueriesAreCounted
#[test]
fn stat_queries_are_counted() {
    let inner = Arc::new(FakeFileSystem::new());
    inner.add_dir(r"C:\photos");
    inner
        .write_all_text_atomic(&p(r"C:\photos\a.jpg"), "12345", true)
        .unwrap();
    let (count, metrics) = counter();
    let fs = CountingFileSystem::new(inner, metrics);

    assert!(fs.file_exists(&p(r"C:\photos\a.jpg")));
    assert!(!fs.directory_exists(&p(r"C:\nope")));
    assert_eq!(fs.get_file_stat(&p(r"C:\photos\a.jpg")).map(|s| s.length), Some(5));
    assert!(fs.get_file_stat(&p(r"C:\photos\missing.jpg")).is_none());

    assert_eq!(count.load(Ordering::SeqCst), 4);
}

// C#: CountingFileSystemTests.OtherOperationsAreNotCounted
#[test]
fn other_operations_are_not_counted() {
    let inner = Arc::new(FakeFileSystem::new());
    inner.add_dir(r"C:\photos");
    let (count, metrics) = counter();
    let fs = CountingFileSystem::new(inner, metrics);

    fs.write_all_text_atomic(&p(r"C:\photos\b.txt"), "hello", true).unwrap();
    let text = fs.read_all_text(&p(r"C:\photos\b.txt")).unwrap();

    assert_eq!(text, "hello");
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

// C#: CountingFileSystemTests.ForwardedOperations_ReachTheInnerFileSystem_WithoutCounting
#[test]
fn forwarded_operations_reach_the_inner_file_system_without_counting() {
    let inner = Arc::new(FakeFileSystem::new());
    let (count, metrics) = counter();
    let fs = CountingFileSystem::new(Arc::clone(&inner) as Arc<dyn FileSystem>, metrics);
    let mut skipped: Vec<SkippedEntry> = Vec::new();

    fs.create_directory(&p(r"C:\photos")).unwrap();
    fs.write_all_text_atomic(&p(r"C:\photos\a.jpg"), "12345", true).unwrap();
    fs.copy_file(&p(r"C:\photos\a.jpg"), &p(r"C:\photos\b.jpg")).unwrap();
    assert!(fs.try_copy_new(&p(r"C:\photos\a.jpg"), &p(r"C:\photos\c.jpg")).unwrap());
    // c.jpg exists now: the inner "create new" refuses.
    assert!(!fs.try_copy_new(&p(r"C:\photos\a.jpg"), &p(r"C:\photos\c.jpg")).unwrap());
    fs.move_file(&p(r"C:\photos\b.jpg"), &p(r"C:\photos\d.jpg")).unwrap();
    fs.delete(&p(r"C:\photos\c.jpg")).unwrap();
    {
        let mut append = fs.open_append(&p(r"C:\photos\log.txt"), false).unwrap();
        append.write_all(b"hi").unwrap();
    }

    let mut all = fs.enumerate_files(&p(r"C:\photos"), "*").unwrap();
    all.sort_by_key(|x| x.to_string_lossy().to_lowercase());
    assert_eq!(
        all,
        vec![p(r"C:\photos\a.jpg"), p(r"C:\photos\d.jpg"), p(r"C:\photos\log.txt")]
    );
    assert_eq!(
        fs.enumerate_files(&p(r"C:\photos"), "a.jpg").unwrap(),
        vec![p(r"C:\photos\a.jpg")]
    );
    assert_eq!(fs.read_all_text(&p(r"C:\photos\log.txt")).unwrap(), "hi");
    let mut text = String::new();
    fs.open_read_shared(&p(r"C:\photos\a.jpg"), 16)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    assert_eq!(text, "12345");
    assert_eq!(fs.try_probe_readable(&p(r"C:\photos\a.jpg")), Ok(()));
    // The C# InMemoryFileSystem reports a missing file as unreadable; FakeFileSystem needs the mark.
    inner.set_unreadable(r"C:\photos\gone.jpg", "missing");
    assert!(fs.try_probe_readable(&p(r"C:\photos\gone.jpg")).is_err());
    let ends_with_a = |path: &Path| path.to_string_lossy().ends_with("a.jpg");
    let listed = fs
        .enumerate_files_with_stat(&p(r"C:\photos"), &ends_with_a, &mut |s| skipped.push(s))
        .unwrap();
    assert_eq!(
        listed.into_iter().map(|(x, _)| x).collect::<Vec<_>>(),
        vec![p(r"C:\photos\a.jpg")]
    );
    let listed = fs
        .enumerate_readable_files_with_stat(&p(r"C:\photos"), &ends_with_a, &mut |s| skipped.push(s))
        .unwrap();
    assert_eq!(
        listed.into_iter().map(|(x, _)| x).collect::<Vec<_>>(),
        vec![p(r"C:\photos\a.jpg")]
    );
    assert!(skipped.is_empty());
    let resolved = fs.resolve_real_path(&p(r"C:\photos\a.jpg")).unwrap();
    assert!(resolved.to_string_lossy().eq_ignore_ascii_case(r"C:\photos\a.jpg"));
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

// C#: CountingFileSystemTests.EnumerateDirectories_IsForwarded
#[test]
fn enumerate_directories_is_forwarded() {
    let inner = Arc::new(FakeFileSystem::new());
    inner.create_directory(&p(r"C:\photos\sub")).unwrap();
    let (_, metrics) = counter();
    let fs = CountingFileSystem::new(Arc::clone(&inner) as Arc<dyn FileSystem>, metrics);

    assert_eq!(
        inner.enumerate_directories(&p(r"C:\photos")).unwrap(),
        fs.enumerate_directories(&p(r"C:\photos")).unwrap()
    );
    assert!(!fs.enumerate_directories(&p(r"C:\photos")).unwrap().is_empty());
}

// C#: CountingFileSystemTests.TryCopyNewWithProof_IsForwardedToTheInnerOverload
#[test]
fn try_copy_new_with_proof_is_forwarded_to_the_inner_overload() {
    let inner = Arc::new(FakeFileSystem::new());
    inner.add_dir(r"C:\photos");
    inner
        .write_all_text_atomic(&p(r"C:\photos\a.jpg"), "12345", true)
        .unwrap();
    let (_, metrics) = counter();
    let fs = CountingFileSystem::new(Arc::clone(&inner) as Arc<dyn FileSystem>, metrics);
    let proof = CopyCreationProof::new();

    let created = fs
        .try_copy_new_with_proof(&p(r"C:\photos\a.jpg"), &p(r"C:\photos\copy.jpg"), &proof)
        .unwrap();

    assert!(created);
    assert_eq!(inner.read_all_text(&p(r"C:\photos\copy.jpg")).unwrap(), "12345");
    assert!(proof.destination_created());
}

// ---------------------------------------------------------------------------------------------------------------
// Forwarding recorder
// ---------------------------------------------------------------------------------------------------------------

/// Records every `FileSystem` method invoked on it. It overrides EVERY trait method, the defaulted ones too, so a
/// decorator that leaves a method on its trait default shows up as a different call (e.g. `try_copy_new` left default
/// would log `try_copy_new_with_proof`). NOTE: a method added to the `FileSystem` trait must be added here and to
/// `counting_file_system_forwards_every_method_to_the_inner_file_system`.
#[derive(Default)]
struct RecordingFs {
    calls: Mutex<Vec<&'static str>>,
    /// Address of the last proof `try_copy_new_with_proof` received.
    proof_address: Mutex<Option<usize>>,
}

impl RecordingFs {
    fn record(&self, name: &'static str) {
        self.calls.lock().unwrap_or_else(PoisonError::into_inner).push(name);
    }

    fn take(&self) -> Vec<&'static str> {
        std::mem::take(&mut *self.calls.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl FileSystem for RecordingFs {
    fn file_exists(&self, _: &Path) -> bool {
        self.record("file_exists");
        true
    }
    fn directory_exists(&self, _: &Path) -> bool {
        self.record("directory_exists");
        true
    }
    fn get_file_stat(&self, _: &Path) -> Option<FileStat> {
        self.record("get_file_stat");
        None
    }
    fn move_file(&self, _: &Path, _: &Path) -> io::Result<()> {
        self.record("move_file");
        Ok(())
    }
    fn copy_file(&self, _: &Path, _: &Path) -> io::Result<()> {
        self.record("copy_file");
        Ok(())
    }
    fn try_copy_new(&self, _: &Path, _: &Path) -> io::Result<bool> {
        self.record("try_copy_new");
        Ok(true)
    }
    fn try_copy_new_with_proof(&self, _: &Path, _: &Path, proof: &CopyCreationProof) -> io::Result<bool> {
        self.record("try_copy_new_with_proof");
        *self.proof_address.lock().unwrap_or_else(PoisonError::into_inner) = Some(std::ptr::from_ref(proof) as usize);
        Ok(true)
    }
    fn delete(&self, _: &Path) -> io::Result<()> {
        self.record("delete");
        Ok(())
    }
    fn open_read_shared(&self, _: &Path, _: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.record("open_read_shared");
        Ok(Box::new(io::Cursor::new(Vec::new())))
    }
    fn open_append(&self, _: &Path, _: bool) -> io::Result<Box<dyn Write + Send>> {
        self.record("open_append");
        Ok(Box::new(io::sink()))
    }
    fn write_all_text_atomic(&self, _: &Path, _: &str, _: bool) -> io::Result<()> {
        self.record("write_all_text_atomic");
        Ok(())
    }
    fn read_all_text(&self, _: &Path) -> io::Result<String> {
        self.record("read_all_text");
        Ok(String::new())
    }
    fn enumerate_files(&self, _: &Path, _: &str) -> io::Result<Vec<PathBuf>> {
        self.record("enumerate_files");
        Ok(Vec::new())
    }
    fn enumerate_files_with_stat(
        &self,
        _: &Path,
        _: &dyn Fn(&Path) -> bool,
        _: &mut dyn FnMut(SkippedEntry),
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        self.record("enumerate_files_with_stat");
        Ok(Vec::new())
    }
    fn try_probe_readable(&self, _: &Path) -> Result<(), String> {
        self.record("try_probe_readable");
        Ok(())
    }
    fn enumerate_readable_files_with_stat(
        &self,
        _: &Path,
        _: &dyn Fn(&Path) -> bool,
        _: &mut dyn FnMut(SkippedEntry),
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        self.record("enumerate_readable_files_with_stat");
        Ok(Vec::new())
    }
    fn enumerate_directories(&self, _: &Path) -> io::Result<Vec<PathBuf>> {
        self.record("enumerate_directories");
        Ok(Vec::new())
    }
    fn create_directory(&self, _: &Path) -> io::Result<()> {
        self.record("create_directory");
        Ok(())
    }
    fn try_delete_empty_directory(&self, _: &Path) -> bool {
        self.record("try_delete_empty_directory");
        true
    }
    fn resolve_real_path(&self, path: &Path) -> io::Result<PathBuf> {
        self.record("resolve_real_path");
        Ok(path.to_path_buf())
    }
}

// C#: FileSystemWrapperForwardingTests.EveryFileSystemDecorator_ForwardsTheCreateNewPrimitivesToTheInnerFileSystem
// Rust has no reflection: the decorator is called method by method and the inner log must be exactly that method. The C#
// scope is narrower (only the Try*New primitives, plus SlowLinkFileSystem, a W6+ unit); checking every method is a
// deliberate Rust strengthening.
#[test]
fn counting_file_system_forwards_every_method_to_the_inner_file_system() {
    let inner = Arc::new(RecordingFs::default());
    let (_, metrics) = counter();
    let fs = CountingFileSystem::new(Arc::clone(&inner) as Arc<dyn FileSystem>, metrics);
    let a = p(r"C:\a.jpg");
    let b = p(r"C:\b.jpg");
    let all = |_: &Path| true;
    let mut sink = |_: SkippedEntry| {};

    let check = |name: &'static str, call: &dyn Fn()| {
        inner.take();
        call();
        assert_eq!(inner.take(), vec![name], "{name} was not forwarded as itself");
    };
    check("file_exists", &|| {
        fs.file_exists(&a);
    });
    check("directory_exists", &|| {
        fs.directory_exists(&a);
    });
    check("get_file_stat", &|| {
        fs.get_file_stat(&a);
    });
    check("move_file", &|| fs.move_file(&a, &b).unwrap());
    check("copy_file", &|| fs.copy_file(&a, &b).unwrap());
    check("try_copy_new", &|| {
        fs.try_copy_new(&a, &b).unwrap();
    });
    check("delete", &|| fs.delete(&a).unwrap());
    check("open_read_shared", &|| {
        fs.open_read_shared(&a, 16).unwrap();
    });
    check("open_append", &|| {
        fs.open_append(&a, true).unwrap();
    });
    check("write_all_text_atomic", &|| {
        fs.write_all_text_atomic(&a, "x", true).unwrap()
    });
    check("read_all_text", &|| {
        fs.read_all_text(&a).unwrap();
    });
    check("enumerate_files", &|| {
        fs.enumerate_files(&a, "*").unwrap();
    });
    check("try_probe_readable", &|| fs.try_probe_readable(&a).unwrap());
    check("enumerate_directories", &|| {
        fs.enumerate_directories(&a).unwrap();
    });
    check("create_directory", &|| fs.create_directory(&a).unwrap());
    check("try_delete_empty_directory", &|| {
        fs.try_delete_empty_directory(&a);
    });
    check("resolve_real_path", &|| {
        fs.resolve_real_path(&a).unwrap();
    });

    // The FnMut sink cannot go through the shared `check` closure.
    inner.take();
    fs.enumerate_files_with_stat(&a, &all, &mut sink).unwrap();
    assert_eq!(inner.take(), vec!["enumerate_files_with_stat"]);
    fs.enumerate_readable_files_with_stat(&a, &all, &mut sink).unwrap();
    assert_eq!(inner.take(), vec!["enumerate_readable_files_with_stat"]);

    // The proof passes through untouched: the SAME object reaches the inner file system.
    let proof = CopyCreationProof::new();
    assert!(fs.try_copy_new_with_proof(&a, &b, &proof).unwrap());
    assert_eq!(inner.take(), vec!["try_copy_new_with_proof"]);
    assert_eq!(
        *inner.proof_address.lock().unwrap_or_else(PoisonError::into_inner),
        Some(std::ptr::from_ref(&proof) as usize)
    );
}
