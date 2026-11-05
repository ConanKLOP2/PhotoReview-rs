# P9 mutation evidence: `painter/widgets.rs`, all 266 mutants run (2026-11-06)

Status: evidence report, task P9, worktree `mut9-widgets`, branch `test/mut9-widgets`, base `origin/master` c91eecb7.
PARITY.md, README, BACKLOG, PROPOSED-DECISIONS, task_on_progress and SYNC-LEDGER are NOT edited. Production code is unchanged (`crates/pr-platform-win/src/dialogs/native/painter/widgets.rs`
has no diff). Rule B (P-391, strict): a mutant is closed only if caught, unviable, or proven equivalent over the whole domain on which mutant and original differ; a sampled or
bounded run, or a reading of the code, closes nothing; a timeout is never caught; an infrastructure failure is NOT RUN. Method as in P7/P8: `tools/dev/mutants.sh`, `-j 1`, one
cargo-mutants process at a time, own target dir outside the worktree, `--cargo-arg --test=<binary>` per test binary so a mutant does not relink every test binary.

## Result in one paragraph

P3 had 42 of 266 mutants tested and 224 NOT RUN. All **266 now have a verdict**: **257 caught, 6 missed (all six equivalent, see E2 and E3), 3 unviable, 0 timeouts, 0 NOT RUN**
(`tested = 257 + 6 + 3 + 0 = 266`). The first run over all 266 mutants (run A, the existing painter tests) gave 208 caught, 54 missed, 3 unviable, 1 timeout. 48 of the 54 survivors and the timeout were
killed by 17 new tests in `crates/pr-platform-win/tests/mut9_painter_widgets.rs` (two of them by the inline `widgets_reselect_tests`, P-394). The 6 survivors that remain cannot be killed by any
test: five are `|` -> `^` on constant flag words (E2, a one-point domain, proven by a run), one is `>` -> `>=` guarding a call that returns an empty list for the one input where they differ
(E3, a structural argument over the input class, backed by a test over a lattice; a strict verifier may keep it as "run-supported, not killable", like `232:38` of P7). 230:38 (E1) is KILLED, but
only by an arithmetic overflow panic of the test profile; it is equivalent in a release build (see E1).

## Counts

| step | mutants | caught | missed | unviable | timeout | NOT RUN |
|---|---:|---:|---:|---:|---:|---:|
| Run A: all 266, tests `dialogs_painter_widgets` + `mut3_painter_pens` (19 tests, `--include-ignored --test-threads=1`) | 266 | 208 | 54 | 3 | 1 | 0 |
| Run B: the 54 survivors of run A, + `mut9_painter_widgets` (36 tests) | 54 | 46 | 8 | 0 | 0 | 0 |
| Run C: the two `Reselect::drop` survivors (48:9, 48:21), `--lib reselect_tests` | 2 | 2 | 0 | 0 | 0 | 0 |
| Run D: `641:56` (the run A timeout), `mut9_painter_widgets` alone | 2 | 2 | 0 | 0 | 0 | 0 |
| Run F: `641:` (4 mutants), all three binaries in one command, after the bounded guard was added to `dialogs_painter_widgets` | 4 | 4 | 0 | 0 | 0 | 0 |
| **Final, one verdict per mutant** | **266** | **257** | **6** | **3** | **0** | **0** |

Final = run A (208 caught + 3 unviable) + 46 survivors caught in run B + 2 in run C + the timeout caught in runs D and F; the 6 missed of run B that stay missed are the equivalents.
Run E (the same as F before the guard was added) showed the problem: with the three binaries together `641:56 / -> *` was a TIMEOUT again, because the old test
`tick_xs_degenerate_inputs` (binary `dialogs_painter_widgets`, runs before `mut9_*`) loops 2^31 times under the mutant. The one-line bounded guard added to that test (an `assert_eq!` on a
count of 1000, before the huge-count call) makes the mutant fail by assertion in the old binary; runs D and F are caught by assertion in a few seconds (log `tick_xs_step_follows_the_span_per_gap_with_a_bounded_count`:
the mutant gives 1000 ticks instead of `[0, 6, 10]`).

Verdict check over every mutant log of runs A, B, C, D and F: a log containing `paging file`, `LNK1xxx`, `E0463`, `E0786`, `os error 1455`, `only metadata stub`, `memory allocation of` or an
`0xc0000...` status would be NOT RUN. Result: 0 suspect logs, 0 infrastructure failures, 0 abnormal exits. The 3 unviable mutants are `Default::default()` bodies of `check_layout`, `combo_layout`
and `trackbar_layout` (`CheckLayout`, `ComboLayout`, `TrackbarLayout` do not implement `Default`, rustc E0277).

## How it ran

