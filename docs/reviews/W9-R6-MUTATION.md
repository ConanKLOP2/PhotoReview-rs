# W9-R6 selected postmerge mutation evidence

Verified 2026-10-28. Production PR #287 merged as `520d72919d05a31ffcc0129cb39f801bcac02e9c`. The follow-up adds only a test to the retired-wait module; production behavior is unchanged. R9 supplies the file-action matrices in the same review change.

## Selected scope and chronology

| Source and filter | File candidates | Selected | Initial verdict |
|---|---:|---:|---|
| `coordinators/presenter/rename.rs`, `ordinal_ignore_case_suffix_start` |17|4|4 caught, 0 missed/unviable/timeouts|
| `coordinators/presenter/state.rs`, `RetiredWait::observes` or `retire_wait_chain` |123|18|17 caught, 1 missed, 0 unviable/timeouts|

The survivor deletes the Dimensions match arm of `RetiredWait::observes` at state.rs:279. Existing tests did not distinguish a pending dimensions wait from other completion kinds. The new `retired_dimensions_wait_observes_only_matching_success_or_error_completion` checks that unrelated stat/frame/cancel events do not complete that wait, and both successful and failed Dimensions events do complete it.

Owner red/green check: deleting only that arm made the new test fail at the matching-success assertion; restoring production made it pass. All five retired-wait tests and twenty presenter_rename tests passed; scoped fmt and locked all-target pr-app Clippy passed. The one-mutant cargo-mutants recheck at source `9f644fbf82fbdd7ef8882a9c72f979deb9832dd7`, selected with zero-based `--shard 6/18 --sharding slice`, caught that exact deletion in 59 seconds (exit0, no budget expiry).

This is **22 unique mutants selected out of 140 candidates in the two files**, with 23 executions including the survivor recheck. Initial runs caught21 and missed1; after the added regression, all22 selected mutants are caught. **118 candidates were not run** (13 suffix-file and105 state-file candidates). This is not a whole-file, whole-unit, native UI, decoder, RAW corpus or performance verdict. Native/FFI/composition glue is excluded.

All runs used `-j 1 --baseline skip --timeout 90 --test-package pr-app --cargo-arg=--lib`, one external target reused sequentially for this unit, and a cumulative 900-second outer unit budget. Remaining budget before lifecycle was645s and before recheck202s; no run expired. Selected JSON inventories and outcomes are retained locally. Final combined source validation is appended after the integration gate.

## Local evidence hashes

Artifacts live under `%TEMP%`; these hashes identify the retained inventory, results and mutation outcomes.

| Run | Artifact | SHA-256 |
|---|---|---|
| suffix | result.txt | A50A4E9F9F1B2BB938199B4982D351DC1506E0FADFFB24369C42CC16093615E2 |
| suffix | outcomes.json | C6F97A8354EDAB11825419EF2A1A05188B74979DE0AAAE5163A3D0BC9272189A |
| suffix | goal-r6-suffix-mutation-selected.json | F8BB57B0EFE8291828D62E9FCA5435201A5DF917FE73902E2C0C7F2CF711319C |
| lifecycle | result.txt | 9BF6F3761977892DA395350D4269E8D22F0BEB5C69B6F3EA2316DD560A826798 |
| lifecycle | outcomes.json | 7028C25ABC35B0440FAF0F760279F837231CD64FEC779BD5F7BD9CD217A5B33C |
| lifecycle | goal-r6-lifecycle-selected.json | D6DFBB9D302AD7BD27AB68FF15204FBFC4082D829613D4CA53C3F275414E1234 |
| dimensions-recheck | result.txt | AFB98EE18B7A0E944E793EE730E4D520308F343ECC2F79998A37F43D0BBC2F5D |
| dimensions-recheck | outcomes.json | AFDB315CA94E2A73E767F32A7BBA5325044D1CCE6A5E3C191F16DC0776473457 |
| dimensions-recheck | goal-r6-dimensions-recheck-selected.json | C507F888C6380C747765335EA5B890B565A8F1CE1EA65A9F6A7E52C3F31DEDFA |
