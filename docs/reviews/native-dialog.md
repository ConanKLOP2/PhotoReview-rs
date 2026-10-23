# Review: native dialog base (W8-18h..18k, PR #184)

Machine C (cloud), 2026-10-23, **static review only: every item below is inferred from the code, not run.** Scope: `crates/pr-platform-win/src/dialogs/native/dialog/` (`core`, `modal`, `dpi`, `layout`, `focus`, `events`, `tooltip`, `draw`, `result`, `mod`; about 2 830 lines) and `crates/pr-platform-win/tests/dialogs_native_dialog_*.rs` (7 files, about 3 100 lines, plus `tests/common/dialogs_dialog_fixture.rs`). Compared with C# `NativeDialog.cs` at `7eaee2a2` (the file did not change between `d92d6aae` and `7eaee2a2`: `git diff --stat` is empty), read lines 189-425 (show/close/loop) and 679-735 (command/notify) side by side. The contracts `contracts/W8-18h..18k.md` were used only to name units.

Counts: **high 0, medium 3, low 6** (9 findings). Nothing was found that I could turn into a crash or a leak on the normal path.

## What was checked and found correct (by reading)

- Cookie lifetime (`make_cookie` / `from_cookie` / `free_cookie`, `core.rs:352-379`): the dialog procedure clones the `Rc` before calling anything, so a handler that ends in `cleanup` (which frees the cookie, `result.rs:105-107`) cannot free the dialog under its own feet. `DWLP_USER` is cleared before `DestroyWindow` (`result.rs:90-91`), so late messages (`WM_DESTROY`, `WM_NCDESTROY`) find no dialog. Messages before `WM_INITDIALOG` see `DWLP_USER == 0` and return 0 (`core.rs:392-403`).
- Cleanup order matches C# `Cleanup` (filter removed, `on_closed`, subclasses removed, owner re-enabled before destroy, window destroyed, GDI objects deleted, cookie freed last, `closed` callback last). Child subclass procedures of derived dialogs still have a live cookie during `DestroyWindow` because the cookie is freed after it.
- Constants checked against Win32: `DWLP_USER = 16` (x64), `TTN_GETDISPINFOW = -530`, `NM_CUSTOMDRAW = -12`, `TTM_*` values, `TTF_IDISHWND`, `TTF_SUBCLASS`, `TTS_*`, `TTTOOLINFOW` `cbSize = 64` (V2 size; the 72-byte struct is passed with the smaller `cbSize`, which comctl accepts), `NMTTDISPINFOW.lpszText` at offset 24, `NMCUSTOMDRAW` offsets 24/32 (C# uses the same raw offsets), `DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX = 0x400 | 0x10 | 0x800`, `WS_TABSTOP = 0x10000`.
- `RefCell` borrows: no borrow is held across a hook, a callback or an OS call in the files read (`events.rs:229-237`, `tooltip.rs:213-229`, `layout.rs:137`, `dpi.rs:163`, `result.rs:49,57,68`: each clones or takes the value first).
- No `unwrap`/`expect`/`panic!` in the production paths of these files (only in `#[cfg(test)]`). Panics inside hooks are caught in `dialog_proc` (`core.rs:391-414`) and in both subclass procedures (`pending_panic::guarded`) and rethrown by `rethrow_pending`.
- `show_modal` / `show_modeless` follow the C# order step by step, including `RethrowPending` before the `hwnd == 0` check and the `CleanupGuard` playing the C# `finally` / `catch { Cleanup(); throw; }`.
- Every `unsafe` block read has a `// SAFETY:` line, except that `core.rs` has 11 unsafe items against 8 `// SAFETY:` comments; the three without are `unsafe fn`/`unsafe extern` declarations that carry a `# Safety` doc section instead (`core.rs:349-351,370-372,387-388`), which is acceptable.

## Findings

### M1 (medium, confirmed by reading the path) - test helper can pass without running its check
`crates/pr-platform-win/tests/dialogs_native_dialog_events_native.rs:50-68` (`run_case`) and `tests/common/dialogs_dialog_fixture.rs:84-94` (`show_on_owner`). `show_modal` calls `ready` only when the dialog is not already `done` (`src/dialogs/native/dialog/modal.rs:180-194`). If the dialog is closed before that point (a `WM_QUIT` already in the queue, or any future change that ends the dialog in `WM_INITDIALOG`), `show_modal` returns `Ok(2)` without calling the closure, `run_case` returns, and the test passes with zero assertions. Scenario: a regression makes `on_initialized` call `close()`; every `run_case` test with only `assert_eq!` inside `check` stays green. Fix: have `run_case` set a flag inside `check` and `assert!` it after `show_modal`.

### M2 (medium, confirmed) - all 100 native tests of this scope are `#[ignore]`d, so CI covers only the pure tables
Counts of `#[ignore` per file: close 22 of 25 tests, events_native 25 of 25, layout_native 27 of 27, modal 26 of 26; the three not ignored in `close.rs` and the files `core` (11), `events` (6), `layout` (18) are pure (constants, tables, arithmetic). Nothing in CI exercises the cookie lifetime, the cleanup order, the `DWLP_USER` clearing, the modeless filter or the `WM_DPICHANGED` path. The reasons are all `native: needs desktop` as AGENTS.md requires, so this is a coverage fact, not a defect; it means the claims in "found correct" above are unproven until someone runs `cargo test -p pr-platform-win -- --ignored` on a desktop.

### M3 (medium, suspicion) - `closed` callback and `on_closed` can run during unwinding, a panic there aborts
`modal.rs:88-98` (`CleanupGuard::drop` calls `cleanup`) and `result.rs:109-112` (the `closed` callback is called unguarded, unlike `on_closed` at line 55). If `ready` panics in `show_modal`, or `rethrow_pending()` in `create_window` resumes a captured hook panic (`modal.rs:141`), `cleanup` runs during unwinding; a panic in the user's `closed` closure (modeless only) is then a panic-while-panicking and aborts the process. Outside unwinding the same panic is caught by `dialog_proc`. Kept as "suspicion": needs a modeless dialog whose `closed` closure can panic; I did not find such a caller (not searched outside this scope). Fix: run the callback under `pending_panic::guarded` like `on_closed`.

### L1 (low, suspicion) - the tooltip text buffer can be reallocated while the tooltip may still hold the old pointer
`tooltip.rs:230-242`. When a provider returns more than 255 UTF-16 units, `tip_buffer.resize(..)` (line 234) reallocates and `lpszText` is overwritten with the new pointer; the previous pointer (given to the tooltip in the previous `TTN_GETDISPINFO`) dangles. The doc comment at lines 40-41 states the buffer must not be reallocated while the tooltip may read it. Whether comctl32 copies the string right after the notification (I believe so) was not verified. Scenario: row 1 tip of 10 chars, row 2 tip of 400 chars while the first tip is still displayed. Fix: allocate with the maximum needed once, or keep the old buffer alive until the next request.

### L2 (low, confirmed) - `cleanup` never clears the strong reference to the shell message loop
`result.rs:48-53` takes `filter_id` but leaves `modal.message_loop` as `Some(Rc<MessageLoop>)`. The dialog (and any clone of the handle kept by the opener) therefore keeps the loop alive after it closed. No cycle (the filter holds a `Weak`), so it is a retention, not a leak. Fix: `state.message_loop.borrow_mut().take()` in `cleanup`.

### L3 (low, confirmed) - `first_tab_stop` ignores disabled and hidden controls
`focus.rs:83-89`. It returns the first registered child with `WS_TABSTOP` whatever its `WS_DISABLED` / `WS_VISIBLE` bits; `set_focus` on a disabled window fails silently (`focus.rs:77`) and the dialog opens with no focus. Matches the C# description (same rule), so parity-correct; reported because a derived dialog that disables its first control in `on_initialized` loses the initial focus. Fix only if the product wants it.

### L4 (low, confirmed) - `apply_fonts` deletes the previous fonts even when font creation failed
`dpi.rs:151-183`. `create_fonts` can return `0` (`gdi32::create_font_indirect` failure is "not an error", line 123 doc); the code then sets the stored font to `0`, sends `WM_SETFONT(0)` to all controls and deletes the old, working font (lines 176-181). Scenario: GDI object exhaustion during a DPI change leaves the dialog with the system default font and `measure_units` measuring with font `0` (`dpi.rs:193`). Parity with C# `ApplyFonts` assumed, not checked line by line. Fix: keep the old font when the new one is `0`.

### L5 (low, confirmed) - the modeless filter passes a copy of the message
`modal.rs:357-368`. C# passes the original `Msg` pointer to `IsDialogMessage`; Rust copies it. `IsDialogMessage` does not write to `MSG` in the cases I know of, so no observable difference expected; reported as a known divergence from C#.

### L6 (low, confirmed) - weak assertions in pure tests
- `tests/dialogs_native_dialog_layout.rs:44-60` (`button_row_matches_the_csharp_golden`): the golden fields `row_width` and `placed` are both compared with the same `layout::button_row_total(..)` result, and the golden `button_width` is compared with `button_width_for(&[], ..)` (an empty text list), so the "widest text" branch is not touched by the golden; it is covered only by the hand-written `button_width_for_grows_with_the_widest_text` (line 142).
- `tests/dialogs_native_dialog_layout.rs:175-225` (`initial_frame_*`): expected rectangles are hand-computed constants labelled "C#: PlaceInitially", not golden rows; they match the algorithm as read, but nothing proves they come from the C# run.
- `tests/dialogs_native_dialog_events_native.rs:359` (`set_list_view_sort_on_an_absent_control_is_ignored`): no assertion at all (the test passes unless the call panics); it also needs a desktop.

## Not checkable here (needs Windows / a desktop)

- Whether the 100 ignored tests pass (`cargo test -p pr-platform-win --test dialogs_native_dialog_close --test dialogs_native_dialog_events_native --test dialogs_native_dialog_layout_native --test dialogs_native_dialog_modal -- --ignored`).
- Cookie and `DWLP_USER` behaviour when a modeless dialog is closed from inside its own `WM_COMMAND` handler (cleanup destroys the window inside the dialog procedure).
- Whether comctl32 copies `lpszText` after `TTN_GETDISPINFO` (L1), and the tooltip dark colours.
- DPI change behaviour on two monitors with different scaling (`on_dpi_changed`), and the `WM_GETMINMAXINFO` minimum.
- GDI/user object counts before and after opening and closing a dialog many times (`GetGuiResources`), to confirm cleanup leaks nothing.
- A hook that panics during `WM_INITDIALOG` of a modeless dialog: the captured panic stays in the thread-local slot until someone calls `rethrow_pending`; I did not trace who does on the shell loop.

## Bổ sung 2026-10-23 — đường modeless filter

Nhận định tổng quát ở phần đọc đúng rằng mọi hook panic đều được rethrow cần giới hạn: message loop chỉ resume sau claim hoặc dispatch; ModelessFilter trả true sau IsDialogMessage không đi qua bước đó. Xem [host-dispatcher M1](host-dispatcher.md) (G11); giải quyết một phần câu hỏi ai rethrow trên shell loop. C# cùng cấu trúc, cần quyết định parity. Suy luận từ mã, chưa chạy thử; không cộng lại M1 vào tổng lịch sử của báo cáo này.
