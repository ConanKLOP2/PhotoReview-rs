#[path = "../src/instance_keys.rs"]
mod instance_keys;
// Crate module wiring is lead-owned; include only the owned source files to keep this lane independent.
#[allow(dead_code)]
#[path = "../src/forward_pipe.rs"]
mod forward_pipe;

use forward_pipe::name_for_mutex;

#[test]
fn pipe_name_recipe_matches_csharp_hash_and_field_order() {
    // C# twin: InstanceForwardPipeTests.PipeName_DerivedFromFolderKey; fixed vector pins the delimiter and field order.
    assert_eq!(
        name_for_mutex(r"Local\PhotoReview_123", "PhotoReviewRs", "S-1-5-21-1-2-3-1001", 42,),
        "PhotoReviewRs.Forward.ECA228995870F1948B809371BA6406E10D062993A51D1D835DFC8838C1374D69"
    );
}

#[test]
fn pipe_name_uses_mutex_key_sid_and_session() {
    // C# twin: InstanceForwardPipeTests.PipeName_DerivedFromFolderKey.
    let baseline = name_for_mutex("mutex-a", "PhotoReviewRs", "S-1-5-21-100", 3);
    assert_ne!(baseline, name_for_mutex("mutex-b", "PhotoReviewRs", "S-1-5-21-100", 3));
    assert_ne!(baseline, name_for_mutex("mutex-a", "PhotoReviewRs", "S-1-5-21-101", 3));
    assert_ne!(baseline, name_for_mutex("mutex-a", "PhotoReviewRs", "S-1-5-21-100", 4));
    assert_ne!(baseline, name_for_mutex("mutex-a", "OtherPrefix", "S-1-5-21-100", 3));
}

