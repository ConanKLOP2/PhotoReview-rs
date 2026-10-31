# CX-SETTINGSWIN: settings window evidence (2026-10-31)

Status: Partial. Lane `feat/cx-settingswin`, starting HEAD `9f85ce288dc46e38ac0de0134ba0f7a76ca235e4`. No production or unsafe changes. No Git staging, commit or push. PARITY and task_on_progress are intentionally left to the orchestrator.

## Scope and existing evidence

Read AGENTS mutation rules, LESSONS, the current PARITY row, [CL-SETTINGSUI](CL-SETTINGSUI-2026-10-30.md), and [CX-SETUI](CX-SETUI-2026-10-31.md) / [CX-SETUI2](CX-SETUI2-2026-10-31.md). Those builder runs do not close the host/frame/surface debt. Current inventory is **400** mutants: host.rs **80**, frame.rs **190**, surface.rs **130**. The supplied historical host count 68 is not the current denominator.

The new `crates/pr-platform-win/tests/w8_settingswin_killers.rs` contains five deterministic tests plus seven imported existing frame tests, compiled as ONE integration binary. Tests cover URL scheme/injection/UTF-8 boundaries without opening a browser; note/warning precedence; combobox count clamps, fallback at zero and saturating arithmetic; independent placement-coordinate changes and clamped negative sizes; wrapping page IDs at integer extremes. Existing frame tests also load the nine-case frame golden. No sleeps, wall-clock assertions, real configuration, user files or Recycle Bin operations were added. No production bug was found.

C# was read only at `7eaee2a2:src/PhotoReview.Shell.Win32/Dialogs/SettingsUi/SettingsDialog.cs`: ViewportId, PageIdBase, StaticTextColor and PageSurface.ComboDropHeight agree with the tested ordinary arithmetic/branch decisions. Saturating/wrapping boundary assertions additionally pin the current Rust helper contracts. Host URL validation is documented Rust hardening with no C# twin.

## Reproduction and isolation

Normal target: `<worktree>/target`; mutation target: `<worktree>/target-mut`. Both use `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0`. Test/mutation commands set `PHOTOREVIEW_RS_DATA_ROOT=<worktree>/target/settingswin-isolated` and `PHOTOREVIEW_RS_ISOLATE_CONFIG=1`. Mutation runs use `-j 1`; corrected build concurrency is `CARGO_BUILD_JOBS=2`, `--cargo-arg=--jobs=2`. No normal cargo test ran in target-mut.

DLL staging passed with verified pinned hashes after process-local safe.directory entries for this worktree and the read-only C# repository. No global Git configuration was changed. Process inventory through CIM was denied; no permission escalation was attempted.

```text
powershell -NoProfile -ExecutionPolicy Bypass -File tools/dev/stage-native.ps1
cargo test -p pr-platform-win --locked -j 2 --test w8_settingswin_killers --lib -- --include-ignored --test-threads=1
cargo test -p pr-platform-win --locked -j 2 --test w8_settingswin_killers --lib
cargo test -p pr-platform-win --locked -j 2 --test settings_dialog_frame_e2e --test settings_dialog_layout_e2e -- --include-ignored --test-threads=1
cargo clippy -p pr-platform-win --all-targets --locked -j 2 -- -D warnings
```

Preflight/native tests have 600/240-second guards. Native-included preflight passed **261 lib + 12 killers**, zero failed/ignored. The exact mutation-style default preflight also passed (native lib tests ignored); its log is separate. Native frame_e2e **11 passed** and layout_e2e **1 passed**, zero failed/ignored. Locked all-target Clippy passed, exit 0 (7m22s). No new test is timing-related, so the conditional 20-repeat gate is not applicable. These results do not establish attended DPI/GDI/language scenarios or C# runtime type-label parity.

## Mutation attempts and limits

The intended 24-mutant selection covers viewport_id/page_id_base, is_web_url, static_text_kind, four combo_drop_height mutations, and two placement_change mutations. The actual CLI selection reported **Found 29**, also including five SHELLEXECUTEINFOW field-deletion mutations in default_open_url. The corrected `-E default_open_url` invocation still reported 29; those raw Win32 fields are not intended logic evidence. Raw selection lists and runner scripts are retained in `target/settingswin-evidence/` so this discrepancy can be investigated without pretending the selection was 24.

First attempt used a 1150-second guard and 600-second per-build guard. The windows dependency compiler failed while allocating memory; tool-reported Unviable outcomes from this dependency failure are infrastructure failures, NOT source-unviable mutants or caught evidence. The owned timeout PID 14498 was sent SIGTERM; no process was killed by name. The session subsequently exited 1. Corrected attempt uses a 700-second total guard, 450-second build guard and two Cargo jobs. Both commands compile only `--test=w8_settingswin_killers --lib`, test timeout 90 seconds, and `--baseline skip` after a successful exact-style preflight.

Final counts, every completed outcome, final gates and cleanup are recorded below after the owned run ends. No equivalence is asserted. A partial or infrastructure-blocked verdict cannot promote the row to Done.

## Proposed PARITY change for the orchestrator

Keep `X-settings-ui-window` **Partial**. Add the new 12-test killers binary and the native re-verification above. Retain all unclosed host/frame/surface mutation debt, C# runtime page type labels, attended DPI/GDI/language scenarios, ScrollInfo glue and the existing posted-action panic-path end-to-end gap. Do not treat this lane as a complete mutation verdict.

