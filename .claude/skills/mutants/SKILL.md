---
name: mutants
description: Use when the owner asks to run time-boxed mutation testing on specific source files of a crate and see which mutants survive.
---

Confirm crate, source files, test binaries and budget with the owner; mutation is slow (each mutant rebuilds the crate)
and should run only on pure logic, after the port is merged, with few agents building.
Run `bash tools/dev/mutants.sh <crate> <src-file>... --tests a,b --budget 1200` in the background; add
`--rerun-survivors` only when asked. Never kill cargo, rustc or cargo-mutants by name.

Report to the owner in Vietnamese: tested / total, caught, missed, unviable, timeouts, whether the budget ran out
(partial result, never a full verdict), and the survivors.txt path with the missed mutants.

Run every command from the repository root.

Trap, the correct form of the native flags: `#[ignore]` tests are skipped by cargo-mutants, so native tests need `--native` (it runs
`--cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1`). A lone `--cargo-test-arg=--include-ignored`
or a bare `-- --include-ignored` is rejected by cargo (`unexpected argument`), and with `--baseline skip` every mutant is still reported
caught although no test ran. `mutants.sh` refuses that form and runs the unmutated test command once first (it must exit 0 and execute
at least one test). Check a raw command line with `bash tools/dev/mutants.sh --lint-args <args>`. Self-check: one deliberately broken
mutant (or a test that must fail) has to be reported MISSED/caught correctly, and the log must contain `running N tests`.

Golden trap: if a log says `no tests/golden above`, the mutants caught only by golden tests are NOT really caught (fixed in `pr_test_support::golden::repo_root`, which reads `PHOTOREVIEW_GOLDEN_ROOT`; `mutants.sh` sets it and warns). Mutation results made before CL-MUT-GOLDEN-INFRA with a reused target dir must be re-checked.

Trap, `-F` does not narrow (P-111 option B, owner decision 2026-11-03): `-F REGEX` (and `--skip`) are passed to cargo-mutants as name filters over the mutant
names; they do NOT narrow a batch to exactly the functions you wrote. A batch that was meant for `retry_group` also pulled in `retry_core`, `already_handled`
and `not_journaled`, and a `--skip` list did not stop `retry_core` from running (seen in the recovery_retry.rs batch of docs/reviews/GLUE-MUTANTS-2026-11-03.md). So batches
of one file overlap: the same mutant can appear in two batches, and the count in a log is the real `Found N mutants` line, never the number of functions you
named. Report tested/caught/missed from the log of each batch, de-duplicate survivors by `file:line:col` before you count them, and do not assume a function
outside `-F` was left untested. The cause is not investigated.
