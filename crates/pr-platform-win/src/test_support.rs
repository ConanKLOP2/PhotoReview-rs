//! Test-only process metrics for native decoder tests.

use std::io;

use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::GetCurrentProcess;

/// Returns this process's private committed bytes (the Win32 `PrivateUsage` counter).
pub fn process_private_bytes() -> io::Result<u64> {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };

    // SAFETY: GetCurrentProcess returns a pseudo-handle valid for the current process, and
    // counters is initialized with the exact extended structure size expected by the API.
    unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            counters.cb,
        )
    }
    .map_err(io::Error::other)?;

    Ok(counters.PrivateUsage as u64)
}
