//! Unit W8-11e test: `startup::set_process_dpi_awareness_context` in a test executable of its own.
//! The call changes the DPI awareness of the whole process and cannot be undone (trap T6), so no other test may share this process: `cargo test` runs
//! every integration test file as a separate executable. `// new:` (no C# test targets `StartupAdditions` directly; the C# caller is `DpiAwarenessBootstrap`).

use pr_platform_win::win32::api::user32;
use pr_platform_win::win32::startup::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, are_dpi_awareness_contexts_equal, set_process_dpi_awareness_context,
};
use pr_platform_win::win32::structs::Win32Error;

#[test]
#[ignore = "native: needs desktop"]
fn set_process_dpi_awareness_context_per_monitor_v2_then_the_thread_context_is_equal() {
    // `Ok` when the test executable has no DPI manifest (the first call wins), `Err(ERROR_ACCESS_DENIED)` when the awareness was already set.
    let first = set_process_dpi_awareness_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    assert!(matches!(first, Ok(()) | Err(Win32Error(5))), "unexpected {first:?}");
    // The shell bootstrap re-checks like this after the call (W8-17).
    assert!(are_dpi_awareness_contexts_equal(
        user32::get_thread_dpi_awareness_context(),
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
    ));
    // The awareness can be set only once per process.
    assert_eq!(
        set_process_dpi_awareness_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2),
        Err(Win32Error(5))
    );
}
