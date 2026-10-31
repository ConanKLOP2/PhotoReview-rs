# CL-MUT-DIALOGS2 2026-10-31: painter/widgets.rs combo/trackbar/tick rerun, partial and time-boxed

Status: PARTIAL. 10 of 99 mutants tested (all caught, genuinely); 88 not run; native base (template, chrome, edit_centering, list_dialog) not run.

## What was run
- Preflight: `cargo test -p pr-platform-win --test dialogs_painter_widgets -- --include-ignored --test-threads=1` -> `running 17 tests`, 17 passed, exit 0.
- Mutation: `cargo mutants -p pr-platform-win --baseline skip --timeout 300 -j 1 --file "**/painter/widgets.rs" -F "combo_layout|trackbar_layout|tick_xs"` with `--cargo-test-arg=--test --cargo-test-arg=dialogs_painter_widgets --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`, own `target-mut`, `CARGO_PROFILE_*_DEBUG=0` (all removed afterwards). Run in file order (not shuffled).
- The machine was heavily loaded: about 7-8 minutes per mutant, so the ~75 minute time-box allowed only 11 mutants; the 11th (379:34 `/` -> `%`) was interrupted and is NOT counted.

## Golden-only check
`dialogs_painter_widgets.rs` contains no golden test (grep `golden` = 0 hits), and `grep -rl 'no tests/golden above'` over the mutant logs = 0 files. Every caught mutant below has `running 17 tests` in its log and 2-4 FAILED non-golden tests, so all 10 are genuinely caught (this also settles the predecessor's 5 "caught": they were not golden-only; the earlier 379:34 and 408:42 stay open).

## Result (real numbers): tested 11 of 99 listed, caught 10, missed 0, timeout 0, unviable 1, interrupted 1 (not counted), NOT RUN 87
| mutant (widgets.rs) | verdict |
|---|---|
| 353:5 combo_layout -> Default::default() | unviable (no Default impl) |
| 362:25 delete `!` | caught (4 failed) |
| 364:23 `\|\|` -> `&&` | caught (4 failed) |
| 376:31 `-` -> `+` / `/` | caught, caught (3 / 2 failed) |
| 377:24 `-` -> `+` / `/` | caught, caught (2 / 2) |
| 379:20 `-` -> `+` / `/` | caught, caught (2 / 2) |
| 379:38 `-` -> `+` / `/` | caught, caught (2 / 2) |

No survivor, so no killer was needed: `crates/pr-platform-win/tests/mut_dialogs2_killers.rs` was NOT created. Production code untouched.

## Not run (CHƯA CHẠY)
- 87 of 99 mutants: from combo_layout 379:34 onward (incl. 408:42 `&&` -> `||`), all of trackbar_layout and tick_xs (the predecessor saw 5 of these caught with the same flags; not re-verified here). Likely-weak spots to try first when a quiet machine exists: `arrow_pen_width` comparison `>` (equivalent for scale >= 1, add a scale 0.5 case), `/ 2` in `cy`/`half / 2`.
- Native base: template, chrome, edit_centering, list_dialog pure functions: not run at all.
- Raw Win32/COM glue excluded by policy.
- Note from the coordinator (PR #516): the golden-failure cause is `CARGO_MANIFEST_DIR` baked into `repo_root()`; irrelevant to this golden-free binary.

## Suggested PARITY change (orchestrator)
X-chrome-painter-widgets: add "cl-mut-dialogs2 2026-10-31: 10/10 tested combo_layout mutants caught (non-golden, running 17 tests), 1 unviable, 87 of 99 not run; no full verdict." No status change.
