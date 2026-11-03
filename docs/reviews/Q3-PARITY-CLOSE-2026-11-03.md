Status: evidence report, task Q3 (started 2026-11-03, finished 2026-11-04 local time). Rows: F-13, X-physical-fs, AR16 (two PARITY rows), F-21, X-stj-reader. Proposed statuses are in the last section; `docs/PARITY.md` was NOT edited (the lead does it).

# Q3: parity close for the journal and physical file system cluster

Worktree `.claude/worktrees/pc-journal-fs`, branch `parity/journal-fs` from `origin/master` `58aad090`. C# baseline `e7c2fcd8` (read-only, `git -C ../PhotoReview show`). Machine: Windows 10 19045, 12 logical CPUs, 32 GB (about 4 to 8 GB free during the run, other agents building), `CARGO_BUILD_JOBS=2`, one cargo-mutants process with `-j 1` per target dir, at most four at a time (each its own `CARGO_TARGET_DIR`). Tool: cargo-mutants 27.1.0 through `tools/dev/mutants.sh` (the preflight `running N tests` was seen for every run; libtest flags use the `--cargo-test-arg=--` form, `--native` where `#[ignore]` tests matter). Rule applied (P-65): a mutant counts as caught only when a non-golden test fails; a harness TIMEOUT is not caught.

## 0. What was run (summary)

| What | Result |
|---|---|
| `bash tools/dev/native-tests.sh pr-core` (all 19 binaries with native tests, `--include-ignored --test-threads=1`, private data root) | 557 passed, 5 failed in 5 binaries. All 5 are machine/OS-dependent, see section 1. The rows' own native binaries pass: `file_actions_journal` 83, `file_actions_journal_compaction` 30, `io_journal_compaction_files` 15, `journal_reconcile` 56, `journal_startup_recovery` 30, `file_actions_partial_cleanup` 16, `file_actions_service_native` 18, `io_physical` 63 of 64, `io_physical_seams` 3, `w2_physrest_killers` 2, `w2_physrest2_killers` 7, `w2_fs_mutation_killers` 17 |
| New tests | 11 tests in 3 new files plus one test in `viewmodels_main_commands.rs` and a hang guard in `w2_phys5_killers.rs` (section 7) |
| `cargo clippy -p pr-core --all-targets --locked -- -D warnings` and `-p pr-app` | both exit 0 (second run, after the last test edit) |
| rustfmt | `cargo fmt -p` fails with os error 206 on these long paths; `rustfmt --edition 2024 --check` on the 5 changed or new test files: clean (one file was reformatted by `rustfmt`) |
| Mutation, total (all `-j 1`, own target dirs) | about 330 mutant runs; per-group tables below |

## 1. Native test failures of this machine (not caused by the rows' code)

Each was rerun alone and fails the same way. None touches a mutated line.

| Test | Failure | Reason |
|---|---|---|
| `io_physical::resolve_real_path_dangling_chain_of_63_junctions_resolves_to_the_missing_target` | `resolve_real_path` returns OS error 1921 (`FilesystemLoop`) after about 50 s | OS-dependent: this Windows 10 build refuses a chain of 63 junctions before the manual walk starts. Already seen by `SEC01-MUTATION-2026-11-03.md`. The 63-hop cap itself is covered by the pure seam tests (`follow_link_chain_for_tests`). Needs a person to run the test on the Windows build the owner targets (Windows 11), or the test needs a smaller chain; not changed here |
| `file_actions_recovery_check::physical_access_denied_source_is_missing_and_lost_as_in_csharp` | precondition `symlink_metadata` of the denied file still succeeds | the session has a privilege that bypasses ACL denies (backup/admin token); needs a normal non-elevated desktop session |
| `rename_service::native_case_sensitive_directory_reports_two_case_variants`, `rename_file_action_matrix::case_only_rename_native_two_different_files_in_case_sensitive_folder_is_refused` | `The request is not supported` when setting per-directory case sensitivity | the WSL/case-sensitive-directory feature is not available on this machine |
| `session_store::golden_session_key_cases` | `C:\photos\CON` becomes `\\.\CON` here, the golden expects `C:\photos\CON` | `GetFullPathName` of reserved device names differs between Windows 10 and the OS of the oracle run |

