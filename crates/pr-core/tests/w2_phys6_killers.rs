//! Port of PhotoReview.Core/IO/PhysicalFileSystem.cs: killers for the survivors of the complete `io/physical.rs` mutation
//! run that no earlier killer reached (CL-PHYS6). Pure functions through `#[doc(hidden)]` seams, plus a few native tests
//! that touch only a private `TempRoot` (the single exception creates nothing: see the `create_directory` test).
#![cfg(windows)]

use pr_core::abstractions::FileSystem;
use pr_core::io::physical::{
    PhysicalFileSystem, ensure_extended_prefix_if_needed_for_tests, is_device_for_tests,
    resolve_if_reparse_point_for_tests,
};
use pr_test_support::TempRoot;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

const Q: &str = "\u{5c}";

fn bounded<T: Send + 'static>(what: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
        let _ = tx.send(outcome);
    });
    match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(Ok(value)) => value,
        Ok(Err(payload)) => std::panic::resume_unwind(payload),
        Err(err) => panic!("{what} did not finish within 20 s ({err})"),
    }
}

// C#: PathInternal.IsDevice: `\\?\`, `\??\` and `\\.\` (either separator for the first two forms), nothing else.
// physical.rs:491:39 `==` -> `!=` (a '.'/'?' server name of a UNC path), :492:16 `<` -> `==` / `<=` (a 3-char `\\.` indexes
// b[3]; the 4-char `\\.\` is a device), :495:16 / :495:29 / :495:63 and :496:27 / :496:59 `&&` -> `||` (every UNC path, or
// one with a one-character server, would be a device).
#[test]
fn is_device_recognises_exactly_the_device_prefixes() {
    let devices = [
        format!("{Q}{Q}?{Q}C:{Q}x"),
        format!("{Q}{Q}.{Q}C:{Q}x"),
        format!("{Q}??{Q}C:{Q}x"),
        "//?/C:/x".to_owned(),
        "//./C:/x".to_owned(),
        format!("{Q}/?{Q}C:{Q}x"),
        format!("/{Q}.{Q}C:{Q}x"),
        // Exactly four characters: `len < 4` must not exclude them (`<=` mutant).
        format!("{Q}{Q}.{Q}"),
        format!("{Q}{Q}?{Q}"),
        format!("{Q}??{Q}"),
    ];
    for path in &devices {
        assert!(is_device_for_tests(path), "{path:?} is a device path");
    }
    let others = [
        format!("{Q}{Q}.x{Q}share{Q}d"), // UNC server `.x`
        format!("{Q}{Q}?x{Q}s{Q}d"),     // UNC server `?x`
        format!("{Q}{Q}a{Q}b{Q}c"),      // UNC with a one-character server: b[3] is a separator
        format!("{Q}{Q}srv{Q}share{Q}x"),
        format!("{Q}{Q}srv{Q}share{Q}dir{Q}file"),
        format!("C:{Q}abc"),
        format!("C:{Q}{Q}x"),
        "C:/abc".to_owned(),
        format!("{Q}{Q}.x"),
        format!("{Q}{Q}?"),
        format!("{Q}??x"),
        format!("{Q}?{Q}{Q}"),
        format!("{Q}{Q}.."),
        "abcd".to_owned(),
        // Shorter than four characters: false, and no index past the end (`<` -> `==` mutant panics on `\\.` and `\\`).
        format!("{Q}{Q}."),
        format!("{Q}{Q}"),
        format!("{Q}{Q}?"),
        "//.".to_owned(),
        "C:".to_owned(),
        "a".to_owned(),
        String::new(),
    ];
    for path in &others {
        assert!(!is_device_for_tests(path), "{path:?} is not a device path");
    }
}

