Status: Partial. 21 selected mutants evaluated: 15 caught, 5 equivalent MISSED, 1 source-unviable, 0 timeouts. No production changes.

# CX-SETTINGSWIN3: settings declaration and detached host (2026-10-31)

Lane `feat/cx-settingswin3`, starting HEAD `cd40b7c26ed55b07ba6801d203834f3ac1daf22b`.
Continues [CX-SETTINGSWIN2](CX-SETTINGSWIN2-2026-10-31.md).
No staging, commit, push, production, unsafe, PARITY or task status edits.

## Scope and contracts

New binary `crates/pr-platform-win/tests/w8_settingswin3_killers.rs` has two inline tests, with inert Shell integration and warning fakes. It does not import existing tests or golden files. Constructing a SettingsDialog builds declarations but never opens a window (asserted HWND zero). A second test uses an empty Weak reference, so no host operation can reach native code. No config, filesystem or Recycle Bin action is part of either test.

Read C# baseline `7eaee2a2:src/PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsDialog.cs`, `BuildItems`, through read-only Git. The test checks all 19 controls in order, class, id, exact style/ex-style and the supplied version text. Host detached values are the documented Rust lifecycle contract D-W8B5-705.

Inventory from `cargo mutants --list`: **400** entries (frame 190, host 80, surface 130). Selection is **21** entries (frame 12, host 9). `build_items` includes five bitwise-AND changes, five bitwise-XOR changes, the default-button comparison and a Default return. Host selection includes combo index 0/1 and unary-minus deletion, four nonempty shortcut-map returns and two nonempty action-JSON returns. Neutral return substitutions are excluded because detached-only coverage cannot distinguish them from a live host.

The selection uses a non-mutating identity diff (frame lines 462..503 and host lines 202, 237, 280, 285), plus a function regex. No source is altered to select mutants. All five functions forbidden for repeat runs are excluded. Remaining surface entries are native layout adapters or the previously evaluated static_text_kind mutant; no surface mutation is selected here.

## Reproduction and preflight

