Status: Partial. 20 selected mutants evaluated: 19 caught, 0 missed, 1 source-unviable, 0 timeouts. No production changes.

# CX-SETTINGSWIN2: settings window mutation follow-up (2026-10-31)

Lane `feat/cx-settingswin2`, starting HEAD `1ba2dacf77308c501cc6d5b003b7b6c97bb0017a`.
Continues [CX-SETTINGSWIN](CX-SETTINGSWIN-2026-10-31.md), which has only five valid caught verdicts out of 400 listed mutants (host 80, frame 190, surface 130).
No staging, commit, push, PARITY or task status edits. No production or unsafe edits; no real user configuration, C# data, Shell launching or Recycle Bin operations.

## Test scope

New binary `crates/pr-platform-win/tests/w8_settingswin2_killers.rs` contains five independent inline tests. No golden files or existing golden-containing test modules are imported. Contracts cover page IDs (including negative and wrapping indices), independent count clamp and item-height fallback, rectangle coordinates and size clamps, note/warning precedence, and HTTP URL length/scheme/injection/Unicode boundaries. All functions are pure; no files, windows, timers or polling are needed.

Read C# baseline `7eaee2a2`, `src/PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsDialog.cs` through read-only Git with a command-local safe.directory exception. `ViewportId`, `PageIdBase`, `StaticTextColor`, `PageSurface.ComboDropHeight` and placement cache decisions agree with these ordinary cases. Saturating/wrapping extremes pin existing Rust contracts; URL validation is documented Rust-only hardening.

The actual frame geometry arithmetic lives in `dialog/footer.rs` (previously audited separately), outside the three source files assigned to this lane. This lane prioritises page ID arithmetic and pure surface layout helpers; raw HWND/Win32/COM, native measurement, PageSurface side effects and frame forwarding are excluded.

## Reproduction

Both profiles use debug=0, incremental=0, Cargo build jobs=2. Normal target is this worktree's `target`, mutation target is `target-mut`. Isolation is `PHOTOREVIEW_RS_DATA_ROOT=<worktree>/target/settingswin2-isolated`, `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`.

Preflight (900-second guard) passed: **running 5 tests; 5 passed, 0 failed, 0 ignored; exit 0**.

```text
cargo test -p pr-platform-win --locked --test w8_settingswin2_killers -- --include-ignored --test-threads=1
```

Cargo-mutants 27.1.0 generated 20 selected mutants: host 11, frame 1, surface 8. The frame mutant is the unfinished `page_id_base -> -1`, avoiding repeats of the two already caught returns. Surface includes four combo mutations, one static-text mutation and three placement mutations (None, zero rectangle and inequality inversion).

The initial broad function regex listed 111 mutants, including 83 placement tuple variants and five raw ShellExecute field deletions. An identity diff restricts eligible lines to host 60..69, surface 25..56 and frame 98; it changes no production file. Combining it with the regex yields exactly the 20 pure mutants above. Both the inventory and actual runner verified this selection before building.

```text
cargo mutants -p pr-platform-win --baseline skip --timeout 300 -j 1 --file "**/settings/dialog/surface.rs" --file "**/settings/dialog/frame.rs" --file "**/settings/dialog/host.rs" --in-diff <worktree>/target/settingswin2-evidence/pure.diff -F 'page_id_base|is_web_url|static_text_kind|replace.combo_drop_height|in.combo_drop_height$|with.None$|with.Some\(\(0,.0,.0,.0\)\)$|replace.!=.with.==.in.placement_change' -E 'page_id_base.*with.[01]$' --output <worktree>/mut-out-settingswin2-pure --cargo-test-arg=--test --cargo-test-arg=w8_settingswin2_killers --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1
```

The initial run caught `page_id_base -> -1`. Its log showed `running 5 tests`, with the page-ID assertion failing (4 passed, 1 failed). The default build phase compiled every test binary, taking 314 seconds for this first mutant. The following `is_web_url -> true` build was interrupted before a verdict so the remaining budget could be spent on the intended binary. Its incomplete build is neither caught nor unviable.

`taskkill /PID 18096 /T /F` was refused by the OS with `Access denied`; no foreign process was targeted. The owned execution session was interrupted through its session handle and exited 1. The launcher PID was absent afterwards, the raw log stopped growing, and an exclusive nonblocking byte lock on its lock.json succeeded, establishing no live mutation owner before the next run. CIM process ancestry inspection was denied and was not used as evidence. The initial logs were copied to `target/settingswin2-evidence/first-run/`.

The corrected run excludes `page_id_base` entirely (19 selected: host 11, surface 8) and adds `--cargo-arg=--test=w8_settingswin2_killers` to the command above; it writes `mut-out-settingswin2-fast`. This retains the required `--cargo-test-arg=--test --cargo-test-arg=w8_settingswin2_killers --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`. An exact unmutated preflight with both Cargo target-selection arguments passed **5/5, exit 0** before starting it. The first corrected mutant's log confirms both build and test select only this binary and that five tests actually run.