// C#: PathInternal.EnsureExtendedPrefixIfNeeded / EnsureExtendedPrefix: a fully qualified non-device path that is 260
// units or longer, or ends with '.' or ' ', gets `\\?\` (`\\?\UNC\` for a UNC path); a device path and a short path are
// left alone. A UNC path must get `\\?\UNC\` (the `is_device` mutants of :495/:496 turn every UNC path into a device).
#[test]
fn ensure_extended_prefix_if_needed_adds_the_unc_form_only_to_non_device_paths() {
    let cases: [(String, String); 12] = [
        (
            format!("{Q}{Q}srv{Q}share{Q}dir{Q}file."),
            format!("{Q}{Q}?{Q}UNC{Q}srv{Q}share{Q}dir{Q}file."),
        ),
        (format!("{Q}{Q}a{Q}b{Q}c."), format!("{Q}{Q}?{Q}UNC{Q}a{Q}b{Q}c.")),
        (
            format!("{Q}{Q}.x{Q}share{Q}d."),
            format!("{Q}{Q}?{Q}UNC{Q}.x{Q}share{Q}d."),
        ),
        (format!("{Q}{Q}?x{Q}s{Q}d "), format!("{Q}{Q}?{Q}UNC{Q}?x{Q}s{Q}d ")),
        (format!("C:{Q}x."), format!("{Q}{Q}?{Q}C:{Q}x.")),
        (format!("C:{Q}x "), format!("{Q}{Q}?{Q}C:{Q}x ")),
        // Device paths and short paths are returned as spelled.
        (format!("{Q}{Q}?{Q}C:{Q}x."), format!("{Q}{Q}?{Q}C:{Q}x.")),
        (format!("{Q}{Q}.{Q}C:{Q}x."), format!("{Q}{Q}.{Q}C:{Q}x.")),
        (format!("{Q}??{Q}C:{Q}x."), format!("{Q}??{Q}C:{Q}x.")),
        (format!("C:{Q}x"), format!("C:{Q}x")),
        (format!("{Q}{Q}srv{Q}share{Q}x"), format!("{Q}{Q}srv{Q}share{Q}x")),
        // A relative path is not fully qualified: unchanged even when it ends with a dot.
        (format!("rel{Q}x."), format!("rel{Q}x.")),
    ];
    for (path, want) in &cases {
        assert_eq!(&ensure_extended_prefix_if_needed_for_tests(path), want, "{path:?}");
    }
    // 260 units or longer: the extended form without any trailing dot or space.
    let long_unc = format!("{Q}{Q}srv{Q}share{Q}{}", "d".repeat(260));
    assert_eq!(
        ensure_extended_prefix_if_needed_for_tests(&long_unc),
        format!("{Q}{Q}?{Q}UNC{Q}srv{Q}share{Q}{}", "d".repeat(260))
    );
    let long_drive = format!("C:{Q}{}", "d".repeat(260));
    assert_eq!(
        ensure_extended_prefix_if_needed_for_tests(&long_drive),
        format!("{Q}{Q}?{Q}{long_drive}")
    );
}

// C#: Directory.CreateDirectory reports the failure of the folder it was asked to create. Creating a single level directly
// under an EXISTING drive root with an invalid name (`|`) fails with raw 123; physical.rs:721:19 `count == 0 &&
// !some_path_exists` -> `||` would take the "nothing to create" branch (root exists) and report Ok(()). The name can never
// be created on NTFS, so the attempt changes nothing on the drive; the test asserts that too. The drive is the one of the
// private TempRoot; nothing is created under the root and nothing outside the TempRoot is written.
#[test]
#[ignore = "native: the only path touched is an invalid name that cannot be created at the drive root"]
fn create_directory_with_an_invalid_name_directly_under_an_existing_root_reports_the_failure() {
    let root = TempRoot::new("phys6-rootfail");
    let text = root.path().to_string_lossy().into_owned();
    let bytes = text.as_bytes();
    if bytes.len() < 3 || bytes[1] != b':' || bytes[2] != b'\\' || !bytes[0].is_ascii_alphabetic() {
        // Not a plain `X:\` path (UNC or extended): the scenario needs the root of a local drive. Mutant 721:19 is NOT
        // checked on such a machine.
        eprintln!("SKIPPED: the temp folder {text:?} is not on a plain local drive path");
        return;
    }
    let drive_root = format!("{}:{Q}", char::from(bytes[0]));
    assert!(Path::new(&drive_root).is_dir());
    let invalid = format!("{drive_root}a|b");
    // A file system driver that accepts '|' (not NTFS) would create the folder: remove it again, whatever the asserts
    // say. `remove_dir` removes an empty folder only, and the name did not exist before (checked just below).
    struct RemoveIfCreated(PathBuf);
    impl Drop for RemoveIfCreated {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir(&self.0);
        }
    }
    assert!(
        !Path::new(&invalid).exists(),
        "{invalid} must not exist before the test"
    );
    let _cleanup = RemoveIfCreated(PathBuf::from(&invalid));
    for spelled in [invalid.clone(), format!("{invalid}{Q}")] {
        let path = PathBuf::from(&spelled);
        let err = bounded("invalid name under the root", move || {
            PhysicalFileSystem.create_directory(&path)
        })
        .expect_err("a name containing '|' cannot be created");
        assert_eq!(err.raw_os_error(), Some(123), "{spelled}");
        assert!(!Path::new(&invalid).exists(), "{spelled}: nothing may be created");
    }
    assert!(
        !std::fs::read_dir(&drive_root)
            .unwrap()
            .any(|e| e.unwrap().file_name().to_string_lossy().contains('|')),
        "no entry with an invalid name can exist on the drive root"
    );
}

