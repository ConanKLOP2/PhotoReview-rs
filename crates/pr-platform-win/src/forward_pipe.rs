//! Port of `PhotoReview.Platform.Windows/InstanceForwardServer.cs`'s `InstanceForwardPipe` class.
//!
//! This module owns the stable identity and current-user-only security descriptor paired with the instance mutex.

/// Derives the Q-R18 pipe name paired with a mutex, current user SID, and logon session.
///
/// C# twin: `InstanceForwardPipe.NameForMutex`.
pub fn name_for_mutex(mutex_name: &str, prefix: &str, sid: &str, session_id: u32) -> String {
    let key = format!("{mutex_name}|{sid}|{session_id}");
    let digest = pr_core::session::store::sha256(key.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02X}");
    }
    format!("{prefix}.Forward.{hex}")
}

/// Derives the pipe name using the process identity cached by W6-03a.
pub fn current_pipe_name(mutex_name: &str, prefix: &str) -> String {
    name_for_mutex(
        mutex_name,
        prefix,
        crate::instance_keys::current_user_sid(),
        crate::instance_keys::current_session_id(),
    )
}

/// An owned Win32 security descriptor. Its DACL is protected and contains exactly one allow ACE for `sid`.
#[cfg(windows)]
#[derive(Debug)]
pub struct PipeSecurityDescriptor {
    descriptor: windows::Win32::Security::PSECURITY_DESCRIPTOR,
}

#[cfg(windows)]
impl PipeSecurityDescriptor {
    /// Creates the C# `PipeSecurity` equivalent from an explicit process SID.
    ///
    /// SDDL `D:P(A;;GA;;;SID)` means a protected DACL with one generic-all allow ACE and no inherited ACEs.
    pub fn for_sid(sid: &str) -> windows_core::Result<Self> {
        use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
        use windows::Win32::Security::PSECURITY_DESCRIPTOR;
        use windows::core::{HRESULT, HSTRING};

        let Some(sid_suffix) = sid.strip_prefix("S-") else {
            return Err(windows_core::Error::new(
                HRESULT(0x8007_0057u32 as i32),
                "SID is not in canonical string form",
            ));
        };
        if sid_suffix.is_empty() || !sid_suffix.bytes().all(|byte| byte.is_ascii_digit() || byte == b'-') {
            return Err(windows_core::Error::new(
                HRESULT(0x8007_0057u32 as i32),
                "SID contains characters outside canonical string form",
            ));
        }
        let sddl = HSTRING::from(format!("D:P(A;;GA;;;{sid})"));
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `sddl` is a live NUL-terminated UTF-16 string and `descriptor` is a valid output pointer.
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, 1, &mut descriptor, None)? };
        if descriptor.0.is_null() {
            return Err(windows_core::Error::new(
                HRESULT(0x8007_0005u32 as i32),
                "security descriptor was null",
            ));
        }
        Ok(Self { descriptor })
    }

    /// Returns the descriptor pointer for `SECURITY_ATTRIBUTES` during a synchronous object creation call.
    pub fn as_ptr(&self) -> windows::Win32::Security::PSECURITY_DESCRIPTOR {
        self.descriptor
    }
}

#[cfg(windows)]
impl Drop for PipeSecurityDescriptor {
    fn drop(&mut self) {
        use windows::Win32::Foundation::{HLOCAL, LocalFree};
        // SAFETY: the conversion API allocated this descriptor with LocalAlloc and this owner frees it once.
        let _ = unsafe { LocalFree(Some(HLOCAL(self.descriptor.0))) };
    }
}

/// Builds the production descriptor using W6-03a's cached current-user SID.
#[cfg(windows)]
pub fn current_security_descriptor() -> windows_core::Result<PipeSecurityDescriptor> {
    let sid = crate::instance_keys::current_user_sid();
    if sid == "nosid" {
        return Err(windows_core::Error::new(
            windows_core::HRESULT(0x8007_0005u32 as i32),
            "current user SID is unavailable",
        ));
    }
    PipeSecurityDescriptor::for_sid(sid)
}