#[test]
fn pipe_name_for_folder_key_is_case_and_separator_insensitive() {
    // C# twin: InstanceForwardPipeTests.PipeName_DerivedFromFolderKey.
    use crate::instance_keys::{current_session_id, current_user_sid, mutex_name_for};
    use pr_core::model::InstanceMode;

    let name = |folder| {
        let mutex = mutex_name_for(InstanceMode::PerFolder, folder, "PhotoReviewRs").expect("the test prefix is valid");
        name_for_mutex(&mutex, "PhotoReviewRs", current_user_sid(), current_session_id())
    };
    assert_eq!(name(Some(r"C:\Photos")), name(Some(r"c:\photos\")));
    assert_ne!(name(Some(r"C:\Photos")), name(Some(r"C:\Photos2")));
    assert_ne!(name(None), name(Some(r"C:\Photos")));
}

#[cfg(windows)]
mod native {
    use crate::forward_pipe::PipeSecurityDescriptor;
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
    use windows::Win32::Security::PSID;
    use windows::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL_SIZE_INFORMATION, AclSizeInformation, DACL_SECURITY_INFORMATION, GetAce,
        GetAclInformation, GetSecurityDescriptorControl, GetSecurityDescriptorDacl, SE_DACL_PROTECTED,
        SECURITY_ATTRIBUTES,
    };
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows::Win32::System::Pipes::{CreateNamedPipeW, PIPE_TYPE_BYTE};
    use windows::core::{HSTRING, PWSTR};

    use crate::instance_keys::current_user_sid;

    struct NativePipe(HANDLE);

    impl Drop for NativePipe {
        fn drop(&mut self) {
            // SAFETY: this test wrapper owns the pipe handle returned by CreateNamedPipeW and closes it once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    struct LocalAllocation(*mut c_void);

    impl Drop for LocalAllocation {
        fn drop(&mut self) {
            // SAFETY: the Windows security APIs used in this test allocate returned descriptors/strings via LocalAlloc.
            let _ = unsafe { LocalFree(Some(HLOCAL(self.0))) };
        }
    }

    fn unique_pipe_name() -> HSTRING {
        let suffix = format!("PhotoReview.Test.{}", std::process::id());
        HSTRING::from(format!(r"\\.\pipe\{suffix}"))
    }

    fn create_test_pipe(security: &PipeSecurityDescriptor) -> NativePipe {
        use windows::Win32::Foundation::INVALID_HANDLE_VALUE;

        let name = unique_pipe_name();
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: security.as_ptr().0,
            bInheritHandle: false.into(),
        };
        // SAFETY: name and SECURITY_ATTRIBUTES remain live for the synchronous call; the security descriptor is owned
        // by the caller. This creates one uniquely named test pipe and performs no I/O or user-file operation.
        let handle =
            unsafe { CreateNamedPipeW(&name, PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE, 1, 0, 0, 0, Some(&attributes)) };
        assert_ne!(handle, INVALID_HANDLE_VALUE, "CreateNamedPipeW failed");
        NativePipe(handle)
    }

    unsafe fn security_descriptor(handle: HANDLE) -> LocalAllocation {
        let mut descriptor = windows::Win32::Security::PSECURITY_DESCRIPTOR::default();
        // SAFETY: handle is the owned live test pipe; descriptor is a valid output slot.
        let status = unsafe {
            GetSecurityInfo(
                handle,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                Some(&mut descriptor as *mut _),
            )
        };
        assert_eq!(status.0, 0, "GetSecurityInfo failed");
        assert!(!descriptor.0.is_null());
        LocalAllocation(descriptor.0)
    }

    const GENERIC_ALL_MASK: u32 = 0x1000_0000;
    const FILE_ALL_ACCESS_MASK: u32 = 0x001F_01FF;

    fn assert_single_current_user_allow(descriptor: *mut c_void, expected_mask: u32) {
        let descriptor = windows::Win32::Security::PSECURITY_DESCRIPTOR(descriptor);
        let mut control = 0u16;
        let mut revision = 0u32;
        // SAFETY: descriptor is a live descriptor returned by GetSecurityInfo or created by the tested constructor.
        unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) }
            .expect("read security descriptor control");
        assert_ne!(control & SE_DACL_PROTECTED.0, 0, "DACL inherits entries");
        let mut present = false.into();
        let mut dacl = ptr::null_mut();
        let mut defaulted = false.into();
        // SAFETY: the descriptor is valid and all outputs are writable.
        unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) }.expect("read DACL");
        assert!(present.as_bool(), "DACL is absent");
        assert!(!dacl.is_null(), "DACL is null");

        let mut info = ACL_SIZE_INFORMATION::default();
        // SAFETY: dacl points into the live security descriptor and info is correctly sized writable storage.
        unsafe {
            GetAclInformation(
                dacl,
                (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        }
        .expect("read ACL information");
        assert_eq!(info.AceCount, 1, "DACL must contain exactly one ACE");

        let mut raw_ace = ptr::null_mut();
        // SAFETY: index zero is within the verified single-ACE ACL.
        unsafe { GetAce(dacl, 0, &mut raw_ace) }.expect("read sole ACE");
        let ace = raw_ace.cast::<ACCESS_ALLOWED_ACE>();
        // SAFETY: the ACL count confirms this is the sole ACE, and GetAce returned its valid header pointer.
        let header = unsafe { &*raw_ace.cast::<ACE_HEADER>() };
        assert_eq!(header.AceType, 0, "sole ACE is not ACCESS_ALLOWED_ACE_TYPE");
        assert!(
            usize::from(header.AceSize) >= size_of::<ACCESS_ALLOWED_ACE>(),
            "allow ACE is truncated"
        );
        // SAFETY: type and size above establish that raw_ace is a complete ACCESS_ALLOWED_ACE.
        let (flags, mask, sid_pointer) = unsafe {
            (
                (*ace).Header.AceFlags,
                (*ace).Mask,
                ptr::addr_of!((*ace).SidStart).cast_mut().cast(),
            )
        };
        assert_eq!(flags, 0, "ACE unexpectedly propagates to children");
        assert_eq!(mask, expected_mask, "ACE does not grant exactly full control");

        let mut sid_string = PWSTR::null();
        // SAFETY: sid_pointer is the SID immediately following the verified ACCESS_ALLOWED_ACE mask.
        unsafe { ConvertSidToStringSidW(PSID(sid_pointer), &mut sid_string) }.expect("format ACE SID");
        assert!(!sid_string.0.is_null());
        let _sid_string = LocalAllocation(sid_string.0.cast());
        let mut length = 0usize;
        // SAFETY: ConvertSidToStringSidW returns a live NUL-terminated string.
        unsafe {
            while *sid_string.0.add(length) != 0 {
                length += 1;
            }
        }
        // SAFETY: the pointer references `length` initialized UTF-16 units.
        let found = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(sid_string.0, length) });
        assert_eq!(found, current_user_sid());
    }

    #[test]
    #[ignore = "native: inspects a uniquely named test pipe DACL; immediate Win32 calls only"]
    fn pipe_security_allows_only_current_user() {
        // C# twin: InstanceForwardPipeTests.PipeSecurity_AllowsOnlyCurrentUser.
        let security = PipeSecurityDescriptor::for_sid(current_user_sid()).expect("construct current-user DACL");
        assert_single_current_user_allow(security.as_ptr().0, GENERIC_ALL_MASK);
    }

    #[test]
    #[ignore = "native: queries a uniquely named test pipe DACL; immediate Win32 calls only"]
    fn live_pipe_has_single_user_dacl() {
        // C# twin: InstanceForwardPipeTests.LivePipe_HasSingleUserDacl.
        let security = PipeSecurityDescriptor::for_sid(current_user_sid()).expect("construct current-user DACL");
        let pipe = create_test_pipe(&security);
        // SAFETY: the returned descriptor is allocated by GetSecurityInfo for this owned, live pipe handle.
        let descriptor = unsafe { security_descriptor(pipe.0) };
        // The kernel maps GENERIC_ALL to the pipe's specific rights (FILE_ALL_ACCESS) when it creates the object.
        assert_single_current_user_allow(descriptor.0, FILE_ALL_ACCESS_MASK);
    }
}