// C#: FileSystemEnumerator.CreateDirectoryHandle opens the folder with share ReadWrite|Delete, so the listing works while
// another handle holds the folder with WRITE or DELETE access. physical.rs:880:56 `FILE_SHARE_WRITE | FILE_SHARE_DELETE`
// -> `&` leaves share READ only (`1 | (2 & 4)`), which conflicts with a handle that has DELETE or write access (the read
// handle of `w2_phys5_killers` does not conflict with it).
#[test]
#[ignore = "native: private TempRoot folder held open by a second handle"]
fn enumerate_works_while_another_handle_holds_the_folder_with_delete_or_write_access() {
    use std::os::windows::fs::OpenOptionsExt;
    const DELETE: u32 = 0x0001_0000;
    const FILE_ADD_FILE: u32 = 0x2;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    for (label, access) in [("delete", DELETE), ("write", FILE_ADD_FILE)] {
        let root = TempRoot::new("phys6-listing");
        std::fs::write(root.path().join("a.txt"), b"x").unwrap();
        std::fs::create_dir(root.path().join("sub")).unwrap();
        let _held = std::fs::OpenOptions::new()
            .access_mode(access)
            .share_mode(7)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(root.path())
            .unwrap_or_else(|e| panic!("{label}: the test handle must open: {e}"));
        let files = PhysicalFileSystem
            .enumerate_files(root.path(), "*")
            .unwrap_or_else(|e| panic!("{label}: listing files must share with the holder: {e}"));
        assert_eq!(files, vec![root.path().join("a.txt")], "{label}");
        let folders = PhysicalFileSystem
            .enumerate_directories(root.path())
            .unwrap_or_else(|e| panic!("{label}: listing folders must share with the holder: {e}"));
        assert_eq!(folders, vec![root.path().join("sub")], "{label}");
    }
}

