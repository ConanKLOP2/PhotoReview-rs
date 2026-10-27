# W9-R4 selected postmerge mutation and atomic-map regressions

Verified 2026-10-27. Production source is merged PR #280, `33bec6c411e0518a219d7fe34df8c818c59d9cd0`; this follow-up changes tests and documentation only.

- File: `crates/pr-core/src/catalog/rename.rs`; selected function `ReviewCatalog::rename_entries`, test binary `review_catalog_rename`.
- Selected8of22file mutants; initial6caught/2missed/0unviable/0timeouts, exit2, about3minutes.
- Added two regressions: a mixed valid/invalid map rejects every change atomically (four invalid rows), and an empty catalog returns without rebuilding its lazy index. Focused baseline32/32PASS.
- Rerun of the same8:8caught/0missed/0unviable/0timeouts, exit0, about3minutes. Total16executions of8unique mutants;14other file mutants NOT RUN. This is not a full-file mutation verdict.
- Both runs used baseline skip, timeout90, -j1, an owned external target and900-second outer guards; neither exhausted its budget. No production code, native filesystem behavior, RAW corpus or desktop evidence changed.

Local evidence directory: `%TEMP%/goal-r4-mut-b551ae14`. Hashes identify the completed evidence independently of its temporary location.

| Evidence file | SHA-256 |
|---|---|
| `result.txt` | `1626505741FE655173ACC9411133AB01C787878F61EF9F63E5E05DFBD4EA524D` |
| `selected.json` | `DED02817D81F7D9D4A2981D07C9EC69413ACC841CC24DFD28938B342989D593D` |
| `mutants.out/outcomes.json` | `2E67E3267EE1D750C902CE9F00811BAA64ED2C98DDF15921D96CF5D13443F7CA` |
| `rerun/result.txt` | `203F0513F281F5219542EFED603948888269150F5909D0A1427AA7B70AB3C5C9` |
| `rerun/mutants.out/outcomes.json` | `D90742D997D2646C14A42AE3D5F90325AF876AB2A377FA2DF0F6D399D642B6E4` |
| `rerun/test-regressions.diff` | `8B52E3A67C467C1E7A29CEB9A42D1FB18C720BB3B34E372FD4D476D45764E53E` |

| Mutant | Initial | Rerun |
|---|---|---|
| `crates/pr-core/src/catalog/rename.rs:18:9: replace ReviewCatalog::rename_entries -> bool with true` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/catalog/rename.rs:18:9: replace ReviewCatalog::rename_entries -> bool with false` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/catalog/rename.rs:19:31: replace || with && in ReviewCatalog::rename_entries` | MissedMutant | CaughtMutant |
| `crates/pr-core/src/catalog/rename.rs:27:54: replace || with && in ReviewCatalog::rename_entries` | MissedMutant | CaughtMutant |
| `crates/pr-core/src/catalog/rename.rs:68:16: delete ! in ReviewCatalog::rename_entries` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/catalog/rename.rs:73:64: replace && with || in ReviewCatalog::rename_entries` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/catalog/rename.rs:73:24: delete ! in ReviewCatalog::rename_entries` | CaughtMutant | CaughtMutant |
| `crates/pr-core/src/catalog/rename.rs:73:67: delete ! in ReviewCatalog::rename_entries` | CaughtMutant | CaughtMutant |
