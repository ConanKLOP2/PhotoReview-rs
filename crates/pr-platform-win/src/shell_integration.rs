//! Port of `PhotoReview.Platform.Windows/WindowsShellIntegration.cs` (77 lines, baseline `d92d6aae`): `ShellIntegration` over `HKEY_CURRENT_USER\Software\Classes`
//! (W8-09b slice S2, `contracts/W8-09b.md`, decision D-W8B6-151; no earlier unit owned it: `pr_core::abstractions::shell_integration` holds only the trait and the pure
//! `shell_menu_command` values).
//!
//! Writes only `Directory\shell\<VERB>` and `Directory\Background\shell\<VERB>` below the classes root, where `<VERB>` is `shell_menu_command::VERB_KEY_NAME` (`PhotoReviewRs`,
//! R0002: never the C# app's `PhotoReview` key). The root is a constructor argument so tests use a PRIVATE key (`Software\PhotoReviewRsTests\<guid>\Classes`) instead of the
//! user's real one. Per user, no administrator rights. UI thread or worker, no thread affinity.
use std::fmt;
use std::io;

use pr_core::abstractions::{ShellIntegration, ShellMenuState, shell_menu_command};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_ALL_ACCESS, KEY_READ, KEY_WRITE, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SZ,
    REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::PCWSTR;

/// C# `DefaultClassesKey`: the real per-user classes root, relative to HKCU.
pub const DEFAULT_CLASSES_KEY: &str = r"Software\Classes";

/// C# `ArgumentException("A registry key path is required.")`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlankClassesKey;

impl fmt::Display for BlankClassesKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("A registry key path is required.")
    }
}

impl std::error::Error for BlankClassesKey {}

/// C# `WindowsShellIntegration`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsShellIntegration {
    classes_key: String,
}

/// C# `FolderVerbPath`: `Directory\shell\<VERB>`.
fn folder_verb_path() -> String {
    format!(r"Directory\shell\{}", shell_menu_command::VERB_KEY_NAME)
}

/// C# `BackgroundVerbPath`: `Directory\Background\shell\<VERB>`.
fn background_verb_path() -> String {
    format!(r"Directory\Background\shell\{}", shell_menu_command::VERB_KEY_NAME)
}

/// The registry path (below HKCU) of one verb under a classes root: C# `_classesKey + "\\" + verbPath`.
fn join_key(classes_key: &str, relative: &str) -> String {
    format!(r"{classes_key}\{relative}")
}

/// A NUL-terminated UTF-16 copy of `value` for the registry APIs.
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The bytes of a `REG_SZ` value: UTF-16 little endian including the terminating NUL.
fn reg_sz_bytes(value: &str) -> Vec<u8> {
    wide(value).iter().flat_map(|unit| unit.to_le_bytes()).collect()
}

/// A `REG_SZ` payload back to text: UTF-16 little endian, cut at the first NUL (a value stored without a terminator is read whole).
fn decode_reg_sz(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// A Win32 registry status as the `io::Error` the trait returns (the C# `IOException` / `UnauthorizedAccessException`).
fn status_to_result(status: WIN32_ERROR) -> io::Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status.0 as i32))
    }
}

/// An open registry key, closed on drop.
struct KeyGuard(HKEY);

impl Drop for KeyGuard {
    fn drop(&mut self) {
        // SAFETY: this guard exclusively owns the handle it was built from (an open key from `RegOpenKeyExW` / `RegCreateKeyExW`) and closes it exactly once.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

/// `Registry.CurrentUser.OpenSubKey(path, writable)`: `Ok(None)` when the key does not exist.
fn open_key(path: &str, access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> io::Result<Option<KeyGuard>> {
    let path = wide(path);
    let mut opened = HKEY::default();
    // SAFETY: `path` is a live NUL-terminated UTF-16 buffer and `opened` a writable HKEY for the duration of the call.
    let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), None, access, &mut opened) };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    status_to_result(status)?;
    Ok(Some(KeyGuard(opened)))
}

/// `Registry.CurrentUser.CreateSubKey(path, writable: true)`.
fn create_key(path: &str) -> io::Result<KeyGuard> {
    let path = wide(path);
    let mut created = HKEY::default();
    // SAFETY: `path` is a live NUL-terminated UTF-16 buffer, the class string is null (allowed), and `created` is a writable HKEY for the duration of the call.
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
    status_to_result(status)?;
    Ok(KeyGuard(created))
}

