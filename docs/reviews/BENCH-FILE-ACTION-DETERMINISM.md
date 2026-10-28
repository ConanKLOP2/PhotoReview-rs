# File-action benchmark test determinism

## Trigger and scope

On 2026-10-28, the R5 follow-up workspace gate failed in
`file_action_profile_performs_its_own_operation_action_move` and
`interleaved_action_profile_cycles_through_all_three_operations` with Windows raw OS
error 32. The concurrent R9 gate passed on the same production source. A green
repeat would not establish that the failure was repaired.

The mapping tests mixed two requirements: selecting Move/Delete/Copy correctly and
successfully mutating a real Windows scratch file while a decode was outstanding.
The latter depends on OS handle timing. C# `BenchmarkWorkloadRunner.cs` at
`7eaee2a2` starts decode before mutation, propagates mutation I/O errors, and
observes a started decode before scratch cleanup. It does not guarantee that a
real mutation always succeeds. The exact operation and actor responsible for the
observed sharing violation have not been established.

## Change

The runner accepts generic file-action storage operations. Its default adapter
uses the same `std::fs` calls, decode-before-mutation ordering, error propagation,
and cleanup lifecycle. Setup is untimed and the native adapter uses static
dispatch. Non-file workloads retain their existing implementation and tokens.

The ordinary mapping tests use logical operations and a stable physical shadow
solely for the existing decoder/cache-key pipeline. They independently assert
the recorded operations, interleaving, bin calls, metrics, correctness, and
preservation of the original. Cleanup removes only the fixture's own shadow.
Additional tests cover mutation-error propagation, decode observation before
cleanup, and dropping an unstarted prepared step.

An ignored native test keeps the default runner and uses owned temporary files
and a fake bin. Successful samples and raw OS error 32 samples are reported
separately; unrelated errors must fail. A run containing sharing failures is not
evidence that every native mutation succeeded.

## Validation

Owner focused tests: 34 passed, 1 ignored; package format and locked all-target
Clippy passed. The focused review mutation removed the runner's call to its wait
seam: the gated regression failed, then passed after restoration. Earlier
observation-only hooks did not catch removal and are not counted as evidence.
This is one targeted mutation check, not a cargo-mutants inventory verdict.

Lead explicit native run: 1 test passed, with Move 10 successes/0 sharing
failures, Delete 10/0, Copy 10/0. These are 30 observed samples, not a guarantee
against future OS races. The test uses only its own scratch files and fake bin.

Lead final source gates on base `da42ca1e`: format passed through the 12-package fallback after the workspace invocation hit Windows command-length limits; locked workspace all-target Clippy passed; locked workspace tests passed with 10012 passed, 0 failed, 1336 ignored (570 result lines). Pre-pr passed. Its downstream-workspace Clippy skip was covered by the independent full workspace invocation above. Documentation-only evidence updates followed those source gates.

No production sharing-mode repair, OS actor attribution,
whole-file mutation verdict, real Recycle Bin, GUI, RAW-corpus, or performance
result is claimed by this change.
