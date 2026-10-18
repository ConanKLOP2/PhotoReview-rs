//! Port of `PhotoReview.Platform.Windows/InstanceKeys.cs` (`MutexNameFor`, `CanonicalKey`, and process identity reads).

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use pr_core::model::InstanceMode;

/// Rust object-name prefix, isolated from the C# application per ADR R0002.
#[allow(dead_code)] // Consumed by instance_scope; integration-test targets that include this source do not use it.
pub const DEFAULT_PREFIX: &str = "PhotoReviewRs";

/// C# canonical key used when no folder is available (kept for oracle parity).
const NO_FOLDER_CANONICAL_KEY: &str = "PhotoReview";

static CURRENT_USER_SID: OnceLock<String> = OnceLock::new();
static CURRENT_SESSION_ID: OnceLock<u32> = OnceLock::new();

/// Returns the process SID, read once and cached for the lifetime of the process.
///
/// This value is also consumed by W6-03b when deriving the paired pipe name.
pub fn current_user_sid() -> &'static str {
    CURRENT_USER_SID.get_or_init(read_user_sid).as_str()
}

/// Returns the process session id, read once and cached for the lifetime of the process.
///
/// This value is also consumed by W6-03b when deriving the paired pipe name.
pub fn current_session_id() -> u32 {
    *CURRENT_SESSION_ID.get_or_init(read_session_id)
}

/// Produces the named mutex key for a mode, folder, and caller-provided prefix.
///
/// A blank prefix is rejected, matching `ArgumentException.ThrowIfNullOrWhiteSpace`.
pub fn mutex_name_for(mode: InstanceMode, folder: Option<&str>, prefix: &str) -> Result<String, &'static str> {
    // C# static property initializers read both values together on first type use; Rust caches each read separately.
    let _session_id = current_session_id();
    if prefix.trim().is_empty() {
        return Err("prefix cannot be empty or whitespace");
    }

    let key = if mode == InstanceMode::SingleWindow {
        format!("app|{}", current_user_sid())
    } else {
        canonical_key(folder)
    };
    let digest = pr_core::session::store::sha256(key.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02X}");
    }
    let suffix = if mode == InstanceMode::SingleWindow {
        "_App_"
    } else {
        "_"
    };
    Ok(format!("Local\\{prefix}{suffix}{hex}"))
}

/// R2-F-08 key: resolve to an absolute path, trim its ending separator, and uppercase invariantly.
///
/// Null, empty, and whitespace-only paths share the fixed C# canonical `PhotoReview` key. Invalid Windows path text falls back
/// to its original uppercased spelling, so lock lookup itself does not reject the launch.
pub fn canonical_key(path: Option<&str>) -> String {
    let Some(path) = path.filter(|path| !path.trim().is_empty()) else {
        return NO_FOLDER_CANONICAL_KEY.to_owned();
    };
    if path.contains('\0') {
        return path.to_uppercase();
    }

    match absolute_lexical_path(Path::new(path)) {
        Some(full) => trim_ending_separator(full).to_uppercase(),
        None => path.to_uppercase(),
    }
}

fn absolute_lexical_path(path: &Path) -> Option<String> {
    let absolute = std::path::absolute(path).ok()?;
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some() {
                    let _ = normalized.pop();
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    Some(normalized.to_string_lossy().into_owned())
}

fn trim_ending_separator(mut path: String) -> String {
    let root = Path::new(&path).parent().is_none();
    if !root {
        while path.ends_with(['\\', '/']) {
            path.pop();
        }
    }
    path
}

#[cfg(windows)]
fn read_user_sid() -> String {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::PWSTR;

    let mut token = HANDLE::default();
    // SAFETY: GetCurrentProcess returns the current-process pseudo handle; token is a valid out parameter.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
        return "nosid".to_owned();
    }
    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            // SAFETY: this wrapper owns the process token handle and closes it exactly once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
    let _token = Token(token);

    let mut required = 0u32;
    // SAFETY: null buffer and zero length is the documented size-query form.
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut required) };
    if required < std::mem::size_of::<TOKEN_USER>() as u32 {
        return "nosid".to_owned();
    }
    let mut buffer = vec![0usize; (required as usize).div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: aligned storage has at least `required` writable bytes for TOKEN_USER.
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            required,
            &mut required,
        )
    }
    .is_err()
    {
        return "nosid".to_owned();
    }
    // SAFETY: successful TokenUser query initialized the buffer with TOKEN_USER and its SID pointer.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut wide_sid = PWSTR::null();
    // SAFETY: sid remains owned by the live token-information buffer through conversion.
    if unsafe { ConvertSidToStringSidW(sid, &mut wide_sid) }.is_err() || wide_sid.0.is_null() {
        return "nosid".to_owned();
    }
    let mut length = 0usize;
    // SAFETY: ConvertSidToStringSidW returns a live NUL-terminated UTF-16 string.
    unsafe {
        while *wide_sid.0.add(length) != 0 {
            length += 1;
        }
    }
    // SAFETY: pointer references `length` initialized UTF-16 code units followed by NUL.
    let value = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(wide_sid.0, length) });
    // SAFETY: ConvertSidToStringSidW allocates with LocalAlloc; LocalFree releases it.
    let _ = unsafe { LocalFree(Some(HLOCAL(wide_sid.0.cast()))) };
    value
}