Environment: `CARGO_TARGET_DIR=C:\Users\PC\Projects\target-mut9` (own tests, clippy), `TEMP=TMP=C:\Users\PC\Projects\target-mut9-tmp`, `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`,
`CARGO_PROFILE_DEV_DEBUG=0`; the mutation target dir of the script `target-mut9-tmp\mut-target-pr-platform-win`. cargo-mutants via `tools/dev/mutants.sh`, `--baseline skip`, default 90 s test timeout.
All native tests run with `--cargo-test-arg -- --cargo-test-arg --include-ignored --cargo-test-arg --test-threads=1` and the preflight of the script (unmutated test command must run tests and exit 0:
19, 36, 2, 17 tests executed in runs A, B, C, D).

```
W=crates/pr-platform-win/src/dialogs/native/painter/widgets.rs
# list: 266 mutants
cargo mutants -p pr-platform-win --file $W --list
# run A: all 266 (34 min)
bash tools/dev/mutants.sh pr-platform-win $W --cargo-arg --test=dialogs_painter_widgets --cargo-arg --test=mut3_painter_pens \
  --cargo-test-arg -- --cargo-test-arg --include-ignored --cargo-test-arg --test-threads=1 --budget 14400 --jobs 1
# run B: the 54 survivors (one -F per survivor, from the script's filters.txt), 8 min
bash tools/dev/mutants.sh pr-platform-win $W --cargo-arg --test=mut9_painter_widgets --cargo-arg --test=dialogs_painter_widgets \
  --cargo-arg --test=mut3_painter_pens --cargo-test-arg -- --cargo-test-arg --include-ignored --cargo-test-arg --test-threads=1 --budget 7200 --jobs 1 -F '<survivor>' ...
# run C: Reselect::drop with the inline tests
bash tools/dev/mutants.sh pr-platform-win $W --cargo-arg --lib --cargo-test-arg reselect_tests --cargo-test-arg -- \
  --cargo-test-arg --include-ignored --cargo-test-arg --test-threads=1 --budget 3000 --jobs 1 -F 'widgets.rs:48:'
# run D / F: the timeout mutant
bash tools/dev/mutants.sh pr-platform-win $W --cargo-arg --test=mut9_painter_widgets ... -F 'widgets.rs:641:56'
bash tools/dev/mutants.sh pr-platform-win $W --cargo-arg --test=mut9_painter_widgets --cargo-arg --test=dialogs_painter_widgets --cargo-arg --test=mut3_painter_pens ... -F 'widgets.rs:641:'
```

Quality gates on the final tree: `rustfmt --edition 2024 --check` on both changed test files (clean), `cargo clippy -p pr-platform-win --tests -- -D warnings` (clean), `cargo test -p pr-platform-win
--test mut9_painter_widgets --test dialogs_painter_widgets` (mut9: 6 pass, 11 `#[ignore = "native: ..."]`; dialogs_painter_widgets: 12 pass, 5 ignored), the same with `--include-ignored --test-threads=1` (17 + 17 pass), `cargo test -p pr-platform-win --lib reselect_tests` (2 pass).

## New tests (`crates/pr-platform-win/tests/mut9_painter_widgets.rs`)

Pure tests (run everywhere): `check_layout_focus_rect_of_the_widest_control_does_not_overflow`, `the_text_flag_constants_share_no_bit_so_or_and_xor_agree`,
`combo_arrow_pen_is_one_pixel_wide_when_two_scales_to_one`, `tick_xs_step_follows_the_span_per_gap_with_a_bounded_count`, `tick_xs_does_not_repeat_the_last_tick_when_the_step_reaches_it`,
`trackbar_layout_with_equal_range_ends_has_no_ticks_for_every_mode`.

Native pixel tests, all `#[ignore = "native: needs desktop"]` like the other painter pixel tests (they use the shared fixture: a dark owner window shown at (-20000, -20000) without activation, `PrintWindow` into
a memory bitmap, so no attended desktop is needed; they ran on this unattended session): `the_unchecked_check_box_is_a_border_ring_around_the_input_fill`,
`the_checked_radio_circle_and_dot_span_the_layout_extents`, `the_focus_rectangle_follows_the_focus_bit_of_the_button_state`, `the_closed_combo_draws_the_text_of_the_selected_item`,
`an_empty_item_text_is_not_fetched_with_cb_getlbtext`, `the_unbordered_progress_bar_keeps_its_one_pixel_frame_around_the_fill`, `the_bordered_progress_bar_paints_no_second_frame_in_its_client`,
`custom_ticks_are_drawn_at_the_table_positions_and_auto_ticks_are_even`, `the_tick_table_is_requested_only_for_custom_visible_ticks_with_a_range`, `every_inner_tick_of_the_table_is_read`,
`the_trackbar_focus_rectangle_needs_the_focus_and_visible_cues`.

