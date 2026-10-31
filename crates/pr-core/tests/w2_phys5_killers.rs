//! Physical-filesystem mutation guards (CL-PHYS5). TempRoot / pure functions, no sleeps, no clock.
//! Port of PhotoReview.Core/IO/PhysicalFileSystem.cs (`FileSystemName.MatchesWin32Expression` semantics).

use pr_core::abstractions::FileSystem;
use pr_core::io::physical::{PhysicalFileSystem, matches_win32_expression};
use pr_test_support::TempRoot;
use std::sync::mpsc;
use std::time::Duration;

// C#: FileSystemName.MatchesWin32Expression always terminates (the NFA consumes the name one unit per step).
// physical.rs:267 `name_offset += 1`, :297 and :308 `offset += 1` -> `*=` never advance and loop forever; a bounded
// channel wait (not a sleep) turns that hang into a prompt assertion failure instead of a 300 s cargo-mutants timeout.
#[test]
fn matches_win32_expression_terminates_and_answers_for_small_inputs() {
    let cases: [(&str, &str, bool); 11] = [
        ("a", "a", true),  // :267 name_offset
        ("a?", "a", true), // :297 DOS_QM at the end of the name
        ("a?b", "axb", true),
        ("*.", "a", true),  // :308 DOS_DOT beyond the end of the name
        ("a*.", "a", true), // :308 again after a literal
        ("a*.b", "a.b", true),
        ("a?", "ab", true),
        ("a?", "abc", false),
        ("a.*", "a", true), // :308 DOS_DOT (".*") with the name already finished
        ("a.?", "a", true),
        ("a.*", "a.b", true),
    ];
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let got: Vec<bool> = cases.iter().map(|(e, n, _)| matches_win32_expression(e, n)).collect();
        let _ = tx.send(got);
    });
    let got = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("matches_win32_expression must terminate on tiny inputs");
    for ((e, n, want), got) in cases.iter().zip(got) {
        assert_eq!(got, *want, "expression {e:?} name {n:?}");
    }
}

// C#: PathInternal.IsDevice / EnsureExtendedPrefix. A device-form path (`Q Q ? Q`, `Q ? ? Q`) is used as spelled, so a
// trailing '.' or ' ' survives; every other form is normalised by GetFullPath, which trims the trailing '.' (' ' kept).
// physical.rs:490-496 `is_device` (operator mutants) via create_directory/directory_exists on dot- and space-ending names.
#[test]
fn create_directory_device_forms_keep_trailing_dot_and_other_forms_trim_it() {
    let root = TempRoot::new("phys5-device");
    let base = std::fs::canonicalize(root.path()).unwrap();
    let q = "\u{5c}";
    let b = base.to_string_lossy().into_owned();
    let plain = b.strip_prefix(&format!("{q}{q}?{q}")).unwrap_or(&b).to_owned();
    let fs = PhysicalFileSystem;
    let forms = [
        format!("{q}{q}?{q}{plain}{q}s1."), // extended device: dot kept
        format!("{q}{q}.{q}{plain}{q}s2."), // dot device: normalised, dot trimmed
        format!("//?/{plain}/s3."),
        format!("//./{plain}/s4."),
        format!("{q}??{q}{plain}{q}s5."),   // NT device: dot kept
        format!("{q}{q}?{q}{plain}{q}s6 "), // space kept
        format!("{q}/?{q}{plain}{q}s8."),
        format!("/{q}.{q}{plain}{q}s9."),
        format!("{q}{q}?{q}{plain}{q}s10"),
        format!("{plain}{q}s11."), // plain drive path: dot trimmed
    ];
    for form in &forms {
        let path = std::path::PathBuf::from(form);
        fs.create_directory(&path).unwrap_or_else(|e| panic!("{form}: {e}"));
        assert!(fs.directory_exists(&path), "{form}");
    }
    let mut names: Vec<String> = std::fs::read_dir(&base)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["s1.", "s10", "s11", "s2", "s3", "s4", "s5.", "s6 ", "s8", "s9"]);
}

