//! Port of `PhotoReview.Platform.Windows/WindowsNaturalComparer.cs`.
//! Uses the shell's native natural sort and retains the managed fallback when the DLL or export is unavailable.

use std::cmp::Ordering;
use std::sync::OnceLock;

use pr_core::abstractions::NaturalComparer;
use pr_core::catalog::natural_cmp::ManagedNaturalComparer;
use windows::Win32::Foundation::FreeLibrary;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::{PCWSTR, w};

type StrCmpLogicalWFn = unsafe extern "system" fn(PCWSTR, PCWSTR) -> i32;

static STR_CMP_LOGICAL_W: OnceLock<Option<StrCmpLogicalWFn>> = OnceLock::new();

fn native_compare(left: &str, right: &str) -> Option<Ordering> {
    let function = STR_CMP_LOGICAL_W.get_or_init(|| {
        // SAFETY: the DLL name is a static NUL-terminated UTF-16 string.
        let module = match unsafe { LoadLibraryW(w!("shlwapi.dll")) } {
            Ok(module) => module,
            Err(_) => return None,
        };
        // SAFETY: the module handle is valid and the export name is a static NUL-terminated ANSI string.
        let Some(address) = (unsafe { GetProcAddress(module, windows::core::s!("StrCmpLogicalW")) }) else {
            // A valid loaded module can return null here only when the named export is absent, which is the C#
            // EntryPointNotFoundException fallback case.
            // SAFETY: the loaded module is not retained because the required export is absent.
            let _ = unsafe { FreeLibrary(module) };
            return None;
        };
        // SAFETY: the loaded export is StrCmpLogicalW(PCWSTR, PCWSTR) -> i32; the ABI matches its documented signature.
        let function =
            unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, StrCmpLogicalWFn>(address) };
        // Keep the loaded module for process lifetime so the cached function pointer remains valid.
        Some(function)
    });
    let function = (*function)?;
    let left: Vec<u16> = left.encode_utf16().chain(std::iter::once(0)).collect();
    let right: Vec<u16> = right.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: both buffers are NUL-terminated UTF-16 and remain alive for the call.
    Some(
        match unsafe { function(PCWSTR(left.as_ptr()), PCWSTR(right.as_ptr())) }.cmp(&0) {
            Ordering::Less => Ordering::Less,
            Ordering::Equal => Ordering::Equal,
            Ordering::Greater => Ordering::Greater,
        },
    )
}

/// Windows shell natural comparer. If shlwapi or StrCmpLogicalW is unavailable, uses the managed comparer.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsNaturalComparer;

impl WindowsNaturalComparer {
    /// Stateless singleton equivalent.
    pub const INSTANCE: Self = Self;
}

impl NaturalComparer for WindowsNaturalComparer {
    fn compare(&self, left: Option<&str>, right: Option<&str>) -> Ordering {
        match (left, right) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(left), Some(right)) => match native_compare(left, right) {
                Some(Ordering::Equal) => {
                    // C# uses StringComparer.OrdinalIgnoreCase to break native logical ties.
                    pr_core::text::cmp_ignore_case(left, right)
                }
                Some(ordering) => ordering,
                None => ManagedNaturalComparer.compare(Some(left), Some(right)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_values_sort_before_non_null_values() {
        // C# twin: WindowsNaturalComparerTests.SameReference_IsZero and Null_SortsFirst.
        let comparer = WindowsNaturalComparer::INSTANCE;
        assert_eq!(comparer.compare(None, None), Ordering::Equal);
        assert_eq!(comparer.compare(None, Some("")), Ordering::Less);
        assert_eq!(comparer.compare(Some(""), None), Ordering::Greater);
    }

    #[test]
    #[ignore = "native: shlwapi StrCmpLogicalW behavior"]
    fn natural_order_and_case_tie_break_match_windows_shell() {
        // C# twins: WindowsNaturalComparerTests.NaturalOrder, LogicalTie_IsBrokenByOrdinalIgnoreCase,
        // CaseOnly_IsZero; LibraryImportInteropTests.NaturalComparer_UsesTheUtf16Overload.
        let comparer = WindowsNaturalComparer::INSTANCE;
        assert_eq!(comparer.compare(Some("a2"), Some("a10")), Ordering::Less);
        assert_eq!(comparer.compare(Some("IMG_1.JPG"), Some("img_1.jpg")), Ordering::Equal);
        assert_ne!(comparer.compare(Some("ß"), Some("ss")), Ordering::Equal);
        assert_eq!(comparer.compare(Some("中1.jpg"), Some("中2.jpg")), Ordering::Less);
    }
}
