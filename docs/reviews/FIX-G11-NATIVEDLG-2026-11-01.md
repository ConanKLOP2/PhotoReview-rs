# FIX-G11-NATIVEDLG (2026-11-01)

Status: investigation, no production change. Base: origin/master `558c6ea7`. Sources: [native-dialog.md](native-dialog.md) M3, L1..L5; G11 in [ST-BACKLOG-NOW-2026-11-01.md](ST-BACKLOG-NOW-2026-11-01.md); C# `NativeDialog.cs` at `7eaee2a2`. Nothing was built or run (read and grep only).

## Result: no code changed

| Item | State on master | C# 7eaee2a2 | Decision |
|---|---|---|---|
| M3 `closed` callback / `on_closed` while unwinding | OPEN. `result.rs` `cleanup` calls `callback()` at the end unguarded; `on_closed` is guarded by `pending_panic::guarded`. `CleanupGuard::drop` (`modal.rs:88-98`) runs `cleanup` during unwinding. | `Cleanup()` wraps `OnClosed()` in try/catch (Rust has the same); `_closedCallback?.Invoke()` is unguarded, but .NET has no abort on a second exception. | Rust-only hardening. Not fixed: the only production `closed` callbacks (`secondary_window_port.rs:380`, `recovery/mod.rs:403`) just remove the window from a map and cannot reasonably panic; a failing test cannot be written without a real window (a panic in a drop during unwind aborts the process, which no in-process test can observe). Proposed fix if wanted: run the callback under `pending_panic::guarded` (P-67, see below). |
| L1 tooltip buffer reallocation | OPEN, same shape (`tooltip.rs:230-242`, `resize`). | `EnsureTipCapacity` uses `NativeMemory.Realloc` (same behaviour, line ~932). | Parity with C#; comctl32 copy semantics unverified; needs a real tooltip (native, not testable purely). Not fixed. |
| L2 `message_loop` never cleared in `cleanup` | OPEN (retention, no cycle). | C# also keeps `_loop` set after `Cleanup()`. | Parity; harmless. Not fixed. |
| L3 `first_tab_stop` ignores disabled/hidden | OPEN. | `FirstTabStop` has the same rule (style `WS_TABSTOP` only). | Parity, reported as such by the review itself. Not fixed. |
| L4 `apply_fonts` deletes old fonts when new creation returned 0 | OPEN. | `ApplyFonts` has the same order (`DeleteIfSet(oldFont)` after `CreateFonts`). | Parity; needs GDI exhaustion to trigger. Not fixed. |
| L5 modeless filter passes a copy of the message | OPEN. | C# passes the original `Msg`. | Known divergence, no observable difference (`IsDialogMessage` does not write `MSG` in known cases). Not fixed. |

The BACKLOG G11 row ("no trace of a fix") is confirmed: no `thread::panicking` in `dialogs/native/dialog/`, no `WS_DISABLED` filter in `focus.rs`, no `message_loop` clearing in cleanup. The BACKLOG top-8 test proposals ("font moi loi khong xoa font cu", "first_tab_stop bo qua disabled") would deliberately differ from C# and would need a decision, not a bug fix; all current tests of this scope are native and `#[ignore]` (already in the nightly list).

## Proposed decision (not written to PROPOSED-DECISIONS.md by this lane)

P-67 (next free number; P-66 is the last used): harden `NativeDialog::cleanup` for Rust only: (a) run `closed_callback` under `pending_panic::guarded`; (b) optionally keep the old font when `create_fonts` returns 0 and skip disabled controls in `first_tab_stop`. Recommended: do only (a) if ever; (b) deliberately departs from C# parity. Needs owner confirmation before code changes.

## PARITY rows

No change proposed.