Techniques: (1) a subclass installed AFTER `Painter::attach` (so it is first in the chain) answers selected messages (`BM_GETSTATE`, `WM_QUERYUISTATE`, `TBM_GET*`) and logs every message it sees; a
mutated condition is found by the pixels or by the messages that were (not) sent (`TBM_GETPTICS`, `CB_GETLBTEXT`). (2) The trackbar tick table is a `VirtualAlloc` buffer that ends exactly at a
`PAGE_NOACCESS` page; a vectored exception handler (raw `AddVectoredExceptionHandler`, declared in the test) makes the page readable, records the touch and resumes, so an over-read (703:52 `-` -> `+`)
fails an assertion instead of crashing the test process. (3) The check box is compared pixel by pixel as a whole client image (border ring, fill, window colour), the radio circle and dot by their
extents along the middle row and column. (4) Real focus: the offscreen owner takes the focus on this machine (asserted, not skipped). (5) The v5 progress bar of this test process has
`WS_EX_STATICEDGE`, so the "unbordered" test strips the edge styles first (`strip_edges`).

One existing test was touched: `dialogs_painter_widgets.rs::tick_xs_degenerate_inputs` got a bounded `assert_eq!` before its huge-count call (test only, see run E/F above).

## Equivalent mutants

**E1, 230:38 `-` -> `+` in `check_layout` (KILLED, equivalent in release).** `shown = (text_rect.right - text_rect.left).min(text_width)` feeds `focus_rect.right = width.min(text_rect.left + shown + 2)`.
`text_rect.left` is `box + gap >= 0` (or 0 with `BS_LEFTTEXT`, where `+` and `-` are identical). Without overflow: if `text_width <= right - left` both give `text_width`; otherwise the original
gives `right - left`, so `left + shown + 2 = width + 2` (right = width) and the mutant gives `min(right + left, text_width) >= right - left`, so `left + shown + 2 >= width + 2` and the result is `width` again.
Original and mutant therefore return the same `CheckLayout` for every input where `right + left` does not overflow `i32`. They differ only when it does: with `width = i32::MAX - 2` the original is still
computable (`left + shown + 2 = i32::MAX`) and the mutant panics "attempt to add with overflow" (debug and test profile have overflow checks on; `cargo-mutants` reports it as caught by
`check_layout_focus_rect_of_the_widest_control_does_not_overflow`). In a release build (overflow checks off) the mutant wraps and the clamp `width.min(..)` hides it, so it would stay equivalent there. This is
a kill that depends on the test profile, flagged on purpose.

**E2, 224:20, 224:36 (`check_layout`) and 386:35, 386:48, 386:62 (`combo_layout`) `|` -> `^` (MISSED, equivalent over the whole domain).** The expressions are `DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS`
(0x4, 0x20, 0x8000) and `DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS` (0x20, 0x4, 0x800, 0x8000): constants with no input at all, so the domain is one point. `a | b == a ^ b` exactly when
`a & b == 0`; `the_text_flag_constants_share_no_bit_so_or_and_xor_agree` runs all six pairs (`a & b == 0`) and checks the two flag words equal the sums. This is a run over the complete (one-point) domain.

**E3, 562:68 `>` -> `>=` in `trackbar_layout` (MISSED, equivalent; structural class argument).** `range_max > range_min` guards the tick computation. The only input class where `>` and `>=` differ is
`range_max == range_min`; there the mutant enters the branch, computes `top`/`bottom` with `wrapping_add` and `min` (no panic possible) and calls `tick_xs`, whose FIRST statement is
`if count < 2 || max <= min { return Vec::new(); }`, so for EVERY value of the other arguments (channel, count, mode, table, scale) the loop body never runs and `ticks` stays empty, exactly as when the branch is skipped.
For `range_max < range_min` both skip, for `range_max > range_min` both enter. `trackbar_layout_with_equal_range_ends_has_no_ticks_for_every_mode` runs the class over a lattice (2 modes x 8 counts incl.
`i32::MAX` x 3 scales x 2 tables x 5 range values) as support. It is NOT a whole-domain run; the closure rests on the one-statement argument above, so a strict verifier may count it as open.

Nothing else is open. In particular `783:67 > -> >=` (not equivalent as a result, equivalent as a returned string) is KILLED through the message log: for an empty item the original never sends `CB_GETLBTEXT`, the mutant does.

## Per-mutant fate (all 266, in file order)

"run A" is the verdict of the first run with the existing tests; "final" is the verdict with the new tests. `tested = caught + missed + unviable + timeout` per row of Counts.