## Final measured outcome

The corrected run was stopped through its owned timeout PID 15683. Both owned mutation sessions have exited. During shutdown, the interrupted page-ID build was classified Unviable and one Win32 cbSize-field build began without a final outcome; no Win32 verdict is claimed. **5 unique mutants evaluated successfully / 400 listed: 5 caught, 0 missed, 0 source-unviable, 0 mutant timeouts.** The tool recorded 6/29 outcomes: five Caught and one infrastructure Unviable. The sixth was frame.rs:98:5 page_id_base -> -1: rustc exited 143 during the requested stop; this is not a source-unviable or caught verdict. There were zero tool Timeout outcomes. No MISSED mutant required repair or killers-only rerun. No equivalence is claimed.

The first attempt additionally reported **2 Unviable** outcomes (viewport_id -> 0 and -> 1), rejected as dependency allocation/build failures. The same two mutants were subsequently caught by assertion failures in the corrected run. Across both attempts there are **8 completed tool outcomes**, comprising **3 infrastructure failures and 5 valid caught verdicts**, covering **6 unique mutants with raw outcomes**. Only **5 unique mutants** have valid test verdicts. Infrastructure failures are excluded from the valid tested numerator. The first attempt additionally began viewport_id -> -1 without a final outcome; that mutant was later caught in the corrected attempt.

| Evaluated mutant (relative to settings/dialog/) | Final verdict and observed failure |
|---|---|
| `frame.rs:93:5 viewport_id -> 0` | caught: killers `page_ids_preserve_wrapping_at_integer_extremes`, existing page-math and frame-golden assertions |
| `frame.rs:93:5 viewport_id -> 1` | caught: same assertions |
| `frame.rs:93:5 viewport_id -> -1` | caught: same assertions |
| `frame.rs:98:5 page_id_base -> 0` | caught: killers integer-extreme and existing page-math/frame-golden assertions |
| `frame.rs:98:5 page_id_base -> 1` | caught: same assertions |

Caught logs show default lib tests passing, followed by 12 integration tests running and three integration assertions failing. These are test failures, not Cargo argument, linking, DLL-loading or compiler failures. The exact unmutated default preflight passed **176 lib tests** (85 native ignored) plus **12 killers**, zero failed. Native-included evidence above is separate.

| File | Listed | Unique valid verdicts | NOT RUN to a valid verdict |
|---|---:|---:|---:|
| host.rs | 80 | 0 | 80 |
| frame.rs | 190 | 5 | 185 |
| surface.rs | 130 | 0 | 130 |
| Total | 400 | 5 | **395** |

NOT RUN includes the remaining page_id_base -> -1 mutant, all host URL/adapter mutants, all surface logic mutants, and raw Win32/COM/GDI/ScrollInfo glue. New host/surface tests passed unmutated but have no mutation verdict here. Full forwarding, attended DPI/GDI/language/runtime-label scenarios and posted-action panic end-to-end coverage remain unverified. The row cannot become Done.

Final code gates after the comment-only correction: `cargo fmt -p pr-platform-win --check` **passed**; locked all-target Clippy with warnings denied **passed**; killers **12 passed, 0 failed/ignored**. Normal logs, exact lists and scripts are under `target/settingswin-evidence/`; raw mutant logs remain in the two `mut-out-settingswin-*` directories.

## Cleanup blocker

An automated permission check rejected the combined evidence-copy/append/cleanup shell command with **blocked by policy**, before execution. No alternate deletion command was attempted. The orchestrator must remove this worktree's `target-mut`, `mut-out-settingswin-selected` and `mut-out-settingswin-pure` after preserving any required logs. Both owned mutation sessions had exited before the rejected cleanup. No foreign process was killed. This is an unfinished cleanup requirement, not a successful cleanup claim.

## Final raw accounting and documentation gate

| Attempt | Tool outcomes / selected | Caught | Missed | Raw Unviable | Timeout |
|---|---:|---:|---:|---:|---:|
| Initial (allocation failure) | 2 / 29 | 0 | 0 | 2 | 0 |
| Corrected (stopped) | 6 / 29 | 5 | 0 | 1 | 0 |
| Execution totals (includes repeats) | 8 | 5 | 0 | 3 | 0 |

The corrected raw Unviable is `frame.rs:98:5 replace page_id_base -> i32 with -1`; the build log ends with rustc exit **143**, caused by the requested stop. It has no valid test verdict and must be rerun. The initial two Unviable outcomes are the dependency allocation failures described above, not equivalent mutants. Thus **6/400 unique mutants reached raw outcomes, but only 5/400 have valid caught evidence**; 394 never reached a raw outcome and 395 still lack a valid verdict. The attempted `host.rs:47:9 delete cbSize` build during shutdown has no final result and remains in NOT RUN. No raw Win32 field mutation is claimed caught, equivalent, or tested to completion.

`python -I tools/dev/doc-links.py --quiet` passed: **files=610, errors=0, proposed=179, archive_unchecked=42, csharp_refs=507, runtime_paths=13, placeholders=111**. PROPOSED items are separate from errors. Final scope inspection shows only the new killers file and this evidence file; production, PARITY and task_on_progress have no diff.