Only one owned cargo-mutants process ran at a time. Initial runner guard: 2550 seconds; corrected runner guard: 1800 seconds. Both sessions were additionally controllable through their owned execution handles. The corrected runner finished **exit 0 in 870.85 seconds**, including source copying. Raw runner, selection, preflight and validation logs are under `target/settingswin2-evidence/`.

## Final measured outcome

| File | Listed inventory | Evaluated this lane | Caught | Missed | Source-unviable | Timeout |
|---|---:|---:|---:|---:|---:|---:|
| host.rs | 80 | 11 | 11 | 0 | 0 | 0 |
| frame.rs | 190 | 1 | 1 | 0 | 0 | 0 |
| surface.rs | 130 | 8 | 7 | 0 | 1 | 0 |
| Total | 400 | **20** | **19** | **0** | **1** | **0** |

The initial run completed one Caught outcome; its second build was interrupted without a raw outcome. The corrected run completed 19 outcomes (18 Caught, 1 Unviable). There are **20 unique completed outcomes** across the two runs and no repeated completed verdicts. The interrupted host return-true mutant was caught in the corrected run. No infrastructure failure or interrupted build is counted caught or source-unviable. Timeout would count as NOT detected; none occurred.

**19 mutants actually ran the five-test binary and failed a non-golden assertion.** Every caught log was checked for `running 5 tests`, `test result: FAILED` and the failing test names. Golden-only catches: **0**. The frame failure is `page_id_base_negative_and_wrapping_indices_preserve_contract`; host failures are `is_web_url_requires_nonempty_http_authority_and_rejects_injection`; surface failures are the combo or placement test named in this binary. The first log was opened directly to verify the command and assertion. No missed mutant needed a repair or a subsequent killers-only rerun: both runs already select the new killers binary alone for test execution. No equivalence is asserted and no production bug was found.

The source-unviable mutant is `surface.rs:25:5 replace static_text_kind -> StaticTextKind with Default::default()`. Its compiler log gives **E0277**, because `StaticTextKind` does not implement `Default`. This is a source-level invalid mutant, distinct from the previous lane's infrastructure failures. It is **not** counted among the 19 behavioural detections.

Combining the previous lane's five caught frame mutants with this lane gives **24 unique valid caught test verdicts / 400**, plus one source-unviable mutant. Exactly **375** mutants have no completed source/test verdict across these two lanes; **376** lack a behavioural test verdict (including the source-unviable mutant). This lane alone leaves 380 inventory entries unevaluated. No full-window mutation claim is made.

NOT RUN here: host's other 69 mutants (weak adapters, delegation and raw ShellExecute fields); frame's other 189 mutants (five have historical caught evidence); surface's other 122 mutants, including 80 additional tuple-return variants of placement_change and 42 native/surface/forwarding mutants. Native Win32/COM/GDI/ScrollInfo glue is intentionally skipped. Frame geometry in footer.rs was not re-mutated. Attended DPI/GDI/language scenarios, C# runtime page-type labels, full forwarding and posted-action panic end-to-end behaviour remain unverified.

Final gates: `cargo fmt -p pr-platform-win --check` **exit 0**; guarded locked killers run with `--include-ignored --test-threads=1` **5 passed, 0 failed, 0 ignored, exit 0**. Workspace tests, Clippy, native tests and additional mutation were NOT RUN in this lane. `python -I tools/dev/doc-links.py --quiet` passed **exit 0: files=621, errors=0, proposed=180, archive_unchecked=42, csharp_refs=507, runtime_paths=13, placeholders=111**. PROPOSED items are distinct from errors. Final Git scope check confirms only the new killers and this evidence file; production, PARITY and task_on_progress have no diff.

## Cleanup blocker

Both owned execution sessions have exited. The initial output was archived under `target/settingswin2-evidence/first-run`; the corrected output is retained in its raw output directory and copied separately to `target/settingswin2-evidence/fast-run`. `verified-outcomes.json` records every completed verdict and failing non-golden test. Runner commands, identity diff, inventories, preflight/final logs and the corrected exit/elapsed-time record are retained in the same evidence directory.

Automatic approval review rejected the combined lock-check/archive/cleanup command **before execution**, reporting **blocked by policy**. Its planned deletion was restricted to root-validated absolute paths within this worktree. No alternative deletion command was attempted. Required cleanup remains **BLOCKED** for this worktree's `target-mut`, `mut-out-settingswin2-pure` and `mut-out-settingswin2-fast`; the orchestrator must remove them after preserving logs. This is not a successful cleanup claim. No process belonging to another lane was killed. The stock janitor was inspected: it lacks a selector for mutation outputs/current-worktree targets, and was not applied to unrelated worktrees or global temp data.

## Proposed PARITY note for orchestrator

Keep `X-settings-ui-window` **Partial**. Add this killers binary and the measured counts above: 19 new caught (host 11, frame 1, surface 7), one source-unviable, no missed or timeouts; cumulative 24 valid caught / 400, plus one source-unviable. Retain native/DPI/GDI/language/runtime-label/ScrollInfo/posted-action panic gaps. Do not promote selected mutation evidence to full-window parity. The prior report's **395** lacking a valid verdict is the correct starting number (the row's historical 394 refers to never reaching any raw outcome).