Environment: `CARGO_TARGET_DIR=<worktree>/target-mut`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=2`, `PHOTOREVIEW_GOLDEN_ROOT=<worktree>`, `PHOTOREVIEW_RS_DATA_ROOT=<worktree>/target/settingswin3-isolated`, `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`.

Preflight (900-second guard) completed **running 2 tests; 2 passed, 0 failed, 0 ignored; exit 0**, after a 3m18s initial compile:

```text
cargo test -p pr-platform-win --locked --test w8_settingswin3_killers -- --include-ignored --test-threads=1
```

All `.rs` files under this crate's src and tests were touched before the mutation run. Only one cargo-mutants process ran at a time. Runner had a 2700-second guard; every mutant had a 300-second timeout.

```text
cargo mutants -p pr-platform-win --baseline skip --timeout 300 -j 1 --file "**/settings/dialog/frame.rs" --file "**/settings/dialog/host.rs" --in-diff <worktree>/target/settingswin3-evidence/pure.diff -F 'build_items|combo_selected_index|current_shortcut_texts|actions_json' -E 'with.-1$|with.BTreeMap::new|with.String::new' --output <worktree>/mut-out-settingswin3-pure --cargo-arg=--test=w8_settingswin3_killers --cargo-test-arg=--test --cargo-test-arg=w8_settingswin3_killers --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1
```

The extra `--cargo-arg` restricts the build phase to the same binary; test execution also selects only this binary. Raw inventories, identity diff, exact argument array, preflight and runner logs are under `target/settingswin3-evidence/`.

## Final measured results

| File | Inventory | Evaluated here | Caught | MISSED (equivalent) | Source-unviable | Timeout | NOT RUN here |
|---|---:|---:|---:|---:|---:|---:|---:|
| frame.rs | 190 | 12 | 6 | 5 | 1 | 0 | 178 |
| host.rs | 80 | 9 | 9 | 0 | 0 | 0 | 71 |
| surface.rs | 130 | 0 | 0 | 0 | 0 | 0 | 130 |
| Total | **400** | **21** | **15** | **5** | **1** | **0** | **379** |

Runner completed in **710.08 seconds**. Cargo-mutants exit **2** reflects the five MISSED equivalents. Every selected mutant received a completed outcome; no interrupted build or infrastructure failure is counted.

The first completed mutant, frame.rs:462:9 `build_items -> Default::default()`, is source-unviable: **E0277**, `Declaration` does not implement `Default`. The first compiled mutant, frame.rs:464:46 `| -> &`, was opened directly: the exact binary command includes `--include-ignored --test-threads=1`, `running 2 tests`, the declaration assertion failing, and **1 passed / 1 failed**. Its changed text style became zero, independently of golden files.

All **20** compiled-mutant logs were checked for `running 2 tests` and the appropriate final test result; each of the **15** caught logs names a failed inline test from this new binary. Caught-by-golden-only: **0**. A recursive scan of every mutant log for `no tests/golden above` found **0** matches. Raw outcomes and log assertions are recorded in `target/settingswin3-evidence/verified-outcomes.json`.

The six frame catches are all five `| -> &` substitutions and `== -> !=` at line 502. The nine host catches are the three combo-index changes at line 202, four nonempty-map replacements at line 237 and the nonempty strings at lines 280/285. They were run against the killers binary alone from the outset. No non-equivalent MISSED needed an additional killer or rerun; the five MISSED below are equivalent. No real production bug was exposed and no production fix was made.

Compared with SETTINGSWIN2's cumulative **24 caught + 1 source-unviable**, this adds 21 unique outcomes with no repeated completed mutant. Cumulative across SETTINGSWIN/2/3: **46/400 completed outcomes = 39 caught, 5 equivalent MISSED, 2 source-unviable, 0 timeouts**. Exactly **354** entries still have no completed outcome; **356** lack a behavioural test verdict if the two source-unviable entries are included. This is selected mutation evidence, not full-window verification.

NOT RUN in this lane: 379 inventory entries as split above, including all surface mutations, raw ShellExecute/Win32/COM/GDI/ScrollInfo glue, live-HWND host delegation, native measurement, initialization/teardown/message forwarding and remaining public frame adapters. The five explicitly excluded functions were never selected. Native/DPI/GDI/language/posted-action panic scenarios and C# runtime page-type labels remain unverified. Workspace tests, Clippy and native tests were NOT RUN.

Final locked killers run (900-second guard): **running 2 tests; 2 passed, 0 failed, 0 ignored; exit 0**. Formatting after normalization: `cargo fmt -p pr-platform-win --check` **exit 0**. `python -I tools/dev/doc-links.py --quiet`: **exit 0, files=624, errors=0, proposed=181, archive_unchecked=42, csharp_refs=507, runtime_paths=13, placeholders=112**. PROPOSED items are distinct from errors. The documentation index already covers the reviews folder; it was not edited.

## Bitwise equivalence rationale

The five `| -> ^` changes in `build_items` operate on fixed, disjoint masks. For operands A and B with `A & B == 0`, `A | B == A ^ B` for every execution. These expressions have no input-dependent masks:

| Location | Operands at the changed operator | Intersection |
|---|---|---:|
| frame.rs:464:46 | `0x50000000` and `0x80` | 0 |
| frame.rs:484:26 | `0x50000080` and `0x02000000` | 0 |
| frame.rs:484:40 | `0x52000080` and `0x04000000` | 0 |
| frame.rs:502:36 | `0x50000000` and `0x00010000` | 0 |
| frame.rs:502:42 | `0x50010000` and the conditional `0` or `1` | 0 |

Consequently those substitutions cannot be killed by a behavioural test; a MISSED verdict for one is reported as MISSED plus this equivalence explanation, never as caught. The declaration test still checks the exact resulting styles. No production change is warranted for these equivalents.

## Cleanup blocker and evidence retention

The mutation runner and final test session have exited. Raw `mutants.out` was copied to `target/settingswin3-evidence/raw-mutants.out` before cleanup; its outcomes.json contains all 21 outcomes. Inventories, selection diff, exact command array, verified-outcomes.json, logs and runner exit/elapsed time remain under the same evidence directory. The stock janitor was inspected; it has no selector for this current-worktree mutation output and excludes the current worktree's target from its target cleanup. It was not applied to other lanes.

Both deletion targets were resolved to absolute paths and verified inside this worktree. Automatic approval review rejected the following command **before execution**, reporting **blocked by policy**:

```powershell
Remove-Item -LiteralPath 'C:\MyProjects\PhotoReview-rs\.claude\worktrees\cx-settingswin3\target-mut' -Recurse -Force
Remove-Item -LiteralPath 'C:\MyProjects\PhotoReview-rs\.claude\worktrees\cx-settingswin3\mut-out-settingswin3-pure' -Recurse -Force
```

No alternative deletion was attempted. Required cleanup is **BLOCKED**, not complete; orchestrator must remove these two directories. No other lane's process or artifact was touched. Git status shows only the new killers and this report; production, PARITY and task_on_progress have no diff. No Git staging/commit/push was run.

## Proposed PARITY note for orchestrator

Keep `X-settings-ui-window` **Partial**. Add `w8_settingswin3_killers`: frame 6 caught / 5 equivalent MISSED / 1 source-unviable; host 9 caught; surface not run. Cumulative 39 caught, 5 equivalent MISSED, 2 source-unviable / 400, with 354 entries still without a completed outcome. Preserve native/DPI/GDI/language/ScrollInfo/posted-action panic and C# runtime page-type label gaps; selected declaration and detached-host mutation does not establish full-window parity. PARITY and task_on_progress were not edited by this lane.
