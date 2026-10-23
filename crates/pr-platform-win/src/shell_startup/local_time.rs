//! New (no C# twin): the local wall-clock time that `pr-core` cannot read (decision D16, P-05). C# used `DateTime.Now` in `PerfCsvListener.TryStartFromEnvironment`; the Rust
//! listener takes the time as an argument (`pr_core::diagnostics::perf_csv::LocalTimeParts`). Unit W8-17d. Contract: `contracts/W8-17d.md`.
//!
//! Used for ONE thing in this unit: the `perf-<pid>-<yyyyMMdd-HHmmss>.csv` file name. (The text of the general log still uses `pr_core::diagnostics::file_log::SystemLocalTime`, which reads
//! UTC; making it local is outside this unit, decision D-W8B6-38.)

use pr_core::diagnostics::perf_csv::LocalTimeParts;

/// `GetLocalTime` as `LocalTimeParts` (year, month, day, hour, minute, second; the milliseconds are not part of the type). Never fails. The one `unsafe` call carries a `// SAFETY:` comment
/// (or the `windows` crate offers it as a safe function: use it then).
pub fn local_time_parts() -> LocalTimeParts {
    // SAFETY: GetLocalTime returns an initialized SYSTEMTIME by value; no pointer is retained.
    let time = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    LocalTimeParts {
        year: i32::from(time.wYear),
        month: u32::from(time.wMonth),
        day: u32::from(time.wDay),
        hour: u32::from(time.wHour),
        minute: u32::from(time.wMinute),
        second: u32::from(time.wSecond),
    }
}