/// `key.SetValue(name, value, RegistryValueKind.String)`; `name == None` is the default value.
fn set_string(key: &KeyGuard, name: Option<&str>, value: &str) -> io::Result<()> {
    let name = name.map(wide);
    let name_ptr = name.as_ref().map_or(PCWSTR::null(), |buffer| PCWSTR(buffer.as_ptr()));
    let data = reg_sz_bytes(value);
    // SAFETY: `key` is an open key with write access; `name_ptr` is null or points into the live NUL-terminated `name`; `data` is a live byte slice whose length is passed with the call.
    let status = unsafe { RegSetValueExW(key.0, name_ptr, None, REG_SZ, Some(&data)) };
    status_to_result(status)
}

/// `key.GetValue(null) as string`: the default value when it is a string, else `None`.
fn read_default_string(key: &KeyGuard) -> Option<String> {
    let mut kind = REG_VALUE_TYPE(0);
    let mut size = 0u32;
    // SAFETY: null data asks the API for the type and the byte count only; `kind` and `size` are writable locals; the value name is null (the default value).
    let status = unsafe { RegQueryValueExW(key.0, PCWSTR::null(), None, Some(&mut kind), None, Some(&mut size)) };
    if status != ERROR_SUCCESS || (kind != REG_SZ && kind != REG_EXPAND_SZ) {
        return None;
    }
    // Two spare bytes so a value stored without its terminator still decodes whole.
    let mut buffer = vec![0u8; size as usize + 2];
    let mut capacity = size;
    // SAFETY: `buffer` is a live writable allocation of at least `capacity` bytes and `capacity` is passed as its size; the API writes at most that many bytes and updates it.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR::null(),
            None,
            Some(&mut kind),
            Some(buffer.as_mut_ptr()),
            Some(&mut capacity),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let written = (capacity as usize).min(buffer.len());
    Some(decode_reg_sz(&buffer[..written]))
}

/// `key.DeleteSubKeyTree(relative, throwOnMissingSubKey: false)`.
fn delete_tree(root: &KeyGuard, relative: &str) -> io::Result<()> {
    let relative = wide(relative);
    // SAFETY: `root` is an open key with full access and `relative` is a live NUL-terminated UTF-16 buffer.
    let status = unsafe { RegDeleteTreeW(root.0, PCWSTR(relative.as_ptr())) };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(());
    }
    status_to_result(status)
}

impl WindowsShellIntegration {
    /// C# `new WindowsShellIntegration()`: [`DEFAULT_CLASSES_KEY`].
    pub fn new() -> Self {
        Self {
            classes_key: DEFAULT_CLASSES_KEY.to_owned(),
        }
    }

    /// C# `new WindowsShellIntegration(classesKey)`: `Err(BlankClassesKey)` for a blank key; trailing `\` characters are trimmed.
    pub fn with_classes_key(classes_key: &str) -> Result<Self, BlankClassesKey> {
        if classes_key.trim().is_empty() {
            return Err(BlankClassesKey);
        }
        Ok(Self {
            classes_key: classes_key.trim_end_matches('\\').to_owned(),
        })
    }

    /// C# `ReadCommand`: the default value of `<classes>\<verb>\command`.
    fn read_command(&self, verb_path: &str) -> Option<String> {
        let path = join_key(&self.classes_key, &format!(r"{verb_path}\command"));
        let key = open_key(&path, KEY_READ).ok()??;
        read_default_string(&key)
    }

    /// C# `WriteVerb`.
    fn write_verb(&self, verb_path: &str, executable_path: &str, command: &str) -> io::Result<()> {
        let verb = create_key(&join_key(&self.classes_key, verb_path))?;
        set_string(&verb, None, shell_menu_command::MENU_TEXT)?;
        set_string(&verb, Some("Icon"), &shell_menu_command::icon(executable_path))?;
        let command_key = create_key(&join_key(&self.classes_key, &format!(r"{verb_path}\command")))?;
        set_string(&command_key, None, command)
    }
}

impl Default for WindowsShellIntegration {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellIntegration for WindowsShellIntegration {
    /// C# `GetState`: `shell_menu_command::evaluate(read_command(FOLDER_VERB), read_command(BACKGROUND_VERB), executable_path)` where `read_command` is the default value of
    /// `<classes>\<verb>\command` (absent key or non-string value = `None`). A blank `executable_path` is C# `ArgumentException`: here `NotRegistered` (the trait has no error
    /// channel; the Settings page only passes `current_exe`).
    fn get_state(&self, executable_path: &str) -> ShellMenuState {
        if executable_path.trim().is_empty() {
            return ShellMenuState::NotRegistered;
        }
        let folder = self.read_command(&folder_verb_path());
        let background = self.read_command(&background_verb_path());
        shell_menu_command::evaluate(folder.as_deref(), background.as_deref(), executable_path)
    }

