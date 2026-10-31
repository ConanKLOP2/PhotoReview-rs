# CL-MUT-GOLDEN-INFRA: golden tests failing inside cargo-mutants (2026-10-31, lane `cl-mut-golden-infra`)

Status: fix landed (test-support helper + `tools/dev/mutants.sh`); no production code changed. Older results below still need a re-check.

## Root cause

The earlier belief "cargo-mutants does not copy `tests/golden`" is wrong. `tests/golden` is tracked, not gitignored, and it IS in the temp copy
(an unmutated baseline in a fresh target dir passes: `bounded_lru` 19/19, `lru_trace_golden_matches_csharp ... ok`).

The real cause: `pr_test_support::golden::repo_root()` used the COMPILE-TIME `env!("CARGO_MANIFEST_DIR")`. cargo-mutants builds in a new temp
directory every run (`%TEMP%\cargo-mutants-<lane>-XXXX.tmp`, deleted afterwards). The dependency `pr-test-support` is not mutated, and cargo
keeps it `Fresh` from the earlier copy when the target dir is reused (`mutants.sh` uses the stable `mut-target-<crate>`; lanes reuse theirs).
The baked path then points at a deleted directory: `no tests/golden above ...\cargo-mutants-...-cloIrW.tmp\crates\pr-test-support`, every golden
test fails, and with `--baseline skip` each mutant that a golden test would have caught is counted caught only because the test run failed.

A second hazard of a reused target dir (found while proving the fix): artifacts built from the LAST mutant of a previous run stay `Fresh`
(the new copy keeps the original, older mtimes), so the next unmutated baseline fails (`bounded_lru` 6-7 failed in `remove_*`/`set_*`) or a
verdict is judged against a mutated build. Bumping the mtime of the crate sources before the run fixed it.

## Fix

1. `crates/pr-test-support/src/golden.rs::repo_root()`: order `PHOTOREVIEW_GOLDEN_ROOT` (must hold `tests/golden`, else a clear assert),
   then the RUNTIME `CARGO_MANIFEST_DIR` (cargo sets it to the package under test, i.e. the current copy), then the compile-time one.
   One shared helper, no per-crate test changes.
2. `tools/dev/mutants.sh`: exports `PHOTOREVIEW_GOLDEN_ROOT=$ROOT`, `touch`es `crates/<crate>/{src,tests}/*.rs` (stale-target hazard), and prints a
   WARNING after a run when any log contains `no tests/golden above`. Docs: `tools/dev/docs/mutants.md`, `.claude/skills/mutants/SKILL.md`.
3. New `crates/pr-test-support/tests/golden_root.rs` (2 tests: root resolves, `lru-trace` loads).

## Evidence (pr-core, `-F 'remove_entry -> bool with true'`, `--baseline run --test bounded_lru`, own target dir)

| Step | Code | Result |
|---|---|---|
| Run 1, fresh target | before | baseline ok (19 passed), mutant caught |
| Run 2, same target, new temp copy | before | baseline FAILED, `no tests/golden above ...cloIrW.tmp\crates\pr-test-support`, 6 failed |
| Run 1, fresh target | after | baseline ok (19 passed), mutant caught |
| Run 2, same target, new copy | after | NO `no tests/golden` panic; 7 failures remained from the stale mutated artifacts (second hazard) |
| Run 3, same target, sources touched | after | baseline ok, mutant caught |

`cargo test -p pr-test-support --test golden_root`: 2 passed. `bash tools/dev/test/mutants-selftest.sh`: PASSED. fmt clean.

## Other crates

No other golden helper exists: every `golden::load` caller goes through `pr-test-support`, so one change covers all crates. Tests that use
`env!("CARGO_MANIFEST_DIR")` directly (pr-app `benchmark_presentation`, `presenters_secondary`, `services_shortcut_key_name`, pr-l10n
`localizer`/`generated_tr`, pr-core `recovery_retry:3035`, pr-imaging-raw) live in the mutated crate, which is rebuilt in each copy; they are
only exposed if the crate under test is a different crate than the test's owner (not the case for `-p <crate>`).

## Results that should be re-run (list only, nothing re-run here)

A result is suspect when it used a target dir reused across runs, where a mutant was reported caught and the only killers were golden tests
(or the baseline was skipped and the run log contains `no tests/golden above`). Check with
`grep -rl "no tests/golden above" <out>/mutants.out/log`. Reports that mention golden and mutation: `CL-MUT-BENCH`, `CL-MUT-MISC`,
`CL-MUT-PERFCOM`, `CL-MUT-RAW`, `CL-MUT-STATE2`, `CL-MUT-ZOOMCORE`, `CX-INPUTMUT`, `CX-MUTW4`, `MUT-NEWFIXES`, `IMAGING-MUTATION`,
`W2-CONFIG-MUTATION`, `W3-01-JOURNAL-MUTATION`, `W3-05-UNDO-MUTATION`, `W8-PRESENTER-MUTATION`, `W8-PURE-MUTATION`,
`AUDIT-MUTATION-FLAG`. Re-run only the mutants those reports list as "caught by golden only" (survivor-style `-F` reruns), with a fresh
target dir or the fixed `mutants.sh`.
