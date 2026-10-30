# AUDIT-MUTATION-FLAG: mutation passes run with an invalid native-test flag (2026-10-30, lane `cl-flagaudit`)

Status: audit report. No production change. Triggered by `cl-sec01`: `W2-FS-EVIDENCE-2026-10-29.md` ran native mutation passes with
`--cargo-test-arg=--include-ignored`; cargo rejected it and cargo-mutants (`--baseline skip`) still counted the mutants as caught.

## 1. The defect, reproduced

Throwaway crate (one `#[ignore]` test, `add(a, b)`, 5 mutants), cargo-mutants 27.1.0, `--baseline skip`:

| Arguments | cargo test line in the mutant log | Result |
|---|---|---|
| (none) | tests run, the ignored test is skipped | 5 MISSED (expected: ignored tests are skipped) |
| `--cargo-test-arg=--include-ignored` | `cargo test ... --include-ignored` gives `error: unexpected argument`, `Failure(1)` | **5 caught, no test ran** |
| `-- --include-ignored` (one bare `--`) | the same line, the same error | **5 caught, no test ran** |
| `--cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1` | `cargo test ... -- --include-ignored --test-threads=1`, `running 1 test` | real run (the mutant fails the test) |
| `-- -- --include-ignored --test-threads=1` (the second `--` is the libtest separator) | same as above | correct |

Rule: cargo-mutants hands everything after `--cargo-test-arg=` (or after one bare `--`) to `cargo test` BEFORE its own `--`. libtest flags
(`--include-ignored`, `--ignored`, `--test-threads`, `--nocapture`, `--exact`, `--skip`) must come after a separator. A failed test command is
indistinguishable from a failing test for cargo-mutants, so the false result is "caught", never "missed". Fingerprint of a void pass: all
caught, very fast per mutant, 0 missed where a plain run had survivors, and no `running N tests` in the per-mutant log.

## 2. Where the repo mentions the flags

Searched: `docs/reviews/*`, `docs/archive/*`, `docs/PARITY*.md`, `task_on_progress.md`, `contracts/*`, `tools/autopilot/*`, `tools/dev/*`,
`.claude/skills/*`, `AGENTS.md`, `.github/workflows/*`, `crates/**/tests` (330 lines with `include-ignored`, `cargo-test-arg` or `test-threads`).

| Group | Places | Verdict |
|---|---|---|
| Plain `cargo test ... -- --include-ignored --test-threads=1` (native runs, contracts, DEVELOPING.md, NATIVE-EVIDENCE, `native-tests.sh`, workflows, test file headers; about 290 lines) | everywhere | not affected: the `--` after the cargo arguments is the correct libtest separator. (CX-SURV recorded and fixed one wrong plain `cargo test` form itself.) |
| `tools/autopilot/codex-pipeline.sh` `run_mutants` (`CTA+=("--cargo-test-arg=$w")`) | 1 | not affected: `testargs` yields only `--test <bin>` and `--lib`, no libtest flag, no `--` inserted and none needed. Ignored tests stay skipped, which can only produce extra MISSED, never a false caught. |
| `tools/dev/mutants.sh` before this lane | 1 | not affected (passed only `-- --test a --test b`) but it had no native option and no check that tests run. Fixed below. |
| `.claude/skills/mutants`, `tools/dev/docs/mutants.md`, `tools/autopilot/README.md`, `workflows/*.js` | 8 | no mutation command with the flag; the workflow prompts only say to run `cargo test -- --include-ignored`. |
| Mutation commands with a CORRECT form | `CX-MUTW4` (3 passes), `CX-W4REST` (corrected pass), `CX-REL` (`--cargo-test-arg=-- --cargo-test-arg=<name> --cargo-test-arg=--exact`), `CX-DIALOGS` (`-- -- --include-ignored`) | valid; `CX-W4REST` documents that an earlier wrong attempt was discarded and credited nothing |
| Mutation commands with a WRONG documented form | `W2-FS-EVIDENCE` (lone `--cargo-test-arg=--include-ignored`) | **void** (confirmed by `cl-sec01`) |
| Mutation commands documented only as `-- --include-ignored --test-threads=1` after the cargo-mutants options (one `--`) | `CL-P01`, `CL-STARTUP`, `CL-MAINWIN2`, `CL-HOST2`, `CL-REST2`; `W6-PLATFORM-EVIDENCE` ("native binaries with `-- --include-ignored` where stated") | **suspect**: written literally this is the invalid bare form; it may be shorthand for `-- -- --include-ignored` (valid). The raw logs are gone, so it cannot be told apart. |

## 3. Affected evidence

