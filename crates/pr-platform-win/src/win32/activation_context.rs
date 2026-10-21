//! Port of `PhotoReview.Shell.Interop/Win32/ActivationContext.cs` (baseline `d92d6aae`, 39 lines): the side-by-side (SxS) activation context calls
//! (`CreateActCtxW`, `ActivateActCtx`, `DeactivateActCtx`, `ReleaseActCtx`). The C# comment: native tests need comctl32 v6 in a process without its own
//! manifest (the .NET test host); the real `.exe` has the manifest, so the shell never uses this.
//!
//! Unit W8-11c. The C# `NativeDialogTestKit.ActivateComctl32V6()` loads the manifest from resource 1 of `PhotoReview.dll`. A Rust test executable has no
//! manifest resource, so the Rust twin writes `comctl::SHELL_MANIFEST` to a temp file and activates that (`ActivationScope::from_manifest_text`).
use core::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::ApplicationInstallationAndServicing::{
    ActivateActCtx, CreateActCtxW, DeactivateActCtx, ReleaseActCtx,
};
use windows::core::PCWSTR;

use super::structs::{ActCtx, Win32Error, new_act_ctx, to_utf16z};

/// The code of a `windows` error: the low 16 bits of an `HRESULT_FROM_WIN32` value (what `GetLastError` returned), else the raw code.
fn win32_error(error: &windows::core::Error) -> Win32Error {
    let code = error.code().0 as u32;
    if code >> 16 == 0x8007 {
        Win32Error(code & 0xFFFF)
    } else {
        Win32Error(code)
    }
}

pub const ACTCTX_FLAG_RESOURCE_NAME_VALID: u32 = 8; // FlagResourceNameValid

/// C# `CreateActCtx(ActCtx*)`: the context handle; `Err` when the OS returns `INVALID_HANDLE_VALUE` (-1).
///
/// # Safety
/// `context.lpSource`, `lpAssemblyDirectory`, `lpApplicationName` (when set) are NUL-terminated UTF-16 text valid for the call, and `lpResourceName` is a
/// pointer or `MAKEINTRESOURCE` id as `dwFlags` says.
pub unsafe fn create_act_ctx(context: &ActCtx) -> Result<isize, Win32Error> {
    // SAFETY: upheld by the caller of this function; `context` is a live ACTCTXW whose `cbSize` is its size.
    unsafe { CreateActCtxW(context) }
        .map(|handle| handle.0 as isize)
        .map_err(|e| win32_error(&e))
}

/// C# `ActivateActCtx(context, &cookie)`: the cookie to pass to `deactivate_act_ctx`.
pub fn activate_act_ctx(context: isize) -> Result<usize, Win32Error> {
    let mut cookie = 0usize;
    // SAFETY: a context handle value the OS validates; `cookie` is a live local.
    unsafe { ActivateActCtx(Some(HANDLE(context as *mut c_void)), &mut cookie) }
        .map(|()| cookie)
        .map_err(|e| win32_error(&e))
}

/// C# `DeactivateActCtx(flags, cookie)`: must be called on the same thread, in reverse order of activation (out of order is a fast-fail in the OS).
pub fn deactivate_act_ctx(flags: u32, cookie: usize) -> Result<(), Win32Error> {
    // SAFETY: integer arguments; the caller keeps the thread and the reverse order (a violation is a fast-fail in the OS, not memory unsafety here).
    unsafe { DeactivateActCtx(flags, cookie) }.map_err(|e| win32_error(&e))
}

/// C# `ReleaseActCtx(context)`.
pub fn release_act_ctx(context: isize) {
    // SAFETY: a context handle value the OS validates and releases one reference of.
    unsafe { ReleaseActCtx(HANDLE(context as *mut c_void)) }
}

/// RAII twin of the C# test kit's private `ActivationScope`: dropping it deactivates the context and releases it (in that order). Not `Send`: the cookie is
/// valid on the creating thread only.
#[derive(Debug)]
pub struct ActivationScope {
    context: isize,
    cookie: usize,
    _not_send: core::marker::PhantomData<*const ()>,
}

impl ActivationScope {
    /// Activates the context `created` (or turns the creation error into text); a failed activation releases the context again.
    fn activate(created: Result<isize, Win32Error>) -> Result<Self, String> {
        let context = created.map_err(|e| format!("CreateActCtx failed: {e}"))?;
        match activate_act_ctx(context) {
            Ok(cookie) => Ok(Self {
                context,
                cookie,
                _not_send: core::marker::PhantomData,
            }),
            Err(e) => {
                release_act_ctx(context);
                Err(format!("ActivateActCtx failed: {e}"))
            }
        }
    }

    /// Creates and activates a context from manifest XML text (written to a uniquely named file in the temp directory, which is deleted again before this
    /// returns: the context keeps its own copy). Error text on failure (file, `CreateActCtx` or `ActivateActCtx`; includes the Win32 code).
    pub fn from_manifest_text(manifest_xml: &str) -> Result<Self, String> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "PhotoReviewRs-comctl-{}-{}.manifest",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, manifest_xml.as_bytes())
            .map_err(|e| format!("cannot write the manifest file {}: {e}", path.display()))?;
        let source = to_utf16z(&path.to_string_lossy());
        let mut context = new_act_ctx();
        context.lpSource = PCWSTR(source.as_ptr());
        // SAFETY: `lpSource` points into `source`, a NUL-terminated UTF-16 buffer in a local that lives past the call; the other pointers are null.
        let created = unsafe { create_act_ctx(&context) };
        // The context has parsed the file (or failed to): the file is no longer needed either way.
        let _ = std::fs::remove_file(&path);
        Self::activate(created)
    }

    /// Creates and activates a context from resource `resource_id` of the module file `module_path` (`ACTCTX_FLAG_RESOURCE_NAME_VALID`, the C# kit's
    /// `ResourceName = 1` on `PhotoReview.dll`).
    pub fn from_module_resource(module_path: &str, resource_id: u16) -> Result<Self, String> {
        let source = to_utf16z(module_path);
        let mut context = new_act_ctx();
        context.dwFlags = ACTCTX_FLAG_RESOURCE_NAME_VALID;
        context.lpSource = PCWSTR(source.as_ptr());
        context.lpResourceName = PCWSTR(usize::from(resource_id) as *const u16);
        // SAFETY: `lpSource` points into `source`, a NUL-terminated UTF-16 buffer in a local that lives past the call; `lpResourceName` is
        // `MAKEINTRESOURCE(resource_id)` as `ACTCTX_FLAG_RESOURCE_NAME_VALID` says.
        let created = unsafe { create_act_ctx(&context) };
        Self::activate(created)
    }
}

impl Drop for ActivationScope {
    fn drop(&mut self) {
        // Errors cannot be reported from `drop`: deactivate first, then release.
        let _ = deactivate_act_ctx(0, self.cookie);
        release_act_ctx(self.context);
    }
}