| line:col | mutant | function | run A | final | killed by / argument |
|---|---|---|---|---|---|
| 34:9 | Owned::drop with () | Owned::drop | caught | caught |  |
| 34:19 | != with == | Owned::drop | caught | caught |  |
| 48:9 | Reselect::drop with () | Reselect::drop | MISSED | caught | widgets_reselect_tests (lib, P-394) |
| 48:21 | != with == | Reselect::drop | MISSED | caught | widgets_reselect_tests (lib, P-394) |
| 56:5 | with_pen with () | with_pen | caught | caught |  |
| 66:5 | with_pen_and_brush with () | with_pen_and_brush | caught | caught |  |
| 81:5 | round_even -> i32 with -1 | round_even | caught | caught |  |
| 81:5 | round_even -> i32 with 0 | round_even | caught | caught |  |
| 81:5 | round_even -> i32 with 1 | round_even | caught | caught |  |
| 144:5 | check_layout -> CheckLayout with Default::default() | check_layout | unviable | unviable | no `Default` impl for the return type (rustc E0277) |
| 161:41 | - with + | check_layout | caught | caught |  |
| 161:41 | - with / | check_layout | caught | caught |  |
| 164:23 | - with + | check_layout | caught | caught |  |
| 164:23 | - with / | check_layout | caught | caught |  |
| 164:35 | / with % | check_layout | caught | caught |  |
| 164:35 | / with * | check_layout | caught | caught |  |
| 166:18 | - with + | check_layout | caught | caught |  |
| 166:18 | - with / | check_layout | caught | caught |  |
| 166:30 | / with % | check_layout | caught | caught |  |
| 166:30 | / with * | check_layout | caught | caught |  |
| 168:25 | delete ! | check_layout | caught | caught |  |
| 177:23 | delete ! | check_layout | caught | caught |  |
| 177:30 | && with \|\| | check_layout | caught | caught |  |
| 177:41 | && with \|\| | check_layout | caught | caught |  |
| 183:22 | && with \|\| | check_layout | caught | caught |  |
| 185:32 | * with + | check_layout | caught | caught |  |
| 185:32 | * with / | check_layout | caught | caught |  |
| 187:17 | delete ! | check_layout | caught | caught |  |
| 187:24 | && with \|\| | check_layout | caught | caught |  |
| 191:29 | + with * | check_layout | caught | caught |  |
| 191:29 | + with - | check_layout | caught | caught |  |
| 191:48 | * with + | check_layout | caught | caught |  |
| 191:48 | * with / | check_layout | caught | caught |  |
| 192:28 | + with * | check_layout | caught | caught |  |
| 192:28 | + with - | check_layout | caught | caught |  |
| 192:47 | * with + | check_layout | caught | caught |  |
| 192:47 | * with / | check_layout | caught | caught |  |
| 195:29 | + with * | check_layout | caught | caught |  |
| 195:29 | + with - | check_layout | caught | caught |  |
| 195:48 | * with + | check_layout | caught | caught |  |
| 195:48 | * with / | check_layout | caught | caught |  |
| 196:28 | + with * | check_layout | caught | caught |  |
| 196:28 | + with - | check_layout | caught | caught |  |
| 196:47 | * with + | check_layout | caught | caught |  |
| 196:47 | * with / | check_layout | caught | caught |  |
| 199:29 | + with * | check_layout | caught | caught |  |
| 199:29 | + with - | check_layout | caught | caught |  |
| 199:48 | * with + | check_layout | caught | caught |  |
| 199:48 | * with / | check_layout | caught | caught |  |
| 200:28 | + with * | check_layout | caught | caught |  |
| 200:28 | + with - | check_layout | caught | caught |  |
| 200:47 | * with + | check_layout | caught | caught |  |
| 200:47 | * with / | check_layout | caught | caught |  |
| 210:30 | - with + | check_layout | caught | caught |  |
| 210:30 | - with / | check_layout | caught | caught |  |
| 215:28 | + with * | check_layout | caught | caught |  |
| 215:28 | + with - | check_layout | caught | caught |  |
| 224:20 | \| with & | check_layout | caught | caught | equivalent, E2 |
| 224:20 | \| with ^ | check_layout | MISSED | MISSED | equivalent, E2 |
| 224:36 | \| with & | check_layout | caught | caught | equivalent, E2 |
| 224:36 | \| with ^ | check_layout | MISSED | MISSED | equivalent, E2 |
| 227:20 | \|= with &= | check_layout | caught | caught |  |
| 229:31 | && with \|\| | check_layout | caught | caught |  |
| 229:34 | delete ! | check_layout | caught | caught |  |
| 229:46 | && with \|\| | check_layout | caught | caught |  |
| 230:38 | - with + | check_layout | MISSED | caught | mut9: check_layout_focus_rect_of_the_widest_control_does_not_overflow (overflow panic, see E1) |
| 230:38 | - with / | check_layout | caught | caught |  |
| 234:22 | - with + | check_layout | caught | caught |  |
| 234:22 | - with / | check_layout | caught | caught |  |
| 234:37 | / with % | check_layout | caught | caught |  |
| 234:37 | / with * | check_layout | caught | caught |  |
| 237:34 | - with + | check_layout | caught | caught |  |
| 237:34 | - with / | check_layout | caught | caught |  |
| 238:28 | - with + | check_layout | caught | caught |  |
| 238:28 | - with / | check_layout | caught | caught |  |
| 239:45 | + with * | check_layout | caught | caught |  |
| 239:45 | + with - | check_layout | caught | caught |  |
| 239:53 | + with * | check_layout | caught | caught |  |
| 239:53 | + with - | check_layout | caught | caught |  |
| 240:41 | + with * | check_layout | caught | caught |  |
| 240:41 | + with - | check_layout | caught | caught |  |
| 240:55 | + with * | check_layout | caught | caught |  |
| 240:55 | + with - | check_layout | caught | caught |  |
| 262:5 | draw_check_or_radio with () | draw_check_or_radio | caught | caught |  |
| 267:68 | != with == | draw_check_or_radio | caught | caught |  |
| 278:31 | & with ^ | draw_check_or_radio | MISSED | caught | mut9: the_focus_rectangle_follows_the_focus_bit_of_the_button_state |
| 278:31 | & with \| | draw_check_or_radio | MISSED | caught | mut9: the_focus_rectangle_follows_the_focus_bit_of_the_button_state |
| 278:43 | != with == | draw_check_or_radio | MISSED | caught | mut9: the_focus_rectangle_follows_the_focus_bit_of_the_button_state |
| 279:26 | & with ^ | draw_check_or_radio | caught | caught |  |
| 279:26 | & with \| | draw_check_or_radio | caught | caught |  |
| 279:41 | != with == | draw_check_or_radio | caught | caught |  |
| 280:26 | & with ^ | draw_check_or_radio | caught | caught |  |
| 280:26 | & with \| | draw_check_or_radio | caught | caught |  |
| 280:40 | != with == | draw_check_or_radio | caught | caught |  |
| 289:42 | + with * | draw_check_or_radio | caught | caught |  |
| 289:42 | + with - | draw_check_or_radio | caught | caught |  |
| 289:54 | + with * | draw_check_or_radio | MISSED | caught | mut9: the_checked_radio_circle_and_dot_span_the_layout_extents |
| 289:54 | + with - | draw_check_or_radio | caught | caught |  |
| 293:35 | + with * | draw_check_or_radio | MISSED | caught | mut9: the_checked_radio_circle_and_dot_span_the_layout_extents |
| 293:35 | + with - | draw_check_or_radio | MISSED | caught | mut9: the_checked_radio_circle_and_dot_span_the_layout_extents |
| 293:48 | + with * | draw_check_or_radio | caught | caught |  |
| 293:48 | + with - | draw_check_or_radio | MISSED | caught | mut9: the_checked_radio_circle_and_dot_span_the_layout_extents |
| 293:62 | + with * | draw_check_or_radio | caught | caught |  |
| 293:62 | + with - | draw_check_or_radio | caught | caught |  |
| 293:69 | - with + | draw_check_or_radio | MISSED | caught | mut9: the_checked_radio_circle_and_dot_span_the_layout_extents |
| 293:69 | - with / | draw_check_or_radio | caught | caught |  |
| 293:82 | + with * | draw_check_or_radio | MISSED | caught | mut9: the_checked_radio_circle_and_dot_span_the_layout_extents |
| 293:82 | + with - | draw_check_or_radio | caught | caught |  |
| 293:89 | - with + | draw_check_or_radio | MISSED | caught | mut9: the_checked_radio_circle_and_dot_span_the_layout_extents |
| 293:89 | - with / | draw_check_or_radio | caught | caught |  |
| 297:35 | + with * | draw_check_or_radio | caught | caught |  |
| 297:35 | + with - | draw_check_or_radio | caught | caught |  |
| 297:47 | + with * | draw_check_or_radio | MISSED | caught | mut9: the_unchecked_check_box_is_a_border_ring_around_the_input_fill |
| 297:47 | + with - | draw_check_or_radio | caught | caught |  |
| 298:36 | + with * | draw_check_or_radio | MISSED | caught | mut9: the_unchecked_check_box_is_a_border_ring_around_the_input_fill |
| 298:36 | + with - | draw_check_or_radio | MISSED | caught | mut9: the_unchecked_check_box_is_a_border_ring_around_the_input_fill |
| 298:48 | + with * | draw_check_or_radio | MISSED | caught | mut9: the_unchecked_check_box_is_a_border_ring_around_the_input_fill |
| 298:48 | + with - | draw_check_or_radio | caught | caught |  |
| 353:5 | combo_layout -> ComboLayout with Default::default() | combo_layout | unviable | unviable | no `Default` impl for the return type (rustc E0277) |
| 362:25 | delete ! | combo_layout | caught | caught |  |
| 364:23 | \|\| with && | combo_layout | caught | caught |  |
| 376:31 | - with + | combo_layout | caught | caught |  |
| 376:31 | - with / | combo_layout | caught | caught |  |
| 377:24 | - with + | combo_layout | caught | caught |  |
| 377:24 | - with / | combo_layout | caught | caught |  |
| 379:20 | - with + | combo_layout | caught | caught |  |
| 379:20 | - with / | combo_layout | caught | caught |  |
| 379:34 | / with % | combo_layout | caught | caught |  |
| 379:34 | / with * | combo_layout | caught | caught |  |
| 379:38 | - with + | combo_layout | caught | caught |  |
| 379:38 | - with / | combo_layout | caught | caught |  |
| 380:21 | / with % | combo_layout | caught | caught |  |
| 380:21 | / with * | combo_layout | caught | caught |  |
| 386:35 | \| with & | combo_layout | caught | caught | equivalent, E2 |
| 386:35 | \| with ^ | combo_layout | MISSED | MISSED | equivalent, E2 |
| 386:48 | \| with & | combo_layout | caught | caught | equivalent, E2 |
| 386:48 | \| with ^ | combo_layout | MISSED | MISSED | equivalent, E2 |
| 386:62 | \| with & | combo_layout | caught | caught | equivalent, E2 |
| 386:62 | \| with ^ | combo_layout | MISSED | MISSED | equivalent, E2 |
| 388:46 | > with < | combo_layout | caught | caught |  |
| 388:46 | > with == | combo_layout | caught | caught |  |
| 388:46 | > with >= | combo_layout | MISSED | caught | mut9: combo_arrow_pen_is_one_pixel_wide_when_two_scales_to_one |
| 396:23 | - with + | combo_layout | caught | caught |  |
| 396:23 | - with / | combo_layout | caught | caught |  |
| 397:23 | - with + | combo_layout | caught | caught |  |
| 397:23 | - with / | combo_layout | caught | caught |  |
| 397:30 | / with % | combo_layout | caught | caught |  |
| 397:30 | / with * | combo_layout | caught | caught |  |
| 401:23 | + with * | combo_layout | caught | caught |  |
| 401:23 | + with - | combo_layout | caught | caught |  |
| 401:30 | / with % | combo_layout | caught | caught |  |
| 401:30 | / with * | combo_layout | caught | caught |  |
| 401:34 | + with * | combo_layout | caught | caught |  |
| 401:34 | + with - | combo_layout | caught | caught |  |
| 404:23 | + with * | combo_layout | caught | caught |  |
| 404:23 | + with - | combo_layout | caught | caught |  |
| 405:23 | - with + | combo_layout | caught | caught |  |
| 405:23 | - with / | combo_layout | caught | caught |  |
| 405:30 | / with % | combo_layout | caught | caught |  |
| 405:30 | / with * | combo_layout | caught | caught |  |
| 408:30 | && with \|\| | combo_layout | caught | caught |  |
| 408:33 | delete ! | combo_layout | caught | caught |  |
| 408:42 | && with \|\| | combo_layout | caught | caught |  |
| 408:45 | delete ! | combo_layout | caught | caught |  |
| 411:32 | - with + | combo_layout | caught | caught |  |
| 411:32 | - with / | combo_layout | caught | caught |  |
| 412:28 | - with + | combo_layout | caught | caught |  |
| 412:28 | - with / | combo_layout | caught | caught |  |
| 420:5 | draw_combo_box with () | draw_combo_box | caught | caught |  |
| 421:31 | == with != | draw_combo_box | caught | caught |  |
| 423:75 | != with == | draw_combo_box | caught | caught |  |
| 441:14 | >= with < | draw_combo_box | MISSED | caught | mut9: the_closed_combo_draws_the_text_of_the_selected_item |
| 459:5 | progress_fill_width -> Option<i32> with None | progress_fill_width | caught | caught |  |
| 459:5 | progress_fill_width -> Option<i32> with Some(-1) | progress_fill_width | caught | caught |  |
| 459:5 | progress_fill_width -> Option<i32> with Some(0) | progress_fill_width | caught | caught |  |
| 459:5 | progress_fill_width -> Option<i32> with Some(1) | progress_fill_width | caught | caught |  |
| 459:13 | <= with > | progress_fill_width | caught | caught |  |
| 463:59 | / with % | progress_fill_width | caught | caught |  |
| 463:59 | / with * | progress_fill_width | caught | caught |  |
| 465:62 | * with + | progress_fill_width | caught | caught |  |
| 465:62 | * with / | progress_fill_width | caught | caught |  |
| 474:5 | draw_progress with () | draw_progress | caught | caught |  |
| 476:46 | > with < | draw_progress | MISSED | caught | mut9: the_unbordered_progress_bar_keeps_its_one_pixel_frame_around_the_fill, the_bordered_progress_bar_paints_no_second_frame_in_its_client |
| 476:46 | > with == | draw_progress | MISSED | caught | mut9: the_unbordered_progress_bar_keeps_its_one_pixel_frame_around_the_fill, the_bordered_progress_bar_paints_no_second_frame_in_its_client |
| 476:46 | > with >= | draw_progress | MISSED | caught | mut9: the_unbordered_progress_bar_keeps_its_one_pixel_frame_around_the_fill, the_bordered_progress_bar_paints_no_second_frame_in_its_client |
| 477:27 | delete ! | draw_progress | MISSED | caught | mut9: the_unbordered_progress_bar_keeps_its_one_pixel_frame_around_the_fill |
| 478:8 | delete ! | draw_progress | MISSED | caught | mut9: the_unbordered_progress_bar_keeps_its_one_pixel_frame_around_the_fill |
| 490:39 | + with * | draw_progress | caught | caught |  |
| 490:39 | + with - | draw_progress | caught | caught |  |
| 490:56 | - with + | draw_progress | MISSED | caught | mut9: the_unbordered_progress_bar_keeps_its_one_pixel_frame_around_the_fill |
| 490:56 | - with / | draw_progress | caught | caught |  |
| 550:5 | trackbar_layout -> TrackbarLayout with Default::default() | trackbar_layout | unviable | unviable | no `Default` impl for the return type (rustc E0277) |
| 554:56 | / with % | trackbar_layout | caught | caught |  |
| 554:56 | / with * | trackbar_layout | caught | caught |  |
| 555:47 | / with % | trackbar_layout | caught | caught |  |
| 555:47 | / with * | trackbar_layout | caught | caught |  |
| 557:61 | / with % | trackbar_layout | caught | caught |  |
| 557:61 | / with * | trackbar_layout | caught | caught |  |
| 562:8 | delete ! | trackbar_layout | caught | caught |  |
| 562:24 | && with \|\| | trackbar_layout | caught | caught |  |
| 562:44 | >= with < | trackbar_layout | caught | caught |  |
| 562:49 | && with \|\| | trackbar_layout | caught | caught |  |
| 562:68 | > with < | trackbar_layout | caught | caught | equivalent, E3 |
| 562:68 | > with == | trackbar_layout | caught | caught | equivalent, E3 |
| 562:68 | > with >= | trackbar_layout | MISSED | MISSED | equivalent, E3 |
| 606:21 | delete ! | trackbar_layout | caught | caught |  |
| 634:5 | tick_xs -> Vec<i32> with vec![-1] | tick_xs | caught | caught |  |
| 634:5 | tick_xs -> Vec<i32> with vec![0] | tick_xs | caught | caught |  |
| 634:5 | tick_xs -> Vec<i32> with vec![1] | tick_xs | caught | caught |  |
| 634:5 | tick_xs -> Vec<i32> with vec![] | tick_xs | caught | caught |  |
| 634:14 | < with <= | tick_xs | caught | caught |  |
| 634:14 | < with == | tick_xs | caught | caught |  |
| 634:14 | < with > | tick_xs | caught | caught |  |
| 634:18 | \|\| with && | tick_xs | caught | caught |  |
| 634:25 | <= with > | tick_xs | caught | caught |  |
| 639:32 | - with + | tick_xs | caught | caught |  |
| 639:32 | - with / | tick_xs | caught | caught |  |
| 641:37 | / with % | tick_xs | caught | caught |  |
| 641:37 | / with * | tick_xs | caught | caught |  |
| 641:56 | / with % | tick_xs | caught | caught |  |
| 641:56 | / with * | tick_xs | TIMEOUT | caught | mut9: tick_xs_step_follows_the_span_per_gap_with_a_bounded_count + bounded guard in dialogs_painter_widgets::tick_xs_degenerate_inputs |
| 647:54 | * with + | tick_xs | caught | caught |  |
| 647:54 | * with / | tick_xs | caught | caught |  |
| 647:62 | / with % | tick_xs | caught | caught |  |
| 647:62 | / with * | tick_xs | caught | caught |  |
| 648:21 | == with != | tick_xs | caught | caught |  |
| 650:21 | == with != | tick_xs | caught | caught |  |
| 650:30 | - with + | tick_xs | caught | caught |  |
| 650:30 | - with / | tick_xs | caught | caught |  |
| 655:53 | - with + | tick_xs | caught | caught |  |
| 655:53 | - with / | tick_xs | caught | caught |  |
| 659:89 | * with + | tick_xs | caught | caught |  |
| 659:89 | * with / | tick_xs | caught | caught |  |
| 659:107 | / with % | tick_xs | caught | caught |  |
| 659:107 | / with * | tick_xs | caught | caught |  |
| 661:15 | - with + | tick_xs | caught | caught |  |
| 661:15 | - with / | tick_xs | MISSED | caught | mut9: tick_xs_does_not_repeat_the_last_tick_when_the_step_reaches_it |
| 661:20 | % with + | tick_xs | caught | caught |  |
| 661:20 | % with / | tick_xs | caught | caught |  |
| 661:27 | != with == | tick_xs | caught | caught |  |
| 673:5 | draw_trackbar with () | draw_trackbar | caught | caught |  |
| 685:26 | & with ^ | draw_trackbar | caught | caught |  |
| 685:26 | & with \| | draw_trackbar | caught | caught |  |
| 685:40 | != with == | draw_trackbar | caught | caught |  |
| 686:28 | & with ^ | draw_trackbar | MISSED | caught | mut9: custom_ticks_are_drawn_at_the_table_positions_and_auto_ticks_are_even |
| 686:28 | & with \| | draw_trackbar | MISSED | caught | mut9: custom_ticks_are_drawn_at_the_table_positions_and_auto_ticks_are_even |
| 686:44 | != with == | draw_trackbar | MISSED | caught | mut9: custom_ticks_are_drawn_at_the_table_positions_and_auto_ticks_are_even |
| 697:26 | delete ! | draw_trackbar | MISSED | caught | mut9: the_tick_table_is_requested_only_for_custom_visible_ticks_with_a_range |
| 697:36 | && with \|\| | draw_trackbar | MISSED | caught | mut9: the_tick_table_is_requested_only_for_custom_visible_ticks_with_a_range |
| 697:39 | delete ! | draw_trackbar | MISSED | caught | mut9: the_tick_table_is_requested_only_for_custom_visible_ticks_with_a_range |
| 697:51 | && with \|\| | draw_trackbar | MISSED | caught | mut9: the_tick_table_is_requested_only_for_custom_visible_ticks_with_a_range |
| 697:65 | >= with < | draw_trackbar | MISSED | caught | mut9: the_tick_table_is_requested_only_for_custom_visible_ticks_with_a_range |
| 697:70 | && with \|\| | draw_trackbar | MISSED | caught | mut9: the_tick_table_is_requested_only_for_custom_visible_ticks_with_a_range |
| 697:83 | > with < | draw_trackbar | MISSED | caught | mut9: the_tick_table_is_requested_only_for_custom_visible_ticks_with_a_range |
| 697:83 | > with == | draw_trackbar | MISSED | caught | mut9: the_tick_table_is_requested_only_for_custom_visible_ticks_with_a_range |
| 697:83 | > with >= | draw_trackbar | MISSED | caught | mut9: the_tick_table_is_requested_only_for_custom_visible_ticks_with_a_range |
| 703:52 | - with + | draw_trackbar | MISSED | caught | mut9: custom_ticks_are_drawn_at_the_table_positions_and_auto_ticks_are_even, every_inner_tick_of_the_table_is_read (guard page) |
| 703:52 | - with / | draw_trackbar | MISSED | caught | mut9: custom_ticks_are_drawn_at_the_table_positions_and_auto_ticks_are_even, every_inner_tick_of_the_table_is_read (guard page) |
| 762:20 | == with != | draw_trackbar | MISSED | caught | mut9: the_trackbar_focus_rectangle_needs_the_focus_and_visible_cues |
| 762:28 | && with \|\| | draw_trackbar | MISSED | caught | mut9: the_trackbar_focus_rectangle_needs_the_focus_and_visible_cues |
| 762:31 | delete ! | draw_trackbar | MISSED | caught | mut9: the_trackbar_focus_rectangle_needs_the_focus_and_visible_cues |
| 778:5 | combo_item_text -> String with "xyzzy".into() | combo_item_text | MISSED | caught | mut9: the_closed_combo_draws_the_text_of_the_selected_item, an_empty_item_text_is_not_fetched_with_cb_getlbtext |
| 778:5 | combo_item_text -> String with String::new() | combo_item_text | MISSED | caught | mut9: the_closed_combo_draws_the_text_of_the_selected_item, an_empty_item_text_is_not_fetched_with_cb_getlbtext |
| 783:67 | > with < | combo_item_text | MISSED | caught | mut9: the_closed_combo_draws_the_text_of_the_selected_item (== and <), an_empty_item_text_is_not_fetched_with_cb_getlbtext (>=) |
| 783:67 | > with == | combo_item_text | MISSED | caught | mut9: the_closed_combo_draws_the_text_of_the_selected_item (== and <), an_empty_item_text_is_not_fetched_with_cb_getlbtext (>=) |
| 783:67 | > with >= | combo_item_text | MISSED | caught | mut9: the_closed_combo_draws_the_text_of_the_selected_item (== and <), an_empty_item_text_is_not_fetched_with_cb_getlbtext (>=) |