| Evidence | What used the flag | Verdict | PARITY rows touched |
|---|---|---|---|
| `W2-FS-EVIDENCE-2026-10-29.md` section 2, `physical.rs` passes 2-7 (35 + 97 + 185 + 143 + 17 + 1 mutants, "0 missed") | lone `--cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1` | **certainly void.** Only pass 1 (plain tests, 28 caught / 8 missed) is real; the 8 survivors "re-run with native tests and caught" were never actually re-run with them. Mutants that plain tests or the non-ignored killers reach keep their result. | L31 X-physical-fs, L43 AR16, L40 and L433 D-05, L41 RV-C05 (cites the 427 run), L45 SEC-01 / F-05 (strongest doubt in AUDIT-DONE) |
| the same report, other files (`counting`, `writer`, `perf_csv`, `perf`, `metrics`, `checker`) | "every pass that needs a native test" used the flag | **suspect** for mutants whose only killer is a native test (e.g. `try_start_from_environment`); not itemised in the report | L48 X-perf-diagnostics, L74 F-23, L50 P-09, L75 F-22 |
| `CL-FSFLUSH-2026-10-30.md`: 17 mutants of `write_all_text_atomic` etc. with the (ignored, native) killers binary alone | command line not recorded | **suspect**: the killers are `#[ignore]`; 15 caught / 0 missed is the void fingerprint if the flag was missing or wrong. The 7 hand mutants are independent evidence. | L40 D-05 |
| `W6-PLATFORM-EVIDENCE-2026-10-29.md` round 2 rows with `*_native` binaries (`forward_server`, `forward_client`, `vblank_driver`) | "`-- --include-ignored` where stated", form not recorded | **suspect** for mutants killed only by the native binaries | L393 P-05, L399 P-11 |
| `CL-P01-2026-10-30.md` (82 tested, 74 caught, 0 missed, killers binary alone) | `--cargo-arg=--test=w6_p01_killers -- --include-ignored --test-threads=1` (one `--`) | **suspect** (void if literal) | L58 P-01, L391 P-03, L393 P-05, L399 P-11 |
| `CL-STARTUP-2026-10-30.md` groups a-c (185 tested, 0 missed) | `... -- --include-ignored --test-threads=1`; the report notes that a run WITHOUT the flag showed 21 MISSED and "with the flag all were caught" | **suspect**, with the void fingerprint (21 MISSED to 0 MISSED) | L655-L659 X-startup-*, L662 |
| `CL-MAINWIN2-2026-10-30.md` (223 tested, 0 missed) | `-- --include-ignored --test-threads=1` | **suspect** | L631-L633 X-main-window-* |
| `CL-HOST2-2026-10-30.md` `ui_dispatcher.rs` (90) and `dpi.rs` (28), 0 missed | `-- --include-ignored --test-threads=1` | **suspect**; `wndproc.rs` (killers alone, no flag) is fine | L600 X-host-ui-dispatcher, L602 |
| `CL-REST2-2026-10-30.md` X-startup-composition-root (`secondary_dialogs.rs`, 72) | `startup_composition_root (--include-ignored --test-threads=1)` | **suspect** | L656 |
| `CX-DIALOGS`, `CX-MUTW4`, `CX-W4REST`, `CX-REL`, `CX-SURV` | correct form or no flag | **not affected** | none |

Row numbers are the PARITY.md lines at the time of this audit.

No PARITY row is demoted. For every Done row above the status also rests on passing golden or unit tests, so the mutation figure is only
part of the evidence and "certainly void" does not make a row unsupported. The X-physical-fs, AR16 and D-05 rows (L31, L40, L43, L433) now
carry a note in PARITY.md saying the native-only mutation claim is void and a re-run is pending. The other rows are only listed here.

## 4. Fix in the tools (this PR)

- `tools/dev/mutants.sh`: new `--native` (adds `--cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`),
  `--cargo-arg A` and `--cargo-test-arg A`; a lint refuses a libtest flag in front of the separator (exit 2, prints the correct form);
  `--lint-args <raw cargo-mutants args>` checks any command line without running it; a preflight runs the UNMUTATED `cargo test` with the same
  arguments once and fails (exit 5, before any mutant) when it exits non-zero or executes 0 tests (`running N tests` summed).
  Test binaries now go through `--cargo-test-arg=--test --cargo-test-arg=<bin>`, like the pipeline does.
- `tools/dev/test/mutants-selftest.sh`: lint, `--native` argument order, preflight with zero tests and with a failing command.
- `AGENTS.md` (Mutation testing and parallel agents) and `.claude/skills/mutants/SKILL.md`: the trap and the self-check.
- `tools/autopilot/codex-pipeline.sh`: comment only; the pipeline never passes libtest flags.

## 5. Proposed re-run lanes (list only, nothing run here)

Always with `tools/dev/mutants.sh ... --native` (or the verified raw form) and keep the preflight line in the report.

1. `cl-physical-rerun`: `physical.rs` native-only mutants (the 8 pass-1 survivors and the mutants of passes 2-7: `final_link_target`,
   `strip_nt_prefix`, `try_copy_new_with_proof`, `write_all_text_atomic`, `delete`, `resolve_*`, `immediate_link_target`, listing, matcher, FFI wrappers)
   with `io_physical`, `io_physical_seams` and `w2_fs_mutation_killers`; also settles SEC-01 `MAX_FOLLOWED_LINKS` (shared with `cl-sec01`). Rows L31, L40, L41, L43, L45.
2. `cl-w2-native-rerun`: native-dependent survivors of `perf_csv.rs`, `perf.rs`, `writer.rs`, `checker.rs` (rows L48, L50, L74).
3. `cl-fsflush-rerun`: the 17 flush mutants with `w2_fs_flush_killers` (D-05).
4. `cl-w6-native-rerun`: `forward_server`, `forward_client`, `vblank_driver` with their `*_native` binaries, plus the `CL-P01` set (P-01, P-03, P-05, P-11).
5. `cl-host-rerun`: `ui_dispatcher.rs`, `dpi.rs` (CL-HOST2), `frame`/`viewing`/`windowing` (CL-MAINWIN2), `secondary_dialogs.rs` (CL-REST2).
6. `cl-startup-rerun`: CL-STARTUP groups a and b (the ignored-test dependent mutants: `ScopeGate`, `GlobalLog`, `phase_one`).

A re-run that reproduces the old numbers closes the suspicion; a pass is valid when its preflight shows `running N tests` (N > 0). Cheaper
triage first: for each suspect lane run only the preflight command (one `cargo test`) to prove that the documented argument shape works at all,
then re-run the mutants of the native-only killers.
