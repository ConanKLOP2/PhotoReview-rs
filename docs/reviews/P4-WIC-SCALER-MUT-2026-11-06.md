Status: evidence report (2026-11-06), task P4, branch `test/mut4-wic-scaler` from origin/master 762360c1. Tests and this file only; no production change, no edit of PARITY, README, BACKLOG, PROPOSED-DECISIONS, task_on_progress or SYNC-LEDGER.

# P4: mutation of `wic/scaler.rs` and `host/native.rs` under OWNER RULE B (strict)

These two files were NOT RUN in P3 (`docs/reviews/P3-MUT-2026-11-06.md`: 25 + 8 mutants, 0 run). This lane ran all 33 current mutants, one `cargo-mutants -j 1` process, own target dir, `CARGO_BUILD_JOBS=2`, no out-of-memory and no link error happened (a mutant took about 35 s build + 1 s test).

## Counts

tested = caught + missed + unviable + timeout. NOT RUN is counted separately.

| file | listed | tested | caught | missed | unviable | timeout | NOT RUN |
|---|---|---|---|---|---|---|---|
| `wic/scaler.rs` | 25 | 25 | 17 | 0 | 8 | 0 | 0 |
| `host/native.rs` | 8 | 8 | 8 | 0 | 0 | 0 | 0 |
| total | 33 | 33 | 25 | 0 | 8 | 0 | 0 |

`native.rs`: the first run (run 1) gave 3 caught and 5 missed; after the new tests were added the 5 survivors were rerun (runs 2 and 3, on the final test file) and are all caught, so the table counts the final verdict of each mutant: 3 caught in run 1 plus 5 caught in run 3. Run 1 itself: 33 tested, 20 caught, 5 missed, 8 unviable, 0 timeout.

## How a verdict was accepted

* Every "caught" log (25 mutants: 20 in run 1, 5 reruns) contains a `panicked at` / `test result: FAILED` of a named test; no timeout occurred, so nothing was counted from a hang.
* Every "unviable" (8, all in `scaler.rs`, all `Default::default()` replacements) has a rustc `error[E0277]` in its log (`ScaleError`, `PixelBuffer`, `ResizePlan` or `DotnetExceptionKind` does not implement `Default`); none has an infrastructure marker (paging file, LNK1102, E0463, metadata stub, os error 1455).
* No mutant is "proven equivalent": there were none to prove, every survivor was killed by a test.

## Commands

* Run 1: `bash tools/dev/mutants.sh pr-platform-win crates/pr-platform-win/src/wic/scaler.rs crates/pr-platform-win/src/host/native.rs --native --tests wic_scaler,wic_repair1,mut3_host_icons_exe,host_icons --cargo-test-arg --lib --budget 36000 --jobs 1` (preflight: 328 tests executed, ok; Found 33 mutants).
* Runs 2 and 3: `bash tools/dev/mutants.sh pr-platform-win crates/pr-platform-win/src/host/native.rs --native --tests mut4_host_native,mut3_host_icons_exe,host_icons --budget 7200 --jobs 1 --skip load_shared_icon` (preflight: 14 tests executed, ok; Found 5 mutants; run 2 before a clippy-only edit of the test file, run 3 on the final file: 5 of 5 caught each).
* `--native` = `-- --include-ignored --test-threads=1` after the cargo-test arguments (the correct libtest form).

## Per-mutant fate

`scaler.rs` (line:column of current master):

| mutant | fate |
|---|---|
| 40:9 `Display::fmt` -> `Ok(Default::default())` | caught |
| 61:9 `dotnet_kind` -> `None` | caught |
| 61:9 `dotnet_kind` -> `Some(Default::default())` | unviable (E0277) |
| 62:13 delete arm `Com` | caught |
| 66:13 delete arm `InsufficientMemory` | caught |
| 91:5 `plan_resize` -> `Ok(Default::default())` | unviable (E0277) |
| 94:14 `<` -> `==`, `>`, `<=` (3) | caught |
| 97:15 `<` -> `==`, `>`, `<=` (3) | caught |
| 100:26 `>` -> `==`, `<`, `>=` (3) | caught |
| 105:14 `==` -> `!=` | caught |
| 105:30 `&&` -> `\|\|` | caught |
| 105:40 `==` -> `!=` | caught |
| 121:5 `resize` -> `Ok(Default::default())` | unviable (E0277) |
| 136:5 `copy_same_size` -> `Ok(Default::default())` | unviable (E0277) |
| 153:9 `wic_error` -> `Default::default()` | unviable (E0277) |
| 158:9 `classify_hresult` -> `Default::default()` | unviable (E0277) |
| 158:20 `==` -> `!=` | caught |
| 166:9 `com_error` -> `Default::default()` | unviable (E0277) |
| 179:9 `scale` -> `Ok(Default::default())` | unviable (E0277) |

`native.rs`:

| mutant | fate | killer |
|---|---|---|
| 27:32 delete `-` (`HWND_MESSAGE`) | KILLED (was missed) | `mut4_host_native::hwnd_message_is_minus_three` (a single constant: the assertion covers the whole domain) |
| 50:5 `get_capture` -> 0 | KILLED (was missed) | `get_capture_returns_the_capturing_window_else_zero` (native) |
| 56:5 `validate_whole_client` -> true | KILLED (was missed) | `validate_whole_client_of_a_bad_handle_is_false` and `validate_whole_client_clears_the_update_region_and_reports_success` (native) |
| 56:5 `validate_whole_client` -> false | KILLED (was missed) | `validate_whole_client_clears_the_update_region_and_reports_success` (native) |
| 62:5 `window_from_point` -> 0 | KILLED (was missed) | `window_from_point_returns_the_visible_window_under_the_point` (native) |
| 70:5 `load_shared_icon` -> 0, 1, -1 (3) | caught in run 1 | `mut3_host_icons_exe` (real RT_GROUP_ICON resource) |

## New tests

`crates/pr-platform-win/tests/mut4_host_native.rs`, 5 tests. Two are plain (`hwnd_message_is_minus_three`, `validate_whole_client_of_a_bad_handle_is_false`). Three are `#[ignore = "native: needs a window station"]` / `#[ignore = "native: needs an unlocked desktop"]`: they create a `BUTTON` popup window on the test thread (a `STATIC` answers HTTRANSPARENT, so `WindowFromPoint` would skip it) and clean it up on drop; the `window_from_point` one needs a visible topmost popup at (200,200) 64x64, so it needs an unlocked interactive desktop, the other two need only a window station. They are run by `mutants.sh --native` and by `cargo test -p pr-platform-win --test mut4_host_native -- --include-ignored --test-threads=1` (5 passed). Without `--include-ignored` the three native mutants (50:5, 56:5 `false`, 62:5) are NOT killed by a default `cargo test`; they are killed only by the native run.

## NOT RUN

None: all 33 listed mutants got a valid verdict.
