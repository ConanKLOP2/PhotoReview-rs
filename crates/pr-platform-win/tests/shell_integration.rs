//! Port of `tests/PhotoReview.App.Tests/HotPath/NativeShellIntegrationTests.cs` (baseline `d92d6aae`), W8-09b slice S2.
//!
//! The real registry behind Explorer's "Browse with PhotoReviewRs", but ONLY on a PRIVATE key `HKCU\Software\PhotoReviewRsTests\<unique>\Classes`
//! (never the user's real `Software\Classes`, never the C# app's `PhotoReview` verb): the whole private tree is deleted by a `Drop` guard.
//! Every test is `#[ignore = "native: registry"]`, runs on its own thread and is bounded by 30 s (hang guard).
#![cfg(windows)]

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pr_core::abstractions::{ShellIntegration, ShellMenuState, shell_menu_command};
use pr_platform_win::shell_integration::{BlankClassesKey, DEFAULT_CLASSES_KEY, WindowsShellIntegration};
use windows::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE, RegCloseKey,
    RegCreateKeyExW, RegDeleteKeyW, RegDeleteTreeW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::PCWSTR;

const EXE: &str = r"C:\Apps\Photo Review\PhotoReviewRs.exe";
const TEST_ROOT_PREFIX: &str = r"Software\PhotoReviewRsTests";
const BOUND: Duration = Duration::from_secs(30);

/// Runs `body` on its own thread and fails the test when it does not finish within [`BOUND`] (or panics).
fn bounded(body: impl FnOnce() + Send + 'static) {
    let (sender, receiver) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        body();
        let _ = sender.send(());
    });
    match receiver.recv_timeout(BOUND) {
        Ok(()) => handle.join().expect("test thread"),
        Err(mpsc::RecvTimeoutError::Disconnected) => match handle.join() {
            Ok(()) => panic!("test thread ended without a result"),
            Err(panic) => std::panic::resume_unwind(panic),
        },
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("native shell integration test exceeded {BOUND:?}"),
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn ok(status: WIN32_ERROR) -> bool {
    status == ERROR_SUCCESS
}

/// An open key for the test helpers, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: this wrapper exclusively owns the handle from `RegOpenKeyExW` / `RegCreateKeyExW` and closes it once.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

fn open_read(path: &str) -> Option<Key> {
    let path = wide(path);
    let mut opened = HKEY::default();
    // SAFETY: `path` is a live NUL-terminated UTF-16 buffer and `opened` a writable HKEY.
    let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), None, KEY_READ, &mut opened) };
    ok(status).then_some(Key(opened))
}

/// `Registry.CurrentUser.OpenSubKey(path)?.GetValue(name) as string`; `None` for a missing key, a missing value or a non-string value.
fn read_string(path: &str, name: Option<&str>) -> Option<String> {
    let key = open_read(path)?;
    let name = name.map(wide);
    let name_ptr = name.as_ref().map_or(PCWSTR::null(), |buffer| PCWSTR(buffer.as_ptr()));
    let mut kind = REG_VALUE_TYPE(0);
    let mut size = 0u32;
    // SAFETY: null data asks for the type and the byte count only; the name is null or a live NUL-terminated buffer.
    let status = unsafe { RegQueryValueExW(key.0, name_ptr, None, Some(&mut kind), None, Some(&mut size)) };
    if !ok(status) || kind != REG_SZ {
        return None;
    }
    let mut buffer = vec![0u8; size as usize];
    let mut capacity = size;
    // SAFETY: `buffer` is a live writable allocation of `capacity` bytes, passed as the size.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            name_ptr,
            None,
            Some(&mut kind),
            Some(buffer.as_mut_ptr()),
            Some(&mut capacity),
        )
    };
    if !ok(status) {
        return None;
    }
    let units: Vec<u16> = buffer[..capacity as usize]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// `Registry.CurrentUser.CreateSubKey(path)` + `SetValue(null, value)`: a neighbour verb the integration must never touch.
