# W9-R1 selected postmerge mutation evidence

Verified 2026-10-27 from the original completed run; no restart.

- Source: merged master `1c81af8c617532de8c870d778d66db6259be4233` (PR #275).
- Owned file: `crates/pr-core/src/file_actions/rename.rs`.
- Selected function: `RenameNamePolicy::exceeds_path_budget`; test binary `rename_policy`.
- Run: `--baseline skip --timeout 90 -j 1`, unique external target, 900-second outer guard.
- Terminal marker: `END mutant exit=0 budgetExpired=False`.
- Runner summary: `12 mutants tested in 5m: 12 caught`.
- Scope: 12 selected of 97 mutants enumerated for the file; caught 12, missed 0, unviable 0, timeouts 0. The other 85 were not run. This is a selected-function verdict, not a full-file mutation verdict.
- Source files and tests were not changed for this report. No native filesystem, RAW corpus, or desktop evidence is implied.

The completed local evidence lives under `%TEMP%/w9-r1-mutation-4a126939`; hashes below identify it without requiring that ephemeral directory to remain available.

| Evidence file | SHA-256 |
|---|---|
| `result.txt` | `241274B1829FF870AD24F83DC857D3C51CC4FB519893036A8FD1EE5AEF4A72AE` |
| `selected.json` | `62578F75DF4F176FBA8F2486E3FBCC172384CE036E1DCDF848B6D562D65B3873` |
| `mutants.out/outcomes.json` | `115A851A7ACF9B619DB58D48A94838466B1C48D9C5F5C208D69C7E44FE64A602` |

| Mutant | Verdict |
|---|---|
| `crates/pr-core/src/file_actions/rename.rs:200:9: replace RenameNamePolicy::exceeds_path_budget -> bool with true` | CaughtMutant |
| `crates/pr-core/src/file_actions/rename.rs:200:9: replace RenameNamePolicy::exceeds_path_budget -> bool with false` | CaughtMutant |
| `crates/pr-core/src/file_actions/rename.rs:206:95: delete ! in RenameNamePolicy::exceeds_path_budget` | CaughtMutant |
| `crates/pr-core/src/file_actions/rename.rs:214:13: replace + with - in RenameNamePolicy::exceeds_path_budget` | CaughtMutant |
| `crates/pr-core/src/file_actions/rename.rs:214:13: replace + with * in RenameNamePolicy::exceeds_path_budget` | CaughtMutant |
| `crates/pr-core/src/file_actions/rename.rs:213:13: replace + with - in RenameNamePolicy::exceeds_path_budget` | CaughtMutant |
| `crates/pr-core/src/file_actions/rename.rs:213:13: replace + with * in RenameNamePolicy::exceeds_path_budget` | CaughtMutant |
| `crates/pr-core/src/file_actions/rename.rs:215:34: replace && with || in RenameNamePolicy::exceeds_path_budget` | CaughtMutant |
| `crates/pr-core/src/file_actions/rename.rs:215:16: replace >= with < in RenameNamePolicy::exceeds_path_budget` | CaughtMutant |
| `crates/pr-core/src/file_actions/rename.rs:215:44: replace > with == in RenameNamePolicy::exceeds_path_budget` | CaughtMutant |
| `crates/pr-core/src/file_actions/rename.rs:215:44: replace > with < in RenameNamePolicy::exceeds_path_budget` | CaughtMutant |
| `crates/pr-core/src/file_actions/rename.rs:215:44: replace > with >= in RenameNamePolicy::exceeds_path_budget` | CaughtMutant |
