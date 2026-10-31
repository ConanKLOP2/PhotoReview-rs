# CX-INPUTMUT 2026-10-31: input mutation follow-up

Lane: `feat/cx-inputmut`, base `9f85ce288dc46e38ac0de0134ba0f7a76ca235e4`.
Scope: `pr-platform-win` input layer. No production changes were needed. No
commits, pushes, PARITY edits or task status edits were made. Proposed row
updates belong in this report for the orchestrator's later sync. The more
specific instruction to leave those shared files untouched takes precedence
over the generic status/PARITY update instructions in the task template.

## Reconciled starting evidence

Read [CL-INPUT](CL-INPUT-2026-10-30.md),
[CL-INPUT2](CL-INPUT2-2026-10-30.md), and
[CL-INPUTDONE](CL-INPUTDONE-2026-10-30.md) before selecting work.
No tracked `CX-INPUT*` predecessor exists at this base.

| Row | Current source listing | Gap and disposition |
|---|---:|---|
| X-input-native | `shell_input/native.rs`: 92; `environment.rs`: 1 | CL-INPUTDONE ran 41 OS-read mutants and 17/51 pure helpers. Prioritize remaining pure shards. Precision touchpad branch remains hardware-gated. Environment's single mutant is Debug formatting, outside the 92-mutant native ledger. |
| X-input-message-handlers | `shell_input/handlers.rs`: 42 | CL-INPUTDONE shard 1/3 covered 14. The other valid shards, 0/3 and 2/3, contain 28 entries before accounting for any overlap with the older 13-entry run. The historical "15 remaining" is not an identity-based union. |
| X-input-shell-input | `shell_input/dispatch.rs`: 196 | The old 61-entry debt is superseded: CL-INPUT covered 22 and CL-INPUT2 covered 60 + 113 = 173 (their listings differ by one). Do not claim a complete 196-entry identity ledger from aggregate counts. No repeat planned within this time-box; real OS-window golden replay and RAW startup remain unverified. |
| X-input-main-window | `shell_input/main_window.rs`: 98 | Old 70-entry debt superseded by CL-INPUT2 whole-file run: 98 evaluated, 78 caught, 20 unviable. Full production startup remains outside that evidence. |
| X-input-viewport-surface | `shell_input/surface.rs`: 126 | CL-INPUTDONE shard 1/6 covered 21. The other valid shards, 0/6 and 2/6 through 5/6, contain 105 entries before accounting for the older 13-entry run. Forwarding is lower priority than branching logic. |

The source files above contain no inline unit tests (`#[test]` / `#[cfg(test)]`
search); a second mutation run with `--cargo-arg=--lib` is therefore inapplicable.
Raw Win32/COM glue is excluded from new mutation runs.

## Environment and commands

Normal builds: `CARGO_TARGET_DIR=<worktree>/target`. Mutations:
`CARGO_TARGET_DIR=<worktree>/target-mut`, `-j 1`, own `mut-out-<group>`.
Both use `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`,
`CARGO_INCREMENTAL=0`, `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`.
Mutation builds use `CARGO_BUILD_JOBS=2` and `--build-timeout 600`.
No normal Cargo test runs in `target-mut`.

Inventory command:

```text
cargo mutants -p pr-platform-win --list --file **/shell_input/native.rs --file **/shell_input/environment.rs --file **/shell_input/handlers.rs --file **/shell_input/surface.rs --file **/shell_input/dispatch.rs --file **/shell_input/main_window.rs
```

Native baseline:

```text
cargo test -p pr-platform-win --locked --test shell_input_basics -- --include-ignored --test-threads=1
```

Result: 46 passed, 0 failed, 0 ignored, including the 7 native tests.
No decoder DLL staging was needed by this binary. This is local native evidence,
not attended touchpad or full application startup evidence.

## Sharding correction

This cargo-mutants installation numbers shards from **zero**. An explicit
listing probe with `--shard 3/3` exited 1 with `shard k must be less than n`.
Valid thirds are 0/3, 1/3, 2/3; valid sixths are 0/6 through 5/6. Therefore the
prior command `--shard 1/3` denotes the middle third, not the first third.
The new 2/3 helper listing has 17 entries, starting at `signed_high_word`'s XOR
operator and ending at `is_key_repeat`'s XOR operator. A separate 0/3 listing
has 17 entries, starting at `modifiers_from_key_states`'s default return and
ending at its `|| -> &&` operator. It is not run in this time-box.

## New killers

`crates/pr-platform-win/tests/w8_inputmut_killers.rs`: two independent pure
probes. One checks signed/unsigned word boundaries with independently varied
high/low words and upper-bit noise (108 packed values); one checks all four
extended/repeat flag combinations against five unrelated-bit patterns (20
packed values). Neither test uses native APIs, user files, clocks or sleeps.
The tests name the guarded mutant locations/operators in comments. Oracle read
at `7eaee2a2:src/PhotoReview.Shell.Win32/Input/InputNative.cs` confirmed the masks
and bit constants (read-only, Git safe.directory scoped to this command; no
global config change). Baseline:
2 passed, 0 failed, 0 ignored. These are additional independent probes; no new
MISSED mutant or production bug was observed in this lane.

