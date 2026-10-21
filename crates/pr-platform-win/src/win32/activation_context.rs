//! Port of `PhotoReview.Shell.Interop/Win32/ActivationContext.cs` (baseline `d92d6aae`, 39 lines): the side-by-side (SxS) activation context calls
//! (`CreateActCtxW`, `ActivateActCtx`, `DeactivateActCtx`, `ReleaseActCtx`). The C# comment: native tests need comctl32 v6 in a process without its own
//! manifest (the .NET test host); the real `.exe` has the manifest, so the shell never uses this.
//!
//! Unit W8-11c. The C# `NativeDialogTestKit.ActivateComctl32V6()` loads the manifest from resource 1 of `PhotoReview.dll`. A Rust test executable has no
//! manifest resource, so the Rust twin writes `comctl::SHELL_MANIFEST` to a temp file and activates that (`ActivationScope::from_manifest_text`).
#![allow(unused_variables, dead_code)] // stub: removed by the W8-11c porter when the last `todo!()` is gone.

use super::structs::{ActCtx, Win32Error};

pub const ACTCTX_FLAG_RESOURCE_NAME_VALID: u32 = 8; // FlagResourceNameValid

/// C# `CreateActCtx(ActCtx*)`: the context handle; `Err` when the OS returns `INVALID_HANDLE_VALUE` (-1).
///
/// # Safety
/// `context.lpSource`, `lpAssemblyDirectory`, `lpApplicationName` (when set) are NUL-terminated UTF-16 text valid for the call, and `lpResourceName` is a
/// pointer or `MAKEINTRESOURCE` id as `dwFlags` says.
pub unsafe fn create_act_ctx(context: &ActCtx) -> Result<isize, Win32Error> {
    todo!()
}

/// C# `ActivateActCtx(context, &cookie)`: the cookie to pass to `deactivate_act_ctx`.
pub fn activate_act_ctx(context: isize) -> Result<usize, Win32Error> {
    todo!()
}

/// C# `DeactivateActCtx(flags, cookie)`: must be called on the same thread, in reverse order of activation (out of order is a fast-fail in the OS).
pub fn deactivate_act_ctx(flags: u32, cookie: usize) -> Result<(), Win32Error> {
    todo!()
}

/// C# `ReleaseActCtx(context)`.
pub fn release_act_ctx(context: isize) {
    todo!()
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
    /// Creates and activates a context from manifest XML text (written to a uniquely named file in the temp directory, which is deleted again before this
    /// returns: the context keeps its own copy). Error text on failure (file, `CreateActCtx` or `ActivateActCtx`; includes the Win32 code).
    pub fn from_manifest_text(manifest_xml: &str) -> Result<Self, String> {
        todo!()
    }

    /// Creates and activates a context from resource `resource_id` of the module file `module_path` (`ACTCTX_FLAG_RESOURCE_NAME_VALID`, the C# kit's
    /// `ResourceName = 1` on `PhotoReview.dll`).
    pub fn from_module_resource(module_path: &str, resource_id: u16) -> Result<Self, String> {
        todo!()
    }
}

impl Drop for ActivationScope {
    fn drop(&mut self) {
        todo!()
    }
}
