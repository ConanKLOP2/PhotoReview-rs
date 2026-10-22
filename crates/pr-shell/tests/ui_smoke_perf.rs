//! Slice S6 of unit W8-20b (`contracts/W8-20b.md`): the MANUAL measurement that decides whether the Phase 0 spike (`ui-spike`, `r0001-spike`) can be deleted (slice S7). It runs the real shell
//! over a generated folder and PRINTS the numbers `docs/adr/R0001-ui-framework.md` lists for the spike: first frame, switch p50/p95 and idle CPU. It asserts only invariants (every frame
//! presented, no UI-thread block longer than the C# bound), never a time against a fixed limit: the lead compares the printed numbers with the spike's by hand on an attended machine.
//! C# twins: `PresentationPerfReportTests.Frames_AfterTheFirstImage_DoNotBlockTheUiThreadInPresent` (line 28) and `Hosting/ShellIdleCpuReportTests.EmptyWindow_Idle_UsesLessThanHalfAPercentOfOneCore`.
//!
//! `#[ignore = "manual: perf report"]`. STUB: the bodies are `todo!()`.
#![cfg(windows)]
#![allow(dead_code, unused_imports)] // stub

#[path = "common/ui_smoke_fixture.rs"]
mod ui_smoke_fixture;

use ui_smoke_fixture::{OpenTarget, SmokeConfig, UiSmokeRun, vk};

/// 60 photos of the 24 MP size class (the generator writes them, see the contract), `PHOTOREVIEW_RS_WARP` NOT forced (the report is meant for the real GPU: `prefer_warp = false`), 100 switches
/// 250 ms apart by an event timer (a `Condvar` timeout, not a sleep poll), then 3 s idle: prints `first_frame_ms`, `switch_p50_ms`, `switch_p95_ms`, `ui_block_max_ms`, `idle_cpu_seconds`.
#[test]
#[ignore = "manual: perf report"]
fn first_frame_switch_p50_p95_and_idle_cpu_report() {
    todo!("W8-20b S6: contracts/W8-20b.md")
}