fn write_default_string(path: &str, value: &str) {
    assert!(
        path.starts_with(TEST_ROOT_PREFIX),
        "tests write only below {TEST_ROOT_PREFIX}"
    );
    let path = wide(path);
    let mut created = HKEY::default();
    // SAFETY: `path` is a live NUL-terminated buffer, the class is null, `created` is a writable HKEY.
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE | KEY_READ,
            None,
            &mut created,
            None,
        )
    };
    assert!(ok(status), "create test key: {status:?}");
    let key = Key(created);
    let bytes: Vec<u8> = wide(value).iter().flat_map(|unit| unit.to_le_bytes()).collect();
    // SAFETY: `key` is open with write access, the name is null (default value) and `bytes` is a live slice whose length is passed.
    let status = unsafe { RegSetValueExW(key.0, PCWSTR::null(), None, REG_SZ, Some(&bytes)) };
    assert!(ok(status), "set test value: {status:?}");
}

/// A private registry tree `HKCU\Software\PhotoReviewRsTests\<unique>`, deleted when dropped.
struct PrivateRoot {
    root: String,
}

impl PrivateRoot {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let unique = format!(
            "{}-{}-{nanos}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        Self {
            root: format!(r"{TEST_ROOT_PREFIX}\{unique}"),
        }
    }

    fn classes(&self) -> String {
        format!(r"{}\Classes", self.root)
    }

    fn shell(&self) -> WindowsShellIntegration {
        WindowsShellIntegration::with_classes_key(&self.classes()).expect("a non-blank key")
    }

    /// C# `Read(verbPath, valueName, command)`.
    fn read(&self, verb_path: &str, value_name: Option<&str>, command: bool) -> Option<String> {
        let suffix = if command { r"\command" } else { "" };
        read_string(&format!(r"{}\{verb_path}{suffix}", self.classes()), value_name)
    }
}

impl Drop for PrivateRoot {
    fn drop(&mut self) {
        // Only ever the private tree: refuse anything that is not below the test prefix.
        assert!(
            self.root.starts_with(&format!(r"{TEST_ROOT_PREFIX}\")),
            "refusing to delete {}",
            self.root
        );
        let root = wide(&self.root);
        // SAFETY: `root` is a live NUL-terminated buffer naming this test's private tree below HKCU.
        let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(root.as_ptr())) };
        let ancestor = wide(&self.root[..TEST_ROOT_PREFIX.len()]);
        // SAFETY: `ancestor` is the shared test prefix key; `RegDeleteKeyW` removes it only when it has no subkeys left (another running test keeps it), failure is ignored.
        let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(ancestor.as_ptr())) };
    }
}

fn verb(prefix: &str) -> String {
    format!(r"{prefix}\{}", shell_menu_command::VERB_KEY_NAME)
}

fn folder_verb() -> String {
    verb(r"Directory\shell")
}

fn background_verb() -> String {
    verb(r"Directory\Background\shell")
}

// C#: NativeShellIntegrationTests.Nothing_is_registered_on_a_fresh_key
#[test]
#[ignore = "native: registry"]
fn nothing_is_registered_on_a_fresh_key() {
    bounded(|| {
        let root = PrivateRoot::new();
        assert_eq!(root.shell().get_state(EXE), ShellMenuState::NotRegistered);
    });
}

// C#: NativeShellIntegrationTests.Register_writes_the_folder_and_background_verbs_with_text_icon_and_command
#[test]
#[ignore = "native: registry"]
fn register_writes_the_folder_and_background_verbs_with_text_icon_and_command() {
    bounded(|| {
        let root = PrivateRoot::new();
        let shell = root.shell();

        shell.register(EXE).expect("register");

        assert_eq!(shell.get_state(EXE), ShellMenuState::Registered);
        assert_eq!(
            root.read(&folder_verb(), None, false).as_deref(),
            Some(shell_menu_command::MENU_TEXT)
        );
        assert_eq!(
            root.read(&background_verb(), None, false).as_deref(),
            Some(shell_menu_command::MENU_TEXT)
        );
        assert_eq!(
            root.read(&folder_verb(), Some("Icon"), false).as_deref(),
            Some(format!("\"{EXE}\",0").as_str())
        );
        assert_eq!(
            root.read(&folder_verb(), None, true).as_deref(),
            Some(format!("\"{EXE}\" \"%1\"").as_str())
        );
        assert_eq!(
            root.read(&background_verb(), None, true).as_deref(),
            Some(format!("\"{EXE}\" \"%V\"").as_str())
        );
    });
}

