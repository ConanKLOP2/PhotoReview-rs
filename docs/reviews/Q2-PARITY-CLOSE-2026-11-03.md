# Q2 parity close, presenters cluster (2026-11-03 / 2026-11-04)

Status: evidence report of one run. Branch `parity/presenters` (worktree `.claude/worktrees/pc-presenters`), base `origin/master` 58aad090, C# baseline e7c2fcd8.
Rows: X-action-profiles, X-settings-field-map, X-secondary-presenters, X-recovery-presentation, X-scrollbar, X-compare-view-model, X-zoom-detail, F-17.
Nothing here edits `docs/PARITY.md`; the proposed status text per row is at the end of each section. **No row is proposed as Done** (reasons per row).

## How it was run, and the limits of this machine

- Build: `CARGO_TARGET_DIR=C:\Users\PC\Projects\target-pc-presenters`, `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`.
  The machine was saturated by other agents during the whole run (CPU 98 %, 95 cargo/rustc processes, 2.4 GB free RAM). One mutant cost 70 to 100 s
  (build) instead of 15 s, so about 50 mutants fit in 50 minutes per process. Numbers below are what that allowed; nothing is extrapolated.
- Mutation: `tools/dev/mutants.sh`, `--baseline skip --timeout 90` (200 for zoom_detail, see there), `-j 1`, own target dir per process under
  `C:\Users\PC\Projects\tmp-pcp-{A,B,C}`. Hand-written tests only: `--cargo-test-arg -- --cargo-test-arg --skip --cargo-test-arg golden_` was passed
  where the test binary has golden drivers, so a mutant counts as caught only when a non-golden test failed. Wall time used: about 50 min (two
  processes in parallel) + 15 min + 17 min + 8 min. That is above the "about 60 minutes" box by roughly 20 minutes of single-process time; it was
  spent on the zoom_detail hang mutants and on the file whose production code was changed.
- Formatting: `cargo fmt -p` was not needed; `tools/dev/pre-pr.sh` ran `fmt -p pr-app`, `fmt -p pr-platform-win` and passed, and each changed file was
  also checked with `rustfmt --edition 2024 --check`.
- Python for pre-pr: `PY_BIN=%LOCALAPPDATA%\Programs\Python\Python312\python.exe`.

## What changed in the repository

| Commit | Content |
|---|---|
| 75e2dca3 | `crates/pr-app/tests/action_list_json.rs`: ports of the three C# `ActionListJsonTests` (red: 3 failing tests that expose the defect below); `crates/pr-platform-win/tests/shell_viewing_scrollbar_native.rs`: two new native tests |
| f1a07f14 | production fix of `crates/pr-app/src/presenters/action_list_json.rs` (flagged as productionChanged) |
| 9d6aa01d, d26f6937 | killer for the `json_string` `'\r'` arm mutant (the first version had a wrong expectation, fixed by the second commit) |

### Defect found and fixed (small, C# behaviour clear): action-list `Operation` reader was not the C# strict reader

C# (e7c2fcd8) `ActionListJson.Deserialize`/`DeserializeOrNull` first run `ThrowIfUnknownOperation`, which finds every property named `Operation` ignoring case and
requires `LenientEnumConverter<FileOperationType>.TryReadStrict` to accept it: the text is trimmed, matched against names and aliases ignoring case, or parsed
with `int.TryParse` and accepted when it names a defined member; a JSON number must be a defined int32; anything else throws `JsonException`.
The Rust reader matched only the exact names Move/Copy/Recycle/Delete (no trim), JSON numbers 0..2, and only the property `Operation`. So:

- `"Operation": "2"` (ported C# row `Deserialize_KnownOperation_Parses`, `"\"2\""` to Recycle), `" Move "`, `"+1"` were rejected (Rust `Malformed`), C# accepts them;
- `"operation": "Teleport"` (any property case) was ignored by Rust, C# throws.

Found by porting `ActionListJsonTests` (test `deserialize_known_operation_parses` failed on the row `"2"`). Fix: `read_operation` uses `pr_core::model::serde_ext::{parse_lenient_text, lenient_from_number}`
(the existing C# `LenientEnumConverter` port) and the strict check loops over properties with `eq_ignore_case(key, "Operation")`. All 288 baseline tests of the cluster and the
pr-app goldens still pass (see below). Impact: only the Settings Actions row and the action-profiles importer; no journal or file behaviour.

## Runs of this session (all green unless stated)

| Run | Result |
|---|---|
| `cargo test -p pr-app --locked` with the 16 binaries of the cluster, base `origin/master` | 288 passed, 0 failed (action_list_json 17, action_profiles 25, action_profiles_io 8, presenters_recovery_golden 1, presenters_recovery_presentation 22, presenters_secondary 31, presenters_settings_form_state 5, settings_field_map 32, viewing_scrollbar_geometry 29, viewing_scrollbar_geometry_kills 5, viewmodels_compare 43, viewmodels_main_zoom_events 3, w8_presenter_killers2 2, w8_presenter_mutation_killers 15, w8_zoom_killers2 5, zoom_detail 45) |
| same after the fix and the new tests | action_list_json 24 (was 17), action_profiles 25, action_profiles_io 8, settings_field_map 32, settings_presenter_save 31, settings_presenter_apply 36: all passed |
| `cargo test -p pr-app --locked` (whole crate, after all changes) | 200 test binaries (incl. unit and doc), 3040 passed, 0 failed, 22 ignored (native, not run in this line) |
| `cargo test -p pr-platform-win --test dialogs_action_profiles_scenarios_native -- --include-ignored --test-threads=1` (users of the changed reader) | 7 passed |
| `cargo clippy -p pr-app --all-targets --locked -- -D warnings` | clean |
| `cargo clippy -p pr-platform-win --tests --locked -- -D warnings` | clean |
| `bash tools/dev/pre-pr.sh` (changed crates pr-app, pr-platform-win) | PASSED (fmt, clippy, unsafe-audit, doc-links, markers, BOM). NOT RUN there: `cargo test` (needs `--test`), workspace-wide clippy |
| `cargo test -p pr-platform-win --test shell_viewing_scrollbar_native -- --include-ignored --test-threads=1` | 7 passed (5 existing + 2 new native), real desktop |
| `cargo test -p pr-core --test io_physical_seams -- --include-ignored --test-threads=1` | 3 passed (includes `try_get_file_stat_physical_matches_the_recovery_check_goldens`, the 20 physical recovery goldens) |
| `cargo test -p pr-core --test file_actions_recovery_check -- --include-ignored --test-threads=1` | elevated session: 58 passed, 1 failed (`physical_access_denied_source_is_missing_and_lost_as_in_csharp`, its own precondition: with a High-integrity admin token the denied file's metadata is still readable). The same exe under `runas /trustlevel:0x20000` (basic user token): that test passed (1 passed). |

## Row X-action-profiles

Before: Ported (W8-10a, PR #47); Different (null list item rejected, I/O message carries the path). Evidence: goldens `action-list-json` 26, `action-profiles` 38, `action-profiles-io` 13.
Gaps before: (a) no mutation run of `action_list_json.rs` (46 mutants) or `action_profiles.rs` (53); (b) the three C# `ActionListJsonTests` were not ported ("every test here is new"); (c) unknown.

Done in this run:
- Ported the C# tests (`deserialize_unknown_operation_throws` 5 rows x 2 entry points, `deserialize_known_operation_parses` 6 rows, `deserialize_missing_operation_uses_default`); found and fixed the defect above (3 extra tests: trimmed/numeric text, blank/undefined/fractional values, property-name case).
- Mutation, `action_list_json.rs`, pre-fix source, non-golden tests only: 38 of 46 tested = 32 caught, 2 missed, 4 unviable, 0 timeout. Missed: `:121 delete match arm Some(2)` (the arm no longer exists after the fix) and `:154 delete match arm '\r' in json_string` (killed by the new `serialize_escapes_a_carriage_return_with_its_short_form_in_both_modes`, checked by deleting the arm: the test fails). 8 of 46 NOT RUN.
- Mutation after the fix, `-F 'read_operation' -F 'parse_actions' -F 'json_string'`: 26 selected, 16 tested = 15 caught, 1 unviable, 0 missed, 0 timeout; 10 NOT RUN.
- `action_profiles.rs`: 0 of 53 run (NOT RUN). Test binaries `action_profiles` 25 and `action_profiles_io` 8 pass.

Gaps after: mutation of `action_profiles.rs` (53) and the remaining 8 + 10 of `action_list_json.rs`; the Different notes still need their ADR (`ADR-<next id>` placeholder in PARITY-CLOSEOUT).
Proposed status: stays `Ported`; text: `Ported (W8-10a, PR #47); Different (null list item rejected, I/O message carries the path); Q2 2026-11-04: C# ActionListJsonTests ported (3 + 3 new), strict Operation reader fixed to match TryReadStrict/ThrowIfUnknownOperation, mutation of action_list_json.rs 38+16 tested, 47 caught, 0 survivors left (the '\r' arm killed), action_profiles.rs not mutated`.
Not Done because: action_profiles.rs has no mutation figure and the Different still lacks its ADR.

## Row X-settings-field-map

Before: Ported (W8-10b, PRs #47, #62); Different (undefined enum values); golden `settings-field-map` 617. Gaps before: no mutation run of `settings_field_map.rs` (249 mutants), none was ever recorded (CL-PRAPP covered the Settings presenter files only).
Done: `settings_field_map` 32 tests pass before and after the fix (the Actions row uses the changed reader and `actions_row_accepts_numeric_operation` still passes). C# diffs since d92d6aae (OpenInDefaultEditor/RenameFile shortcuts, ExternalEditors row) are already ported (rust lines 145-146, 878-890).
Mutation: 0 of 249 run (NOT RUN: the machine allowed about 50 mutants in the whole box and the file is golden-heavy; run it with `--skip golden_`).
Gaps after: the whole 249-mutant file.
Proposed status: stays `Ported`, unchanged text plus `Q2: tests re-run green after the action-list fix; mutation NOT RUN (249)`.

## Row X-secondary-presenters

Before: Ported (W8-10c, PR #52), hand-written (F). `settings_form_state.rs` 1 mutant, `secondary.rs` 56.
Done: `presenters_secondary` 31 and `presenters_settings_form_state` 5 pass. C# `SecondaryDialogPresenterTests`: all 19 test names have a same-named Rust twin (name search over `crates/pr-app/tests` and `crates/pr-platform-win/tests`). The 9 `ActionProfilesWindowTests` (WPF window level, row X-action-profiles) have no same-named test; their behaviour (clone, select/open, save, add, remove neighbour, remove last) is covered by the presenter tests `presenter_edits_a_copy`, `open_returns_fields_and_operation_index`, `save_current_writes_trimmed_fields_into_the_open_row`, `add_appends_move_with_next_free_key_and_localized_name`, `remove_selection_rules_clamp_and_signal_empty` (read, not mutated).
Mutation: 0 of 57 run (NOT RUN).
Proposed status: stays `Ported`; add `Q2: tests green, mutation NOT RUN (secondary.rs 56, settings_form_state.rs 1)`.

## Row X-recovery-presentation

Before: Ported (W8-10f, PR #50), golden `recovery-presentation` 48 (all 48 case names covered by the 22 F tests and the golden driver). `recovery_filter.rs` has 0 mutants, `recovery_presentation.rs` 99.
Done: `presenters_recovery_presentation` 22 and `presenters_recovery_golden` 1 pass. The C# `RecoveryPresenter*Tests` names that do not match by name (`PathStatusText`, `GroupStatusText`, `RetryConfirmText`, `NearestExistingFolder`, `VerdictBrush`, ...) are covered by golden case groups `path-status-all`, `group-status-*`, `retry-*`, `nearest-folder-*`, `verdict-tone-all` (names listed in the golden), not by same-named tests.
Mutation: 0 of 99 run (NOT RUN). The time-zone parts remain F tests behind `RecoveryTimeFormatter`.
Proposed status: stays `Ported`; add `Q2: tests green, mutation NOT RUN (99)`.

## Row X-scrollbar

Before: Ported (W8-16e, PR #52), goldens `scrollbar-geometry` 133 and `scrollbar-repeat-timing` 145. Gaps: no mutation run of `scrollbar_geometry.rs` (212 mutants) and nothing recorded for `scrollbar_native` natives beyond range checks.
Done:
- `viewing_scrollbar_geometry` 29 and `viewing_scrollbar_geometry_kills` 5 pass; all 14 C# `ScrollBarGeometryTests` names have a Rust twin.
- Native (this desktop, `--include-ignored`): `shell_viewing_scrollbar_native` 7 passed. Two new native tests, both checked by hand-mutating production code (restored afterwards): `track_mouse_leave_requests_a_mouse_leave_for_a_real_window` (a hidden `STATIC` window; witness = `TME_QUERY` reports `TME_LEAVE` or a `WM_MOUSELEAVE` is posted; fails when the `TrackMouseEvent` call is removed) and `repeat_timing_equals_the_formula_of_the_persisted_keyboard_settings` (reads `HKCU\Control Panel\Keyboard` through `reg.exe`; fails when `repeat_timing` ignores the system and returns `repeat_timing_for(0, 31)`). Earlier the native tests only asserted ranges / no panic.
- Mutation, `scrollbar_geometry.rs`, non-golden tests: 36 of 212 tested = 35 caught, 0 missed, 1 unviable, 0 timeout (lines 58-162: both `PartialEq` impls and the first compute branches); 176 NOT RUN.
Gaps after: 176 mutants of `scrollbar_geometry.rs`; the range-fallback branches of `repeat_timing` (`!delay_ok || !(0..=3)`, `!(0..=31)`) cannot be driven natively (the OS returns valid values); real mouse leave delivery to a visible, hovered window needs a person.
Proposed status: stays `Ported`; text adds `Q2: native 7/7 incl. 2 new witnesses, mutation scrollbar_geometry.rs 36 of 212 tested, 35 caught, 0 missed, 176 NOT RUN`.

## Row X-compare-view-model

Before: Ported (W8-06c, PR #59, mutation tests #76). The note "no formatter for the size pair yet" is stale: `CompareViewModel::size_text(ctx)` exists (`compare.rs:453`, `compare.sizePair`).
Done: `viewmodels_compare` 43 passes; all 24 C# `CompareViewModel(Mutation)Tests` names have a Rust twin. The only mutation record is PR #76 ("killed the 24 surviving mutants"), with no report file and no counts.
Mutation: 0 of 117 run (NOT RUN).
Proposed status: stays `Ported`; drop the stale "no formatter" clause; add `Q2: tests green, no mutation figure on record (117 mutants, NOT RUN)`.

## Row X-zoom-detail

Before: Ported (W8-07g, PR #60, race fix #72), plus the FIX-PANIC-REST note: only the `catch_job` unit test covers a panic inside the decode (no seam). Earlier mutation (W8-PRESENTER-MUTATION): 89 tested, 59 caught, 14 missed (killed afterwards), 7 unviable, 9 timeouts at 90 s counted as "detected" (the old rule). CL-MUT-ZOOMCORE: 10 re-run.
Done: `zoom_detail` 45, `w8_presenter_mutation_killers` 15, `w8_zoom_killers2` 5, `viewmodels_main_zoom_events` 3 pass; all 26 C# ZoomDetailLoader test names have a Rust twin (incl. the 10 `Retarget` ones).
Mutation re-run on the 9 old timeout mutants plus the `retarget_path` field mutants, with `--timeout 200` (the hang guard is 20 s per wait, so a hanging mutant fails the test after 20 s but exceeded 90 s overall): `CompletionInbox::{send, drain (2), len (2), is_empty (2)}`, `pending_load` (2), `queued_events` (2) and the 4 `retarget_path` field deletions: 15 tested = 14 caught, 1 unviable, 0 missed, 0 timeout; second run `-F 'ZoomDetailLoader::pump'`: 5 tested, 5 caught (includes `pump -> Default::default()`). So the 9 former timeouts are now real non-golden kills (the old "detected by timeout" wording is replaced by caught).
Gaps after: the other ~65 of 90 mutants (the `refresh`, `update`, `on_original_decoded`, `on_indicator_elapsed` bodies) have no run under the correct rule on the current source (CL-MUT-ZOOMCORE ran 10); the `catch_job` mutant `-> Default` is unviable and the panic inside `decode_original` still has no seam (the service runs the decode in its own worker, so a fake decoder cannot make the loader's closure panic).
Proposed status: stays `Ported`; text adds `Q2: 20 mutants re-run with --timeout 200 (9 former hang mutants now caught), 0 missed; ~65 not re-run`.

## Row F-17

Before: Ported (W3-07); evidence: `file_actions_recovery_check::*`, goldens `recovery-decide` 720 and `recovery-check` virtual 712 of 715 (1 `rust_skip` + 2 `dir_throws` skipped), native physical 20/20; mutation in W3-05-UNDO-MUTATION: `recovery_check.rs` 83 of 83 = 72 caught, 3 missed (all explained equivalent), 8 unviable, 0 timeouts, on a source that has not changed since (`git log` for the file ends at #282, 2026-10-27, before that run).
Gaps before: (a) native physical pass not re-run in a recent session; (b) mutation figure from 2026-10-29 included golden tests among the killers; (c) the 2 `dir_throws` goldens cannot be reproduced.
Done:
- Native physical: see the run table (3/3 `io_physical_seams`; access-denied case passes under a normal user token, fails its precondition under an elevated token, which is a property of the test environment and is why it asserts the precondition first).
- Mutation re-run on the unchanged source (count again 83): 32 of 83 tested = 27 caught, 5 unviable, 0 missed, 0 timeouts. This run did NOT skip the golden drivers (the cap hit before I could repeat it), so it does not by itself prove non-golden kills; 51 NOT RUN.
- The 2 `dir_throws` goldens: `directory_exists` returns `bool` in the Rust seam. In production C# `Directory.Exists` never throws (it returns false), only a fake can, so the case is not reachable outside a fake. Proposal: record it as a Different/accepted gap in the row.
Gaps after: non-golden proof for the 72 caught (needs a rerun with `--skip golden_`: 83 mutants), the 3 equivalent survivors rest on the written W3-05 reasoning.
Proposed status: stays `Ported`; add `Q2 2026-11-04: native physical 3/3 and access-denied pass under a non-elevated token (fails the precondition when elevated), mutation re-run 32 of 83 tested, 27 caught, 5 unviable, 0 missed (goldens included), 51 NOT RUN`. It is the closest row to Done: one mutation run of 83 with `--skip golden_` on a quiet machine (about 25 minutes when idle) plus the ADR/Different note for `dir_throws` would let the lead promote it.

## Proposals (no production change beyond the one defect fix)

1. Record `dir_throws` (F-17) as an accepted Different: `Directory.Exists` cannot throw outside fakes.
2. Run the NOT RUN mutation (listed per row) in one quiet window; the commands are `bash tools/dev/mutants.sh pr-app <file> --tests <binaries> --cargo-test-arg -- --cargo-test-arg --skip --cargo-test-arg golden_ --budget 3600 --jobs 1`.
3. A seam for a panicking `decode_original` in `PreviewImageService` would close the last X-zoom-detail gap from FIX-PANIC-REST.