## Completed mutation run

Cargo-mutants 27.1.0, 2026-10-31, local Windows session. Main run exited 0:
**17 listed, 17 tested, 17 caught, 0 missed, 0 unviable, 0 timeouts**, about
21 minutes including its cold first build. This is **17/51 pure helpers** or
**17/92 native.rs mutants**, not a full-file verdict. The other 34 pure-helper
entries were not run in this lane (17 were previously documented in shard 1/3;
the new listing of shard 0/3 confirms another 17 not run here).

```text
cargo mutants -p pr-platform-win --baseline skip --timeout 90 --build-timeout 600 -j 1 --no-shuffle --file **/shell_input/native.rs -F modifiers_from_key_states|signed_.*word|low_word|is_extended|is_key_repeat --shard 2/3 --cargo-arg=--test=shell_input_basics --output <worktree>/mut-out-native2
```

The filter is one quoted argument when invoking the shell. The 17-entry `--list
--json` used exactly the same file/filter/shard. Mutation tests intentionally
omit ignored native tests because only pure helpers are mutated; the native
tests were run separately in the unmutated baseline. Actual mutant logs are
checked for `test result: FAILED`, not just cargo-mutants' aggregate count.

## Covering tests and gates

All test commands used `--locked`, target `<worktree>/target`, isolated config,
and `--test-threads=1`. Existing binaries used `--include-ignored`.

| Binary | Passed | Failed | Ignored |
|---|---:|---:|---:|
| shell_input_basics | 46 | 0 | 0 |
| shell_input_dispatch | 18 | 0 | 0 |
| shell_input_dispatch_keys | 22 | 0 | 0 |
| shell_input_dispatch_pointer | 32 | 0 | 0 |
| shell_input_surface | 29 | 0 | 0 |
| w8_inputmut_killers | 2 | 0 | 0 |

Total: 149 passing tests in six binaries. This includes the 7 ignored/native
basics tests and the hidden-window
`native::shell_input_registers_three_handlers_in_order_on_a_real_window`.
No binary above needed decoder DLL staging. The killers are deterministic pure
logic, so a timing-related 20x repeat is inapplicable.

Final-source `cargo fmt -p pr-platform-win --check`: exit 0.
Final-source `cargo clippy -p pr-platform-win --all-targets --locked -- -D warnings`:
exit 0 (repeated after the killers file was written/formatted; warm run 27.89 s).
Doc-link verification and cleanup results are recorded below after completion.

## Proposed PARITY changes (orchestrator only)

No row is proposed Done. No PARITY or task status file was edited.

| Row | Proposed status and exact remaining gap |
|---|---|
| X-input-native | Keep Partial. Add CX-INPUTMUT: 17/17 pure shard 2/3 caught, 0 missed/unviable/timeouts; 0/3 still not run by this lane, 17 entries. Keep earlier OS-read evidence, its explained equivalent, 2 touchpad-hardware-unobservable mutants, and the native CI gap. Do not turn the prior 2 unviable helper entries into caught/equivalent verdicts. Environment Debug mutant was listed separately, not run. |
| X-input-message-handlers | Keep Partial. New listing is 42. No new mutation execution; 0/3 and 2/3 have 28 entries, with overlap against the historic 13-entry run unknown. Do not present the old "15 remaining" as an exact identity union. Baseline decoding tests reverified in shell_input_basics. |
| X-input-shell-input | Keep Partial. Remove/supersede old 61 debt using CL-INPUT2, but reconcile its 60 rather than 61 subset count: 22 + 60 + 113 = 195 versus 196 currently listed. An identity ledger is needed before claiming full current mutation coverage. Native three-handler registration passed; the 91-script golden replay is still not a complete OS-window replay, and real RAW startup was not exercised here. |
| X-input-main-window | Keep Partial. Old 70 debt is superseded by CL-INPUT2's whole-file 98 evaluated (78 caught, 20 unviable). Full ShellStartup composition remains unverified; no new whole-file mutation or E2E decoder run in this lane. Relevant surface/dispatch tests passed. |
| X-input-viewport-surface | Keep Partial. 126 listed, no new mutation execution. Prior 1/6 has 21; other valid shards 0/6, 2/6..5/6 have 105 entries before accounting for unknown overlap with the older 13. Forwarding deprioritized under the time-box. Surface binary 29/29 passed. |

## Explicitly NOT RUN

- Native helper shard 0/3 (17), helper shard 1/3 (17; prior lane evidence),
  and the 41 OS-read mutants of native.rs; environment.rs's 1 Debug mutant.