// C#: ResolveIfReparsePoint resolves only an entry that HAS the REPARSE_POINT attribute (and whose link .NET can read): the
// test is `attributes & 0x400 == 0`. physical.rs:957:19 `&` -> `^` is true only for the attributes `0x400` alone (a file
// symlink with its archive bit cleared), which it would return unresolved, and false for 0 / 0x10 / 0x20, which it would
// resolve; `0x410` and `0x420` (junction / directory and file symlinks) behave the same either way.
#[test]
fn resolve_if_reparse_point_resolves_exactly_the_entries_with_the_reparse_attribute() {
    const REPARSE: u32 = 0x400;
    let outcome = |attributes: Option<u32>, readable: bool| {
        let mut walked = 0;
        let mut probed = 0;
        let got = resolve_if_reparse_point_for_tests(
            "C:\\cand".to_owned(),
            &mut |p| {
                assert_eq!(p, "C:\\cand");
                attributes
            },
            &mut |p| {
                assert_eq!(p, "C:\\cand");
                probed += 1;
                readable
            },
            &mut |p| {
                assert_eq!(p, "C:\\cand");
                walked += 1;
                Ok("D:\\final".to_owned())
            },
        )
        .unwrap();
        (got, probed, walked)
    };
    let resolved = ("D:\\final".to_owned(), 1, 1);
    for attributes in [
        REPARSE,
        REPARSE | 0x10,
        REPARSE | 0x20,
        REPARSE | 0x20 | 0x2,
        0xFFFF_FFFF,
    ] {
        assert_eq!(outcome(Some(attributes), true), resolved, "attributes {attributes:#x}");
    }
    // No REPARSE_POINT attribute: unchanged, the link is never probed or walked.
    for attributes in [0, 0x10, 0x20, 0x30, 0x1, 0x2] {
        assert_eq!(
            outcome(Some(attributes), true),
            ("C:\\cand".to_owned(), 0, 0),
            "attributes {attributes:#x}"
        );
    }
    // Missing entry: unchanged without a probe.
    assert_eq!(outcome(None, true), ("C:\\cand".to_owned(), 0, 0));
    // A reparse point .NET cannot read (cloud placeholder, dedup ...): unchanged after one probe, not walked.
    assert_eq!(outcome(Some(0x420), false), ("C:\\cand".to_owned(), 1, 0));
    // The walk's error is the caller's error.
    let err = resolve_if_reparse_point_for_tests(
        "C:\\cand".to_owned(),
        &mut |_| Some(REPARSE),
        &mut |_| true,
        &mut |_| Err(io::Error::from_raw_os_error(1921)),
    )
    .unwrap_err();
    assert_eq!(err.raw_os_error(), Some(1921));
}

fn overwrite_through_a_second_handle(path: &Path, bytes: &[u8]) {
    let mut writer = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    writer.write_all(bytes).unwrap();
    writer.flush().unwrap();
}

// C#: `new FileStream(path, Open, Read, ReadWrite|Delete, bufferSize)`: a buffer size above 1 buffers, 0 and 1 do not.
// physical.rs:1364:24 `buffer_size > 1` -> `==` / `<` (sizes 2+ lose the buffer), `>=` (size 1 gets a one-byte BufReader).
// Buffering is observable through staleness: the first read fills the buffer, so bytes another handle writes afterwards
// are not seen until the buffer is used up, whereas an unbuffered file reads the new bytes.
#[test]
#[ignore = "native: private TempRoot reads and a second writer handle"]
fn open_read_shared_buffers_only_above_one_byte() {
    let root = TempRoot::new("phys6-buffer");
    for (size, buffered) in [(2usize, true), (64, true), (4096, true), (0, false), (1, false)] {
        let path = root.path().join(format!("f{size}.bin"));
        std::fs::write(&path, b"AAAAAAAA").unwrap();
        let mut reader = PhysicalFileSystem.open_read_shared(&path, size).unwrap();
        let mut first = [0u8; 1];
        reader.read_exact(&mut first).unwrap();
        assert_eq!(&first, b"A");
        overwrite_through_a_second_handle(&path, b"BBBBBBBB");
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        // The first read pulled `min(size, 8)` bytes into the buffer; the 7 bytes after the one read come from that stale
        // buffer first and from the new content afterwards.
        let stale = if buffered { size.min(8) - 1 } else { 0 };
        let want = [vec![b'A'; stale], vec![b'B'; 7 - stale]].concat();
        assert_eq!(rest, want, "buffer size {size}");
    }
}

// The same difference for size 1 with a zero-length read: a one-byte BufReader (the `>=` mutant) fills its buffer even for an
// empty request, so the next read serves the stale byte, whereas the raw file the real code returns reads the new one.
#[test]
#[ignore = "native: private TempRoot reads and a second writer handle"]
fn open_read_shared_with_buffer_size_one_is_an_unbuffered_file() {
    let root = TempRoot::new("phys6-buffer-one");
    for size in [0usize, 1] {
        let path = root.path().join(format!("g{size}.bin"));
        std::fs::write(&path, b"AAAA").unwrap();
        let mut reader = PhysicalFileSystem.open_read_shared(&path, size).unwrap();
        assert_eq!(reader.read(&mut []).unwrap(), 0);
        overwrite_through_a_second_handle(&path, b"BBBB");
        let mut one = [0u8; 1];
        reader.read_exact(&mut one).unwrap();
        assert_eq!(&one, b"B", "buffer size {size}");
    }
}
