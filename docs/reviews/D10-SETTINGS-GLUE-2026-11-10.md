# D-10 / C-28 Settings window glue: time-boxed mutation run (2026-11-10)

Owner decision 4C ("try"): mutate the Settings window builder and glue too. Branch `test/d10-settings-glue`. Machine: 12 logical CPUs, about 23 GB RAM, shared
with many other agent builds (50 `rustc` at once; one lane even hit `fork: Resource temporarily unavailable`). Every mutant cost 70 to 180 s of build, so the
whole run reached 1.4% of the 846 identities. This is a PARTIAL result, not a mutation verdict for any file.

Paths: the task text named `crates/pr-app/src/presenters/settings`; that directory (apply, load, shortcuts, validate) was closed by CL-PRAPP. The files in question live
in `crates/pr-platform-win/src/dialogs/settings/{builder/{groups,rows,inputs}.rs, context.rs, dialog/{host,frame,surface}.rs}`.

## 1. Flaky GUI-count test (fixed first)

`settings_dialog_frame_e2e::opening_and_closing_does_not_leak_gdi_or_user_objects` compared ONE sample after 5 cycles with ONE sample before (`after <= before`).
Q5 saw the process USER count oscillate 4/5 without growing (one lazily freed object), so the single-sample compare failed on the high phase (4 of 4 runs there).
Here it did not reproduce (stable 7, 20 cycles, 3 full binary runs plus 3 single runs green), so the cause is the single-sample assertion, not a leak.
Fix: sample after each of 8 cycles and fail only when the series keeps growing (`keeps_growing`: lowest value of the second half above the highest of the first half; a real
leak adds at least one object per cycle). The pure helper has its own non-ignored test (`keeps_growing_ignores_a_plateau_and_a_one_object_oscillation_but_sees_a_leak`,
covers plateau, 4/5 oscillation, decreasing, growing, step growth). Result: `settings_dialog_frame_e2e` 12 passed with `--include-ignored --test-threads=1`, twice.

## 2. Mutation run

Command shape (per lane, own `TEMP` so own target dir, `-j 1`, `--baseline skip`, `tools/dev/mutants.sh`, preflight proved `running N tests`, N > 0):
pure lanes used the non-ignored binaries (223 tests for builder, 106 for context); the native lane used `--native` (`--include-ignored --test-threads=1`, 41 tests,
desktop session available, GUI tests ran unattended). Budgets: 1500 s (A, B, C), 1200 s (A2, B2, native files), then small reruns.

| File | Identities | Tested | Caught | Missed | Unviable | Timeouts |
|---|---|---|---|---|---|---|
| builder/groups.rs + rows.rs | 153 | 16 | 8 | 0 | 8 | 0 |
| builder/inputs.rs | 199 | 18 | 8 | 0 | 10 | 0 |
| context.rs | 94 | 13 | 4 | 1 (killed, below) | 8 | 0 |
| dialog/host.rs | 80 | 7 (+ reruns) | 0 | 2 (killed, below) | 5 (see caveat) | 0 |
| dialog/surface.rs | 130 | 8 | 5 | 0 | 3 | 0 |
| dialog/frame.rs | 190 | 17 | 10 | 4 (killed, below) | 3 | 0 |
| Total | 846 | 79 | 35 | 7 | 37 | 0 |

NOT RUN: 767 of 846 identities (builder 119 of the 153 groups/rows and 171 of 199 inputs; context 81; host 73 (partly rerun); surface 122; frame 173). The tested ones are the
first identities in source order that each lane reached inside its time box (A2/B2 skipped the already tested ones with an exclusion regex). Part of the earlier Q5 and CL-DLG
runs (groups/rows 21, surface 19, 40 more) overlaps and is not added here.

## 3. Survivors

Killed by `crates/pr-platform-win/tests/d10_settings_glue_killers.rs` (3 tests, pure, no window, no browser):
- `context.rs:82:9` `show_external_editors -> Ok(None)`: the trait default must say "unavailable" (`Err`), not "cancelled". Rerun: caught.
- `host.rs:34:5` `default_open_url -> Ok(())` and `host.rs:41:8` `delete !`: empty and non-web URLs must be rejected with their reasons. Rerun: both caught.
- `frame.rs:149:9`, `171:9`, `190:9`, `365:9` (`fmt -> Ok(Default::default())` for Debug of options, args, content and Display of `SettingsDialogError`): assert the written text. Rerun: 4 of 4 caught.

Argued, not killed: `host.rs:47..51` (`delete field cbSize/fMask/lpVerb/lpFile/nShow` of `SHELLEXECUTEINFOW` in `default_open_url`). They only differ on a VALID http(s) URL, where the code
calls the real `ShellExecuteExW` and starts the user's browser; a test would open a browser window. Rerun with the killers: 47, 48, 49, 50 survive (not killed), 51 not seen in time.
These stay open; they are COM/Shell glue in the same class as the P-403 exclusions, but this note does not add them to that policy.

Caveat on unviable: in the native host lane `host.rs:47..51` were reported unviable, but in the later rerun the same identities compiled and survived. The host lane ran under
heavy machine load (fork failures were seen in one lane), so an "unviable" from this run is not independently verified. The unviable counts above are as reported by cargo-mutants
and were NOT re-proved by a clean rebuild.

## 4. Rule B and the two rows (for the lead to decide; PARITY.md not edited)

Rule B needs every identity caught, unviable or proved by a whole-domain run. 767 of 846 identities of the named files were not run, so:
- D-10: does NOT meet rule B. It stays Partial. The honest counts to put in its text are the table above.
- C-28: its open part is the `phase_two` step 6 window glue (hand-mutant only, no cargo-mutants, policy-excluded) plus the attended real-exe run; nothing in this run changes it.
  Whether a hand mutant is enough is the owner's call; this PR gives no new evidence for it.
To close D-10 the remaining identities need an idle machine (about 70 s each at `-j 1`: roughly 15 hours for 767) or shards of one file per night.

## 5. Final checks

`cargo fmt -p pr-platform-win --check` ok; `cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings` ok;
`settings_dialog_frame_e2e` 12 passed native; `d10_settings_glue_killers` 3 passed. NOT RUN: `cargo test --workspace`, `cargo fmt --all --check` (os error 206 here).
