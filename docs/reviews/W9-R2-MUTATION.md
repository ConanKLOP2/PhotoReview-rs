# W9-R2 selected postmerge mutation and fingerprint regressions

Verified 2026-10-27. Production source is merged PR #278, `2825fbd1b2fea454fa1b07115a7071a11a217460`; this follow-up changes only tests and documentation.

## Scope and results

- Owned file: `crates/pr-core/src/file_actions/service.rs`; function `FileActionService::is_rename_untouched`.
- File enumeration: 277 mutants. Selected function: 9 unique mutants; 268 other file mutants were not run. This is not a full-file mutation verdict.
- Initial run: 9 tested, 6 caught, 3 missed, 0 unviable, 0 timeouts; exit2, budget not exhausted, about3minutes.
- Added two table-driven tests (four fake scenarios): source length-only/timestamp-only changes stay Failed; an equal-length target with the same timestamp stays Failed, while a different target timestamp proves the source was untouched and permits Dismissed.
- Rerun of the same9: 9 tested, 9 caught, 0 missed, 0 unviable, 0 timeouts; exit0, budget not exhausted, about3minutes. Across both rounds there were18 executions of9 unique mutants.
- Focused baseline after the new tests:25passed,3native ignored. No native test or real Recycle Bin operation ran during mutation.
- Both rounds used `--baseline skip --timeout 90 -j 1`, an external target owned by this unit, the `rename_service` test binary, and900-second outer guards (about6minutes total actual run time).

## Selection and reproduction

On cargo-mutants27.1.0, listing this file with `-F FileActionService::is_rename_untouched` alone returned35 mutants, including26 struct-field mutants outside the function. The selected scope was therefore verified with both the function regex and the zero-context hunk containing this helper from the actual `6169bd87..2825fbd1` PR diff. The resulting list contains exactly9 mutants, all owned by this helper. The supplied diff is a selection filter, not another source edit.

```text
cargo mutants -p pr-core --baseline skip --timeout 90 -j 1 --file '**/file_actions/service.rs' -F 'FileActionService::is_rename_untouched' --in-diff <helper-pr-hunk.diff> --output <owned-output> -- --test rename_service
```

Local original evidence is under `%TEMP%/goal-mutation-r2-96f39af7`, rerun under its `rerun/` directory. Fingerprints below identify these temporary artifacts; the selected verdict table preserves their essential content.

| Artifact | SHA-256 |
|---|---|
| `scope.diff` | `11BCBA2E170F8DD07B74A747614E8D3814A81FBED1DF38520331408FABFBC6D8` |
| `selected.json` | `A49F74D50ABB8C1D56880F601BAAE82C5376B8E78622DC5753ADF5AE94480C55` |
| `result.txt` | `5639F56F18075F58AB6DFD155F0162645D7A498AF2840AAB688E540362CA6989` |
| `mutants.out/outcomes.json` | `1D8C2716977A5B94802D12553D7654BB76333DFD0C648DA72FCA1446BC288D7B` |
| `rerun/result.txt` | `7EAED2EF64C054D3BA871865D1DF4668015A4A5A76AF87C055200548D3A528FE` |
| `rerun/mutants.out/outcomes.json` | `8E27AAD42300DE9890E212064209F6AFC6276B823BF91FBE29C92866FDD40040` |
| `rerun/test-regressions.diff` | `9ADEA1F3535B0C340122661233B8BA7932BB5CCEB80D50CA2057F30A33251220` |
| `rerun/test-sha256.txt` | `0692D287E351AF3E50EF686667FBCC3FDFB3778C0826342679017C5FE7DCAE6A` |

| Mutant | Initial | Rerun |
|---|---|---|
| `crates/pr-core/src/file_actions/service.rs:1335:9: replace FileActionService::is_rename_untouched -> bool with true` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/service.rs:1335:9: replace FileActionService::is_rename_untouched -> bool with false` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/service.rs:1337:31: replace match guard length_is(&stat, size) && stat.last_write_utc == last_write_utc with true in FileActionService::is_rename_untouched` | MissedMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/service.rs:1337:31: replace match guard length_is(&stat, size) && stat.last_write_utc == last_write_utc with false in FileActionService::is_rename_untouched` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/service.rs:1337:54: replace && with || in FileActionService::is_rename_untouched` | MissedMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/service.rs:1337:77: replace == with != in FileActionService::is_rename_untouched` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/service.rs:1342:78: replace || with && in FileActionService::is_rename_untouched` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/service.rs:1342:52: delete ! in FileActionService::is_rename_untouched` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/file_actions/service.rs:1342:103: replace != with == in FileActionService::is_rename_untouched` | MissedMutant | CaughtMutant |
