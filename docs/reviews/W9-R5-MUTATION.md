# W9-R5 selected postmerge Undo identity mutation

Verified 2026-10-27. Identity production source is merged PR #284, `da42ca1e828f22982c338921edc48292b652e2c8`. The selected core source and focused test file are unchanged from that master. The same follow-up adds the missing C# catalog-conflict diagnostic in app glue, outside the selected mutation file; that logging change is validated by its separate source gates.

- File: `crates/pr-core/src/file_actions/undo.rs`; selected pure functions `UndoActionIdentity::eq` and `UndoService::last_undo_action_identity`.
- Three unique mutants tested: equality always true, equality always false, current identity always None.3caught/0missed/0unviable/0timeout, exit0 in about2minutes. File total199;196other mutants NOT RUN. No full-file/full-Undo/full-R5 mutation verdict.
- Test package `pr-app`, binary `file_action_rename` (15fake tests); `--cargo-arg=--test=file_action_rename` constrains both build and test to that binary. Actual phase argv and logs were inspected: successful builds followed by assertion failures. The new identity regression distinguishes two equal-valued registrations and checks retained identity after the current slot is cleared; controller tests also guard rename versus later Move ownership.
- The installed tool includes struct-field deletion candidates outside the `-F` function match. Its validated `--shard 0/9 --sharding slice` selection contains exactly the three named identity mutations. No unrelated Move/Recycle fields were mutated. A generated `Some(Default::default())` candidate is among the unrun196, not an asserted test success.
- Run used baseline skip, timeout90, -j1, an owned external target, jobs2, no incremental/debug line tables and a900-second outer guard; the budget was not exhausted. Tests use FakeFileSystem/FakeClock/FakeRecycleBin and TempRoot paths. No native desktop/decoder/RAW corpus or real Recycle Bin verdict.

## Local evidence

Directory: `%TEMP%/goal-r5-identity-d2a81a32`. Hashes identify the files independently of that temporary location.

| File | SHA-256 |
|---|---|
| `result.txt` | `04FE35343F33C464A00CE123D841FAE2F908FED18C935E0037F3D13D2488B3F8` |
| `selected.json` | `ADAC2FAB735E0272D3433DB49FEC6DECA7C5C2ED3D8A0BD03884BE0D66125B4B` |
| `mutants.out/outcomes.json` | `027374A4939B4CA5CDD677009BCB462C60F29A06F4BF94A9136C53E93FA8F5E0` |

| Mutant | Verdict |
|---|---|
| `crates/pr-core/src/file_actions/undo.rs:206:9: replace <impl PartialEq for UndoActionIdentity>::eq -> bool with true` | CaughtMutant |
| `crates/pr-core/src/file_actions/undo.rs:206:9: replace <impl PartialEq for UndoActionIdentity>::eq -> bool with false` | CaughtMutant |
| `crates/pr-core/src/file_actions/undo.rs:349:9: replace UndoService::last_undo_action_identity -> Option<UndoActionIdentity> with None` | CaughtMutant |

## Follow-up source gates (2026-10-28)

Integrated benchmark fixture repair #285 into source head 776f12ee (base cf325668). The earlier R5 follow-up workspace run failed two real-race benchmark mapping tests; that failure was preserved and fixed in its own PR before this gate. No production sharing-mode cause is claimed.

Format passed through the 12-package Windows command-length fallback; locked workspace all-target Clippy passed; locked workspace tests: 10012 passed, 0 failed, 1336 ignored (570 result lines). Pre-pr passed; its downstream workspace Clippy skip is covered by the independent full workspace invocation. The selected core identity source and focused test file are unchanged from da42ca1e; artifact hashes and all three outcomes were reverified. The added catalog-conflict log follows the pinned C# diagnostic; it does not change catalog mutation or retries.
