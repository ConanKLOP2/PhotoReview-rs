# FIX-P67-NATIVEDLG (2026-11-01)

Status: implemented (owner confirmed P-67 options (a)+(b)). Sources: [FIX-G11-NATIVEDLG](FIX-G11-NATIVEDLG-2026-11-01.md), [native-dialog.md](native-dialog.md) M3, L3, L4; C# `NativeDialog.cs` at `7eaee2a2`. Crate: `pr-platform-win`, dir `src/dialogs/native/dialog/`.

## Changes (one commit each)

| Item | Change | Tests (inline, pure) |
|---|---|---|
| (a) M3 | `cleanup` step 10 now calls `result::run_closed_callback`, which runs the `closed` callback under `pending_panic::guarded` (first panic captured, resumed by `rethrow_pending`). | `run_closed_callback_runs_the_callback_and_accepts_none`, `run_closed_callback_captures_a_panic_instead_of_unwinding` |
| (b) L4 | `dpi.rs`: pure `plan_font_swap`; `apply_fonts` keeps the previous body/header font (and does not delete it) when `create_fonts` returns 0. Marked `DIVERGENCE DV` in code. | `font_swap_tests` (6 cases) |
| (b) L3 | `focus.rs`: pure `is_focusable_tab_stop` (`WS_TABSTOP` and not `WS_DISABLED`); `first_tab_stop` uses it. `is_tab_stop` (C# rule) unchanged. Marked `DIVERGENCE DV`. | `focus::tests::disabled_tab_stops_are_skipped` |

## Test honesty

- (a): the real hazard (panic in `CleanupGuard::drop` during unwinding aborts the process) cannot be observed in-process, so no test fails "before" in that form. The test is indirect: it goes through the extracted helper and asserts the panic does not unwind out of it and is captured. Before the change the helper did not exist (the callback was called bare).
- (b): tests are pure/fake functions; no window needed, nothing `#[ignore]`d. The `apply_fonts`/`first_tab_stop` wiring itself needs a real dialog (existing native `#[ignore]` tests cover the dialog; not run here).
- Run: only `cargo test -p pr-platform-win --lib` filtered to the new tests : 9 passed, 0 failed, plus `cargo fmt -p pr-platform-win`. No workspace build/clippy; CI runs the rest.

## PARITY note (do not edit PARITY.md here)

Two intentional departures from C# `ApplyFonts` / `FirstTabStop`, owner-confirmed via P-67 option B: (1) a failed font creation keeps the old font (C# deletes it and ends fontless); (2) a disabled first tab stop is skipped (C# gives it the failed initial focus). The `closed` guard is Rust-only hardening with no visible C# difference. Suggested: the PARITY row for NativeDialog (W8-18i/18j/18k) gets a DV note for L3/L4; no status change. P-67 in PROPOSED-DECISIONS.md can move to CONFIRMED/implemented (other lane).