// C#: NativeShellIntegrationTests.Register_again_for_a_moved_exe_repairs_the_path
#[test]
#[ignore = "native: registry"]
fn register_again_for_a_moved_exe_repairs_the_path() {
    bounded(|| {
        const MOVED: &str = r"D:\Moved\PhotoReviewRs.exe";
        let root = PrivateRoot::new();
        let shell = root.shell();
        shell.register(EXE).expect("register");
        assert_eq!(shell.get_state(MOVED), ShellMenuState::RegisteredElsewhere);

        shell.register(MOVED).expect("register moved");

        assert_eq!(shell.get_state(MOVED), ShellMenuState::Registered);
        assert_eq!(shell.get_state(EXE), ShellMenuState::RegisteredElsewhere);
    });
}

// C#: NativeShellIntegrationTests.Unregister_removes_only_the_two_PhotoReview_verbs
#[test]
#[ignore = "native: registry"]
fn unregister_removes_only_the_two_verbs() {
    bounded(|| {
        let root = PrivateRoot::new();
        write_default_string(
            &format!(r"{}\Directory\shell\OtherTool\command", root.classes()),
            "other.exe \"%1\"",
        );
        let shell = root.shell();
        shell.register(EXE).expect("register");

        shell.unregister().expect("unregister");

        assert_eq!(shell.get_state(EXE), ShellMenuState::NotRegistered);
        assert_eq!(root.read(&folder_verb(), None, false), None);
        assert_eq!(root.read(&background_verb(), None, false), None);
        // A neighbour verb survives.
        assert_eq!(
            root.read(r"Directory\shell\OtherTool", None, true).as_deref(),
            Some("other.exe \"%1\"")
        );
    });
}

// C#: NativeShellIntegrationTests.Unregister_when_nothing_is_registered_is_a_no_op
#[test]
#[ignore = "native: registry"]
fn unregister_when_nothing_is_registered_is_a_no_op() {
    bounded(|| {
        let root = PrivateRoot::new();
        let shell = root.shell();

        shell.unregister().expect("no-op on an absent classes key");
        assert_eq!(shell.get_state(EXE), ShellMenuState::NotRegistered);

        // And with the classes key present but no verbs below it.
        write_default_string(
            &format!(r"{}\Directory\shell\OtherTool\command", root.classes()),
            "other.exe",
        );
        shell.unregister().expect("no-op with a neighbour only");
        assert_eq!(shell.get_state(EXE), ShellMenuState::NotRegistered);
    });
}

// C#: NativeShellIntegrationTests.Register_rejects_an_unusable_path_and_writes_nothing (rows "", "relative.exe", "C:\bad\"quote.exe")
#[test]
#[ignore = "native: registry"]
fn register_rejects_an_unusable_path_and_writes_nothing() {
    bounded(|| {
        for bad in ["", "relative.exe", "C:\\bad\"quote.exe"] {
            let root = PrivateRoot::new();
            let shell = root.shell();

            let error = shell.register(bad).expect_err(bad);

            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{bad}");
            assert_eq!(shell.get_state(EXE), ShellMenuState::NotRegistered, "{bad}");
            assert_eq!(root.read(&folder_verb(), None, false), None, "{bad}");
            assert!(
                open_read(&root.classes()).is_none(),
                "nothing at all is created for {bad}"
            );
        }
    });
}

// new: a trailing backslash on the classes key is trimmed, so the same private tree is reached.
#[test]
#[ignore = "native: registry"]
fn a_classes_key_with_trailing_backslashes_reaches_the_same_tree() {
    bounded(|| {
        let root = PrivateRoot::new();
        let trailing = WindowsShellIntegration::with_classes_key(&format!(r"{}\\", root.classes())).expect("key");
        trailing.register(EXE).expect("register");
        assert_eq!(root.shell().get_state(EXE), ShellMenuState::Registered);
    });
}

// new: the key constructors (no registry access: `new` and the blank check never touch it).
#[test]
fn constructors_reject_a_blank_key_and_the_default_is_the_real_classes_root() {
    assert_eq!(DEFAULT_CLASSES_KEY, r"Software\Classes");
    assert_eq!(WindowsShellIntegration::with_classes_key("  "), Err(BlankClassesKey));
    assert_eq!(WindowsShellIntegration::new(), WindowsShellIntegration::default());
}
