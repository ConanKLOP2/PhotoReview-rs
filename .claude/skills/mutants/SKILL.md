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
