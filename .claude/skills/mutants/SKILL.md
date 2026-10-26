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
