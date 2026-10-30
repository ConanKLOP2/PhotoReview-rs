//! Reads the kernel's file-object mode bits of an open handle (`NtQueryInformationFile(FileModeInformation)`).
//! Test-support for `cl-fsflush`: `FILE_FLAG_WRITE_THROUGH` shows up as `FILE_WRITE_THROUGH` and
//! `FILE_FLAG_SEQUENTIAL_SCAN` as `FILE_SEQUENTIAL_ONLY`, which `std` cannot report. `unsafe` stays in this FFI crate (AGENTS.md).

use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::os::windows::io::AsRawHandle;

/// `FILE_WRITE_THROUGH` of `FILE_MODE_INFORMATION.Mode` (set by `FILE_FLAG_WRITE_THROUGH`).
pub const FILE_WRITE_THROUGH: u32 = 0x2;
/// `FILE_SEQUENTIAL_ONLY` of `FILE_MODE_INFORMATION.Mode` (set by `FILE_FLAG_SEQUENTIAL_SCAN`).
pub const FILE_SEQUENTIAL_ONLY: u32 = 0x4;
const FILE_MODE_INFORMATION_CLASS: i32 = 16;

#[repr(C)]
struct IoStatusBlock {
    status: usize,
    information: usize,
}

// SAFETY: the declaration matches ntdll's documented `NtQueryInformationFile` signature; `raw-dylib` needs no import library.
#[link(name = "ntdll", kind = "raw-dylib")]
unsafe extern "system" {
    /// Not in the `windows` features this crate enables.
    fn NtQueryInformationFile(
        handle: *mut c_void,
        io_status: *mut IoStatusBlock,
        information: *mut c_void,
        length: u32,
        class: i32,
    ) -> i32;
}

/// The `FILE_MODE_INFORMATION.Mode` bits of `file`'s handle; the NTSTATUS as an error when the query fails.
#[doc(hidden)]
pub fn file_mode_bits(file: &File) -> io::Result<u32> {
    let mut io_status = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut mode = 0u32;
    // SAFETY: `file` owns a live handle for the whole call; `io_status` and `mode` are local out slots of exactly the
    // sizes the class FileModeInformation (an IO_STATUS_BLOCK and a single ULONG, length 4) needs.
    let nt = unsafe {
        NtQueryInformationFile(
            file.as_raw_handle(),
            &mut io_status,
            (&mut mode as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
            FILE_MODE_INFORMATION_CLASS,
        )
    };
    if nt >= 0 {
        Ok(mode)
    } else {
        Err(io::Error::other(format!(
            "NtQueryInformationFile(FileModeInformation) failed: {nt:#x}"
        )))
    }
}