#[cfg(not(windows))]
fn read_user_sid() -> String {
    "nosid".to_owned()
}

#[cfg(windows)]
fn read_session_id() -> u32 {
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    let mut session = 0u32;
    // SAFETY: `session` is a valid writable output; current process id is valid for this process.
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) }.is_ok() {
        session
    } else {
        0
    }
}

#[cfg(not(windows))]
fn read_session_id() -> u32 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder_name(path: Option<&str>, prefix: &str) -> String {
        mutex_name_for(InstanceMode::PerFolder, path, prefix).expect("test prefix is valid")
    }

    #[test]
    fn mutex_name_for_spelling_variants_are_equal() {
        // C# twin: InstanceLockKeyTests.MutexNameFor_SpellingVariants_AreEqual.
        let expected = folder_name(Some(r"C:\Photos"), "PhotoReviewTest");
        assert_eq!(expected, folder_name(Some(r"C:\Photos\"), "PhotoReviewTest"));
        assert_eq!(expected, folder_name(Some(r"c:\photos"), "PhotoReviewTest"));
        assert_eq!(expected, folder_name(Some(r"C:\Photos\sub\.."), "PhotoReviewTest"));
        assert_ne!(expected, folder_name(Some(r"C:\Photos2"), "PhotoReviewTest"));
    }

    #[test]
    fn mutex_name_for_matches_csharp_sha256_recipe() {
        // C# twin: InstanceKeys.MutexNameFor; the fixed canonical key pins the UTF-8/hash input exactly.
        assert_eq!(
            folder_name(Some(r"C:\Photos"), "PhotoReviewTest"),
            "Local\\PhotoReviewTest_05A1B0917BE15C6BED8FB6896E84FCEF676A0B14EE2D3E39DC5896D82271BA4D"
        );
    }

    #[test]
    fn mutex_name_for_relative_path_resolves_against_current_directory() {
        // C# twin: InstanceLockKeyTests.MutexNameFor_RelativePath_ResolvesAgainstCurrentDirectory.
        let full = std::path::absolute("relative-folder").expect("absolute path");
        assert_eq!(
            folder_name(Some(&full.to_string_lossy()), "PhotoReviewTest"),
            folder_name(Some("relative-folder"), "PhotoReviewTest")
        );
    }

    #[test]
    fn canonical_key_parent_segments_do_not_escape_the_volume_root() {
        assert_eq!(canonical_key(Some(r"C:\..")), r"C:\");
    }

    #[test]
    fn mutex_name_for_no_folder_is_constant() {
        // C# twin: InstanceLockKeyTests.MutexNameFor_NoFolder_IsConstant.
        assert_eq!(
            folder_name(None, "PhotoReviewTest"),
            folder_name(Some(""), "PhotoReviewTest")
        );
        let current_photo_review = std::path::absolute("PhotoReview").expect("absolute path");
        assert_ne!(
            folder_name(None, "PhotoReviewTest"),
            folder_name(Some(&current_photo_review.to_string_lossy()), "PhotoReviewTest")
        );
    }

    #[test]
    fn mutex_name_for_unusable_path_does_not_throw() {
        // C# twin: InstanceLockKeyTests.MutexNameFor_UnusablePath_DoesNotThrow.
        let bad = "C:\\Photos\0bad";
        let name = folder_name(Some(bad), "PhotoReviewTest");
        assert_eq!(name, folder_name(Some(bad), "PhotoReviewTest"));
        assert_ne!(name, folder_name(Some(r"C:\Photos"), "PhotoReviewTest"));
    }

    #[test]
    fn mutex_name_for_rejects_blank_prefix() {
        assert_eq!(
            mutex_name_for(InstanceMode::PerFolder, None, " \t"),
            Err("prefix cannot be empty or whitespace")
        );
    }

    #[test]
    fn single_window_mutex_name_uses_only_current_sid() {
        let first = mutex_name_for(InstanceMode::SingleWindow, Some("one"), "PhotoReviewTest").expect("valid prefix");
        let second = mutex_name_for(InstanceMode::SingleWindow, Some("two"), "PhotoReviewTest").expect("valid prefix");
        let digest = pr_core::session::store::sha256(format!("app|{}", current_user_sid()).as_bytes());
        let expected = format!(
            "Local\\PhotoReviewTest_App_{}",
            digest.iter().map(|byte| format!("{byte:02X}")).collect::<String>()
        );
        assert_eq!(first, second);
        assert_eq!(first, expected);
    }
}