// C#: SafeFileHandle.Open reports "path not found" on a bare root as access denied (raw 5) but stays raw 3 under a root.
// physical.rs:634 match guard `&&` / `==` in open_like_file_stream (a drive letter that has no volume reaches raw 3).
#[test]
fn open_read_shared_file_on_a_missing_drive_root_is_access_denied_and_below_it_not_found() {
    let q = "\u{5c}";
    let letter = ('D'..='Z').find(|c| !std::path::Path::new(&format!("{c}:{q}")).exists());
    let Some(letter) = letter else { return };
    let fs = PhysicalFileSystem;
    let bare = std::path::PathBuf::from(format!("{letter}:{q}"));
    let below = std::path::PathBuf::from(format!("{letter}:{q}sub"));
    assert_eq!(fs.open_read_shared_file(&bare).unwrap_err().raw_os_error(), Some(5));
    assert_eq!(fs.open_read_shared_file(&below).unwrap_err().raw_os_error(), Some(3));
    assert_eq!(fs.read_all_text(&bare).unwrap_err().raw_os_error(), Some(5));
    assert_eq!(fs.read_all_text(&below).unwrap_err().raw_os_error(), Some(3));
}

// C#: Directory.CreateDirectory creates every missing level (also with a trailing separator or `/`) and returns.
// physical.rs:678 `length -= 1` -> `/=` (half the path walked) and :694 / :696 `i -= 1` -> `/=` / `+=` (the walk back never
// ends). A bounded channel wait turns the hang into a prompt failure instead of a 300 s cargo-mutants timeout.
#[test]
fn create_directory_deep_paths_with_trailing_or_forward_separators_create_the_last_level() {
    let root = TempRoot::new("phys5-mkdir");
    let base = root.path().to_path_buf();
    let q = "\u{5c}";
    let (tx, rx) = mpsc::channel();
    let worker_base = base.clone();
    std::thread::spawn(move || {
        let b = worker_base.to_string_lossy().into_owned();
        let forms = [
            format!("{b}{q}aaaa{q}bbbb{q}cccc{q}"),
            format!("{b}{q}dd{q}eeeeeeee{q}ff"),
            format!("{b}/gg/hhhhhh/ii/"),
            format!("{b}{q}j"),
        ];
        let fs = PhysicalFileSystem;
        let results: Vec<bool> = forms
            .iter()
            .map(|f| fs.create_directory(std::path::Path::new(f)).is_ok())
            .collect();
        let _ = tx.send(results);
    });
    let results = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("create_directory must terminate");
    assert_eq!(results, [true, true, true, true]);
    assert!(base.join("aaaa").join("bbbb").join("cccc").is_dir());
    assert!(base.join("dd").join("eeeeeeee").join("ff").is_dir());
    assert!(base.join("gg").join("hhhhhh").join("ii").is_dir());
    assert!(base.join("j").is_dir());
}

// C#: FileSystemEnumerator.CreateDirectoryHandle opens with share ReadWrite|Delete, so listing works while another handle
// on the same folder is open. physical.rs:880 `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE` -> `&` (share none).
#[test]
fn enumerate_files_works_while_another_handle_holds_the_folder_open() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = TempRoot::new("phys5-listing");
    std::fs::write(root.path().join("a.txt"), b"x").unwrap();
    let _held = std::fs::OpenOptions::new()
        .access_mode(0x1)
        .share_mode(7)
        .custom_flags(0x0200_0000)
        .open(root.path())
        .unwrap();
    let listed = PhysicalFileSystem.enumerate_files(root.path(), "*").unwrap();
    assert_eq!(listed.len(), 1);
}

// physical.rs:885 `err.raw_os_error() == Some(ERROR_FILE_NOT_FOUND)` -> true: only "file not found" becomes "path not
// found" (raw 3); every other open failure (here an invalid name, raw 123) is reported as the OS gave it.
#[test]
fn enumerate_files_on_an_invalid_folder_name_reports_the_os_error_not_path_not_found() {
    let root = TempRoot::new("phys5-listing");
    let got = PhysicalFileSystem.enumerate_files(&root.path().join("a|b"), "*");
    let code = got.unwrap_err().raw_os_error();
    assert_ne!(code, Some(3));
    assert_eq!(code, Some(123));
}