    /// C# `Register`: `is_valid_executable_path(Some(path))` false -> `Err(io::ErrorKind::InvalidInput)` and NOTHING written; else `write_verb` for the folder verb then the background verb
    /// (`CreateSubKey`: default value = `MENU_TEXT`, value `Icon` = `icon(exe)`, subkey `command` default value = `for_folder(exe)` / `for_background(exe)`, all `REG_SZ`). A failure of
    /// either write calls `unregister` (best effort, errors swallowed) and returns the original error: never half of the pair.
    fn register(&self, executable_path: &str) -> io::Result<()> {
        if !shell_menu_command::is_valid_executable_path(Some(executable_path)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "The executable path must be rooted and must not contain a quote.",
            ));
        }
        let written = self
            .write_verb(
                &folder_verb_path(),
                executable_path,
                &shell_menu_command::for_folder(executable_path),
            )
            .and_then(|()| {
                self.write_verb(
                    &background_verb_path(),
                    executable_path,
                    &shell_menu_command::for_background(executable_path),
                )
            });
        if let Err(error) = written {
            // Never leave half of the pair behind: a retry then starts from a clean state.
            let _ = self.unregister();
            return Err(error);
        }
        Ok(())
    }

    /// C# `Unregister`: opens the classes key for writing; absent = `Ok(())`; `DeleteSubKeyTree` of both verb paths, a missing one is not an error. Nothing else is touched.
    fn unregister(&self) -> io::Result<()> {
        let Some(root) = open_key(&self.classes_key, KEY_ALL_ACCESS)? else {
            return Ok(());
        };
        delete_tree(&root, &folder_verb_path())?;
        delete_tree(&root, &background_verb_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // new: the text codec of the registry values (the native tests in tests/shell_integration.rs cover the real registry).
    #[test]
    fn reg_sz_bytes_round_trip_through_decode_and_end_with_a_nul() {
        let bytes = reg_sz_bytes("C:\\Apps\\\u{00e9}.exe");
        assert_eq!(&bytes[bytes.len() - 2..], &[0, 0]);
        assert_eq!(decode_reg_sz(&bytes), "C:\\Apps\\\u{00e9}.exe");
    }

    // new: a value stored without a terminator, or with garbage after it, still decodes.
    #[test]
    fn decode_reg_sz_cuts_at_the_first_nul_and_accepts_a_missing_terminator() {
        assert_eq!(decode_reg_sz(&[b'a', 0, b'b', 0]), "ab");
        assert_eq!(decode_reg_sz(&[b'a', 0, 0, 0, b'z', 0]), "a");
        assert_eq!(decode_reg_sz(&[]), "");
    }

    // new: a Win32 registry status becomes the io::Error the trait returns, keeping the OS code.
    #[test]
    fn status_to_result_keeps_the_os_error_code() {
        assert!(status_to_result(ERROR_SUCCESS).is_ok());
        let error = status_to_result(WIN32_ERROR(5)).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(5));
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    // C#: WindowsShellIntegration constructor (blank key is an ArgumentException).
    #[test]
    fn blank_classes_key_is_rejected_and_trailing_backslashes_are_trimmed() {
        for blank in ["", "   ", "\t"] {
            assert_eq!(WindowsShellIntegration::with_classes_key(blank), Err(BlankClassesKey));
        }
        let shell = WindowsShellIntegration::with_classes_key(r"Software\X\Classes\\").unwrap();
        assert_eq!(shell.classes_key, r"Software\X\Classes");
        assert_eq!(WindowsShellIntegration::new().classes_key, DEFAULT_CLASSES_KEY);
    }

    // new: the two verb paths below the classes root (R0002: the Rust verb, never the C# `PhotoReview`).
    #[test]
    fn verb_paths_use_the_rust_verb_key() {
        assert_eq!(folder_verb_path(), r"Directory\shell\PhotoReviewRs");
        assert_eq!(background_verb_path(), r"Directory\Background\shell\PhotoReviewRs");
        assert_eq!(join_key("A\\B", r"C\D"), r"A\B\C\D");
    }

    // C#: Register_rejects_an_unusable_path_and_writes_nothing (the validation happens before any registry access, so this runs without the registry).
    #[test]
    fn register_rejects_an_unusable_path_before_touching_the_registry() {
        let shell =
            WindowsShellIntegration::with_classes_key(r"Software\PhotoReviewRsTests\never-created\Classes").unwrap();
        for bad in ["", "relative.exe", "C:\\bad\"quote.exe"] {
            let error = shell.register(bad).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{bad}");
        }
        assert_eq!(shell.get_state(""), ShellMenuState::NotRegistered);
    }
}