## 2. F-13 (append robustness, `OperationJournal::append`)

**Gaps before** (PARITY text): (a) "journal_line/journal_rules/recovery_retry/transaction/compaction W3-01 counts were not rerun"; (b) 32 `recovery_retry` glue mutants not run; (c) DV-5 branch `pick_and_open_folder` has no dedicated test; (d) native evidence for this run; (e) the Different note (T19/DV-5, unpaired surrogates refused at load time).

**What ran**
- Read `GLUE-MUTANTS-2026-11-03.md` (PR #619, in this base): `recovery_retry.rs` 77 glue mutants 68 caught, 1 known equivalent, 8 unviable, 0 NOT RUN; `journal_line.rs` 5 glue mutants: `Hash for JournalDateTime` killed by `journal_line_hash_killers`, 3 `expecting` equivalent (unobservable), 1 unviable. With `RECOVERY-RETRY-MUTATION*` and `JOURNAL-LINE-MUTATION*` every viable mutant of `recovery_retry.rs` (159 listed) and `journal_line.rs` (141) has a verdict. So (b) is closed on paper; I did not rerun those two files.
- Native: `file_actions_journal` 83, `file_actions_journal_compaction` 30, `io_journal_compaction_files` 15, `journal_reconcile` 56, `journal_startup_recovery` 30: all pass in this run.
- Mutation of the F-13 functions themselves, rerun in this run: `journal.rs` `-F 'append_lines|open_append_with_retry|tail_lacks_newline'`, 27 mutants, tests `file_actions_journal`, `journal_mutation_killers{,_2,_3,_4}`, `w3_jrnl_killers5..8` (`-- --include-ignored --test-threads=1 --skip random_history`, preflight `running 116 tests`): **27 of 27 tested: 21 caught, 2 unviable, 0 missed, 4 TIMEOUT** (`665:16` delete `!`, `700:29` guard `true`, `700:55` `&&`->`||`, `702:29` `+=`->`*=`: hang mutants, NOT counted as caught).
- Fail-fast killer for those 4 (new `crates/pr-core/tests/q3_journal_append_hang_killers.rs`, 3 tests, fake file system, the `append_retry_delay` seam panics when the retry loop exceeds the C# bound): rerun of the same lines with only that binary, 10 mutants selected by the filter, **6 caught** including all four former timeouts (`665:16`, `700:29` true and false, `700:55`, `702:29` `+=`->`*=` and `-=`), the 4 other selected ones are the known `dismiss` field deletions that this narrow binary does not test (caught by the full suite in CL-JRNL4). So the 27 are now: 25 caught (21 + 4), 2 unviable, 0 missed, 0 timeout.
- DV-5 branch test (new): `pr_app::viewmodels_main_commands::pick_and_open_folder_refuses_a_folder_whose_name_cannot_be_represented` (Windows only): a real folder is named with U+FFFD, the picker returns the same path with a lone surrogate instead; nothing may open. Passes; RED check: replacing the refusal in `commands.rs` by `to_string_lossy()` makes it fail (`left: 1, right: 0`); production restored (`git status` shows no `src` change).

**Gaps after**: the whole-file `journal.rs` count (179 identities, 153 caught, 1 equivalent, 25 unviable, CL-JRNL4 on 2026-10-31) was not rerun in this run (27 of 179 were); `journal_rules.rs` has no mutation report at all (it is not part of the F-13 function set); `journal_line`/`recovery_retry` counts are those of the 2026-11-03 reports (read, not rerun). Intentional difference DV-5 stays (accepted ADR `ADR-D-2026-10-31-intentional-differences-from-fix-lanes.md`, "the owner may reverse any item").

## 3. X-physical-fs (`PhysicalFileSystem`, `physical.rs`)

**Gaps before**: about 227 of 438 mutants never run with the right flags; real survivors at `is_device` :490-:496, `ensure_extended_prefix` :501, `_if_needed` :513, `create_directory_impl` :678; native passes 2 to 7 "not trustworthy" (old flag form).

`cargo mutants --list` of the file gives 440 identities now.

**Native**: `io_physical` 63 passed, 1 failed (section 1, OS limit), `io_physical_seams` 3, `w2_physrest_killers` 2, `w2_physrest2_killers` 7, `w2_fs_mutation_killers` 17; the killers `w2_phys2..5` pass in every preflight (29 tests with `--native`).

**Mutation: group `create_directory_impl|is_device|ensure_extended_prefix` (63 identities), every identity now has a verdict**

| Run | Tests | Result |
|---|---|---|
| A: 63 found, budget 3000 s | `w2_phys2..5_killers`, `w2_physrest_killers`, `w2_physrest2_killers`, `io_physical_seams` (`--native`, preflight 29 tests) | 52 tested: 34 caught, 18 missed, 0 unviable, 0 timeout; 11 NOT RUN by budget |
| E: the 11 not run | `w2_phys5`, `w2_physrest2`, `w2_phys4`, `w2_physrest` | 11 tested: 2 caught, 1 unviable, 6 missed, 2 TIMEOUT (`696:15` `-=`->`+=` and `/=`) |
| C2: 25 selected survivors | `io_physical` filtered `long_path_full_lifecycle create_directory` (preflight 4 tests) | 4 caught (`is_device -> true`, `501:8` delete `!`, `683:15` `>`->`==` and `<`), 21 missed |
| G1: 12 selected | `io_physical` filtered `create_directory` | 4 caught (`709:35`, `721:14`, `721:22`, `723:12`) |
| G3 (after the hang guard below): `696:15` | `w2_phys5_killers` alone | 2 of 2 caught (the two E timeouts) |
| I: 15 selected, new killers | `q3_physical_mkdir_killers` | 7 caught (`678:16` `+=`, `683:15` `==` and `<`, `686:41`, `693:21` `==` and `<`, `709:35`) |

Union over the runs (done by a script over the six `outcomes.json` files): **63 identities = 45 caught, 1 unviable, 17 survivors, 0 NOT RUN, 0 timeout**. The 17 survivors, none killable through the public API, all classified equivalent by analysis (not proven by a mutant):
- `is_device` 491:39 `==`->`!=`, 492:16 `<`->`==` and `<=`, 495:16 / 495:29 / 495:63 and 496:27 / 496:43 / 496:59 `&&`->`||` (9 identities of `is_device`): `is_device` is only called by `ensure_extended_prefix` after `is_path_fully_qualified`, so it sees a drive path (`X:\`, where the second unit is `:`) or a UNC path. Each mutant makes `is_device` true for a UNC path whose server name starts with `?` or `.` or has one character (`\\a\share`), or for the 4-character path `\\?\`. The only effect is that `ensure_extended_prefix` does not add `\\?\UNC\`. That matters only for a path of 260 units or more that is handed to the raw `MoveFileExW`/`CopyFileExW` (`win32::wide`), and `move_impl`/`copy` check `file_exists_full` of the source first (a UNC server like that does not exist), and `create_directory` goes through `std::fs::create_dir`, which adds its own verbatim prefix for long paths. Not reachable without a network share with such a name.
- `ensure_extended_prefix_if_needed` 513:43 `||`->`&&`, 513:36 `>=`->`<`: every use is a `std::fs` call (`OpenOptions::open`, `symlink_metadata`, directory handle), and `std` adds the verbatim prefix to long paths itself; trailing `.`/space paths are already trimmed by `get_full_path` (device paths keep them and are `is_device`). Same observable result with and without.
- `create_directory_impl` 677:15 `>=`->`<`, 678:16 `-=`->`/=`: the trailing separator is not trimmed, so the same folder is created twice; the second `CreateDirectory` returns "already exists" for a directory, which the function treats as success. 683:15 `>`->`>=` and 693:21 `>`->`>=`: the loop condition differs only at `i == root_length`, where both forms end up at the same slice. 721:19 `&&`->`||` (`count == 0 || !some_path_exists`): the extra case (`count > 0` with no existing ancestor) reaches the root check, which returns the same raw 3 as the creation error. 730:22 guard `!last_ok`->`true`: differs only when an intermediate level failed while the last one succeeded (a race).

Killers added for the real gaps in this group: `q3_physical_mkdir_killers.rs` (4 native tests, bounded worker thread): no prefix folders are created (`693:21` `<`/`==`), only the missing levels are created below an existing folder, a trailing separator, a missing drive returns raw 3 (`686:41`); and the hang guard in `w2_phys5_killers::create_directory_device_forms_keep_trailing_dot_and_other_forms_trim_it` (it sorted after the guarded test and hung the whole binary under the `696:15` mutants, which is why CL-PHYS5 saw "3 timeouts").

**Other `physical.rs` groups**: the listing group was rerun (section 4). The whole-file remainder was NOT rerun in this run: of the 440 identities, 63 + 21 + 11 + the `match_pattern`/`translate_win32_expression`/decoder/retry/seam groups of CL-PHYS3/4/5, CX-PHYS2 and CL-PHYSRERUN are only known from their reports. Groups never run with correct flags in any report (counts from `--list`): `resolve_if_reparse_point` 3, `win32_file_exists` 3, `win32_directory_exists` 3, `open_read_shared_file` 4, `win32::move_file` 2, `win32::wide` 1, `win32::check` 1, `move_impl` 1, `attributes_of` 2, `new_guid_n` 2, `flush_temp_file` 2, `is_stat_missing` 1, `is_transient_replace_failure` 1, `open_like_file_stream` 5, `final_link_target_with` 2, `immediate_link_target` 3: at most about 36 identities, several of them also covered by SEC01-MUTATION and CL-PHYS3. I could not separate them exactly from the reports.

**Gaps after**: the whole-file mutation count is not complete or reproduced in this run (63 + 32 of 440 rerun); one of the 64 native tests fails on this OS (section 1); no native CI job exists (the killers are in `native-nightly`, unconfirmed until one night); 17 reasoned equivalents.

## 4. AR16 (two PARITY rows)

### 4a. AR16 `PhysicalFileSystem::enumerate_files_with_stat`

**Gaps before**: "native mutation rerun incomplete; listing group inventory 24, 13 not rerun". PHYSICAL-LISTING-MUTATION-2026-11-03 closed it on paper; this run reproduces it.

**Ran**: `listing_root|list_entries|open_physical_listing|PhysicalFileSystem>::enumerate_(files|files_with_stat|directories)` with the four killer binaries (`w2_physrest`, `w2_physrest2`, `w2_phys4`, `w2_phys5`, `--native`, preflight `running 19 tests`): **21 of 21 tested: 19 caught, 0 missed, 2 unviable, 0 timeout**. Seam `enumerate_files_with_stat_from` with `io_physical` (preflight ok): **11 of 11: 10 caught, 0 missed, 1 unviable, 0 timeout**. Identical to the report. Native `io_physical` ListingInterrupted tests (`enumerate_files_with_stat_*`) pass in the native run (the only failing test of that binary is the junction-chain test, section 1).

**Gaps after**: none for this function set. The wider `physical.rs` count is X-physical-fs's job.

### 4b. AR16 (RemovePaths) `ReviewCatalog::remove_paths`

**Gaps before**: "the change in the 29-30/10 window is not confirmed" against C#; mutation figure from 2026-11-03.

**Ran**: compared `ReviewCatalog.RemovePaths` of C# `e7c2fcd8` (`src/PhotoReview.Core/Catalog/ReviewCatalog.cs:467`) line by line with `ReviewCatalog::remove_paths` (`review_catalog.rs:608`): same blank filter, case-insensitive doomed set, capture degrade in place (standalone survivor, `CatalogEntry(survivor)` for a non-representative survivor), removal order, `next_current` rule. `git log` of the C# file: `RemovePaths` came in with `67c4502e` (2026-09-26); the only later commit touching the file is the rename PR #529 (2026-10-15) which does not change `RemovePaths`; no commit touched `Catalog/` or `FolderLoadCoordinator.cs` between 2026-10-28 and 2026-11-03. `catalog_review_catalog`: 66 passed (8 `remove_paths_*`). Mutation rerun (`-F 'ReviewCatalog::remove_paths'`, `catalog_review_catalog`, preflight `running 66 tests`): a first run with a 900 s budget on the loaded machine tested 18 of 24 (15 caught, 1 missed, 2 unviable) and stopped; the second run (1200 s) completed: **24 of 24 tested: 20 caught, 3 missed, 1 unviable, 0 timeout**, identical to the 2026-11-03 report. The 3 missed are `615:30` `||`->`&&` and `673:31` `<`->`<=` and `<`->`==`, all reasoned equivalent in that report (early-return shortcut; `old_current == 0` yields `next_current == 0` anyway; `old_current == -1` with a non-empty catalog is unreachable through the public API).

**Gaps after**: the 3 reasoned equivalents of the 2026-11-03 report (`615:30`, `673:31` `<=` and `==`); `FolderLoadCoordinator` caller side is another row.

## 5. F-21 (COPY-PARTIAL-01, `partial_cleanup`)

**Gaps before**: the row is already Done. The task listed it for the "owner decision" part (`PARITY-GAP-MAP`: "product decision COPY-PARTIAL-01").

**Ran**: the decision is already made in C#: `docs/refactoring/decisions/COPY-PARTIAL-01.md` and `docs/architecture.md:126` at `e7c2fcd8` ("decision c": a Copy that throws after writing part of the destination keeps the partial file, the app never deletes it, Recovery shows a Conflict); Rust implements exactly that (the service calls `remove_partial_copy` only with a raised creation proof). Native: `file_actions_partial_cleanup` 16 passed, `file_actions_service_native` 18 passed. Mutation `partial_cleanup.rs` (`--native`, `file_actions_partial_cleanup`, preflight ok): **14 of 14: 13 caught, 1 missed (`32:20` `source_size > 0` -> `>= 0`, equivalent: `stat.length < 0` is never true for a `u64`), 0 unviable, 0 timeout.** Service side `-F 'single_failure|remove_partial_copy'` with `file_actions_service` + `file_actions_service_native` (46 found, budget 1500 s): 35 tested = 22 caught, 12 missed, 1 unviable, 11 NOT RUN. The F-21 lines are caught: `625:12` (`!prepared` guard) and the proof / destination-created branch. The 12 missed are `delete field` mutations of result fields and the `dismiss` rename rule (`641`, `642`, `683`), not F-21. `remove_partial_copy -> ()` (`1310:9`) was in the NOT RUN part (the cargo-mutants filter does not narrow, P-111); applied by hand: `file_actions_service` fails (`single_copy_verification_fails_own_incomplete_destination_is_removed` FAILED, 90 passed 1 failed) and `file_actions_service_native` passes 18, so it is caught by a non-golden test. Source restored (no `src` diff).

**Gaps after**: none for F-21. Decision recorded in C#, no new owner decision needed; the Rust side keeps `PROPOSED-DECISIONS` P-02 (group rollback / panicking file system).

## 6. X-stj-reader (`pr_core::settings::app_settings::json`)

**Gaps before**: the 304 mutants other than the 8 `*=` cursor loops not rerun; the older figures come from before the golden-infrastructure fix (`settings_app_settings` uses golden files, P-65); 8 compile-unviable return replacements.

`cargo mutants --list` of the file: 312 mutants at lines 1090 and up (`mod json`), as in CX-JSON4.

**Ran**: tests `settings_app_settings`, `w2_json_killers{,2,3,4}`, `w3_stj_timeout_killers` (preflight `running 36 tests`), budget 2400 s: **39 of 312 tested: 32 caught, 0 missed, 4 unviable, 3 TIMEOUT; 273 NOT RUN**. The three timeouts are `+= -> *=` cursor loops at `1146:26`, `1166:47`, `1179:53`; in the combined run the first test binary hangs on them before the guarded binary runs. Resolved separately with the six mutants of those three locations (`+= -> *=` and `-=`): `w3_stj_timeout_killers` alone: 4 caught, 2 missed (`1166:47` `-=` and `*=`, the line-comment loop no earlier killer reached; this is the "1 skip_ws timeout" of CX-JSON4); the new `q3_stj_line_comment_killers` (4 tests, same bounded-wait pattern) alone: 4 caught (`1146:26` x2, `1166:47` x2), 2 missed (`1179:53` x2, which `w3_stj_timeout_killers` catches). Together all 6 are caught by a non-golden test, each in about 10 s or less, no harness kill.

**Gaps after**: 273 of 312 not rerun in this run (about 35 s per mutant, about 3 hours at `-j 1`); the 8 compile-unviable return replacements cannot run and stay as such; the CX-JSON4 counts predate the golden fix and are unverified for the golden-using test binary. Command for the person or the lead to run overnight:

```
TEMP=<dir> MUT_OUT=$TEMP/out MUT_TIMEOUT=120 bash tools/dev/mutants.sh pr-core crates/pr-core/src/settings/app_settings.rs \
  --tests settings_app_settings,w2_json_killers,w2_json_killers2,w2_json_killers3,w2_json_killers4,w3_stj_timeout_killers,q3_stj_line_comment_killers \
  --budget 14400 --jobs 1 -F 'app_settings\.rs:(109[0-9]|1[1-6][0-9][0-9]):'
```

(a TIMEOUT in a combined run is a test-ordering artifact; rerun it with `w3_stj_timeout_killers` and `q3_stj_line_comment_killers` alone).

## 7. Files changed

| File | Change |
|---|---|
| `crates/pr-core/tests/q3_journal_append_hang_killers.rs` | new, 3 tests (F-13) |
| `crates/pr-core/tests/q3_physical_mkdir_killers.rs` | new, 4 native tests (X-physical-fs) |
| `crates/pr-core/tests/q3_stj_line_comment_killers.rs` | new, 4 tests (X-stj-reader) |
| `crates/pr-core/tests/w2_phys5_killers.rs` | the device-form test runs its `create_directory` calls on a worker thread with a bounded wait (turns the `696:15` hangs into failures) |
| `crates/pr-app/tests/viewmodels_main_commands.rs` | new test `pick_and_open_folder_refuses_a_folder_whose_name_cannot_be_represented` (DV-5, F-13) |
| `docs/reviews/Q3-PARITY-CLOSE-2026-11-03.md` | this file |

No production code was changed (`productionChanged: false`). No defect was found by the new tests.

## 8. Not run, and runs that were started but gave nothing usable

- `X-stj-reader`: 273 of 312 mutants (section 6). `X-physical-fs`: the whole-file count (section 3). Whole-file `journal.rs`: 152 of 179 (section 2).
- Workspace `cargo test --workspace --locked` and `cargo clippy --workspace`: not run (pre-pr was run with `--no-clippy`; clippy `-p pr-core` and `-p pr-app --all-targets -D warnings` were run separately, both exit 0). `pre-pr.sh` passed (fmt for both crates worked on this machine, no os error 206; conflict markers, BOM, unsafe-audit, doc-links ok).
- Two extra cargo-mutants processes on `service.rs` (budgets 900 s and 600 s) were started with a filter that does not narrow the batch (P-111: cargo-mutants listed 27 mutants for `-F 'FileActionService::remove_partial_copy'`, 26 of them struct-field deletions); they are not used as evidence, `remove_partial_copy -> ()` was applied by hand instead (section 5).
- Service side of F-21: 11 of 46 mutants of `-F 'single_failure|remove_partial_copy'` not run (budget): struct-field deletions in `group_failure` and `inspect_group_member`, and `remove_partial_copy -> ()` (done by hand).