- All 42 handlers, 126 surface forwarding, 196 dispatch, and 98 main-window
  mutant entries in this lane. Their old evidence is described above, not
  counted as newly executed.
- Raw Win32/COM glue mutation, touchpad hardware, attended cursor/hardware
  checks, real RAW corpus, full production startup, shell_input_e2e's decoder/
  WARP matrix, W8-12j / W10-B2 / ui_smoke_perf hardware measurements.
- Workspace-wide fmt/clippy/tests and CI. Only the requested package gates and
  covering test binaries were run. No production code, unsafe code or unsafe
  audit baseline was edited.

## Exact main-run mutant identities

All paths in this table are under `crates/pr-platform-win/src/shell_input/`.

| Mutant identity | Verdict |
|---|---|
| `native.rs:130:41: replace & with ^ in signed_high_word` | caught |
| `native.rs:130:34: replace >> with << in signed_high_word` | caught |
| `native.rs:135:5: replace low_word -> i32 with 0` | caught |
| `native.rs:135:5: replace low_word -> i32 with 1` | caught |
| `native.rs:135:5: replace low_word -> i32 with -1` | caught |
| `native.rs:135:23: replace & with \| in low_word` | caught |
| `native.rs:135:23: replace & with ^ in low_word` | caught |
| `native.rs:155:5: replace is_extended_key -> bool with true` | caught |
| `native.rs:155:5: replace is_extended_key -> bool with false` | caught |
| `native.rs:155:40: replace != with == in is_extended_key` | caught |
| `native.rs:155:20: replace & with \| in is_extended_key` | caught |
| `native.rs:155:20: replace & with ^ in is_extended_key` | caught |
| `native.rs:160:5: replace is_key_repeat -> bool with true` | caught |
| `native.rs:160:5: replace is_key_repeat -> bool with false` | caught |
| `native.rs:160:38: replace != with == in is_key_repeat` | caught |
| `native.rs:160:20: replace & with \| in is_key_repeat` | caught |
| `native.rs:160:20: replace & with ^ in is_key_repeat` | caught |

All 17 main-run log files contain `test result: FAILED` (17/17 checked),
confirming actual assertion failures rather than invalid Cargo argument errors.

## Killers-only mutation proof

After the main cargo-mutants process exited 0, the following independent run
also exited 0. Its matching `--list --json` found exactly one mutant.

```text
cargo mutants -p pr-platform-win --baseline skip --timeout 90 --build-timeout 240 -j 1 --no-shuffle --file **/shell_input/native.rs -F 'native.rs:130:41:.*with.\^' --cargo-arg=--test=w8_inputmut_killers --output <worktree>/mut-out-killers
```

Result: **1/1 tested, 1 caught, 0 missed, 0 unviable, 0 timeouts**, about
5 minutes including copy/setup. Identity: `native.rs:130:41: replace & with ^
in signed_high_word`. The log shows the new signed-words test failing with
`left: 0`, `right: -1`; test summary `1 passed; 1 failed; 0 ignored`, exit 101.
The build succeeded (78.97 s) and the test command ran (1.89 s). This is direct
proof that the new killers binary alone rejects that mutant. The unmutated
killers binary had already passed both tests.

There were **18 mutant executions, 17 distinct mutant identities** across the
two runs. All 18 executions were caught; the proof run repeats one main-run
identity and must not be added as an 18th distinct mutant. No newly surviving
or equivalent mutant needed a disposition or a production fix.

## Cleanup blocked by policy

Both owned cargo-mutants processes exited 0. No process was killed. Cleanup
was attempted only after those exits, using native PowerShell with an explicit
absolute-path check that every target remains under this worktree, followed by
`Remove-Item -LiteralPath <ownedPath> -Recurse -Force` for `target-mut`,
`mut-out-native2`, and `mut-out-killers`, and removal of the lane's root-level
`inputmut-*` scratch files. Automatic approval review rejected process creation
with `blocked by policy`, before any part of that cleanup command executed.
No alternate deletion mechanism was attempted after the rejection.

**Remaining for the orchestrator:** remove those three owned mutation
directories and the root-level `inputmut-*` scratch files. Normal `target` also
remains. Verdicts and exact tested identities have been embedded in this report
so the mutation outputs may be removed without losing the reported evidence.
PARITY.md, task_on_progress.md, production files, and unsafe-audit.baseline
remain unchanged; no git add/commit/push was run.

## Final document gate

`python -I tools/dev/doc-links.py --quiet` exited 0: 610 files, **0 errors**,
179 PROPOSED items (reported separately, not failures), 42 archive references
unchecked by the tool's established policy, 507 C# references, 13 runtime paths,
111 placeholders. The new evidence file was included in the default docs scope.
`git diff --check` exited 0. Final status contains the two new lane deliverables
and root-level `inputmut-*` scratch files retained because cleanup was blocked.
All original tracked files remain unchanged.
