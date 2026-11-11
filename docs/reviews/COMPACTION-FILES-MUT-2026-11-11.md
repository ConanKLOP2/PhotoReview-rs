# COMPACTION-FILES-MUT: mutation verdict for `io/journal_compaction_files.rs` (2026-11-11)

Status: evidence report (2026-11-11), CORRECTED after a flake analysis of the first result. The first version of this note said 81 caught and called the test-helper flake an open defect; both statements are replaced here. Production code is no longer unchanged on this branch: commit 55b8d6bc made `delete_file` best effort (see "Final source"). The const refactor was tried and reverted. Base: origin/master 844b6fbc. cargo-mutants 27.1.0. This closes the open item of [`TRANSACTION-RERUN-2026-11-10.md`](TRANSACTION-RERUN-2026-11-10.md) (no valid verdict for this file).

## Why the 2026-11-10 attempt gave no verdict
It ran with `--timeout 90`. The unmutated compaction binaries need about 150 to 190 s on this machine (`file_actions_journal_compaction` 161 s, `journal_startup_recovery` 107 s in today's run), so every mutant that reached them timed out (15 timeouts, 9 unviable, nothing caught). A timeout is not a catch. This run used `MUT_TIMEOUT=600`.

## Commands and baseline
`TEMP=...\mut-comp-<n> TMP=...\mut-comp-<n> MUT_TIMEOUT=600 bash tools/dev/mutants.sh pr-core crates/pr-core/src/io/journal_compaction_files.rs --tests io_journal_compaction_files,file_actions_journal_compaction,journal_startup_recovery --native --budget 7200 --jobs 1` in 3 shards (own TEMP each: `mut-comp-0`, `-1`, `-2`). `--native` gives `--include-ignored --test-threads=1`. Preflight (unmutated): 75 tests executed, ok. Raw outputs are under the user temp folder (`mut-comp-<n>/mut-out-pr-core-<id>/mutants.out/{caught,missed,unviable,timeout}.txt` and `log/`); they are not in the repository.

## Step 1: the first run (100 mutants, 0 NOT RUN)
| shard | caught | missed | unviable | timeout |
|---|---|---|---|---|
| 0 (34) | 22 | 2 | 10 | 0 |
| 1 (33) | 23 | 7 | 3 | 0 |
| 2 (33) | 27 | 6 | 0 | 0 |
| integration total | 72 | 15 | 13 | 0 |

The first version of this note added the 9 unit-test catches of the 15 missed to this 72 and got 81. The 72 was not safe, see step 2.

## Step 2: flake analysis of the 72 catches
The test helper `path_of_length` in `crates/pr-core/tests/io_journal_compaction_files.rs` builds a path of an exact length. For some temp-root lengths it produced 299 units instead of 300 (when the remaining length was exactly 202, the long-folder step left a remainder of exactly 1). The test `dispose_without_replace_deletes_temp_files_of_260_units_or_more` then failed on its 299-vs-300 assertion with no mutant involved. A mutant that only makes that test fail is not caught; it is a false catch.
The lead read the log of every one of the 72 caught mutants:
- 53 catches have at least one failing test whose panic is NOT the 299-vs-300 assertion. These are real catches.
- 19 catches failed ONLY in that test with the flake assertion. They were not verdicts: 209:9 (two variants, `""` and `"xyzzy"`), 225:9, 290:25, 290:34, 291:13, 292:24, 292:37, 292:46, 292:62, 293:13, 293:31, 293:39, 293:47, 293:63, 299:8, 312:54, 432:5, 448:11.

## Step 3: the helper fix and the rerun of the 19
Commit 9a95e069 fixes the helper: long folder names now take at most `left - 3` (was `left - 2`), so the remainder can no longer be exactly 1 when `left == 202`. Rerun of the 19 with the fixed helper (integration binaries, `--native`, `MUT_TIMEOUT=600`, preflight 75 tests ok, dir `mut-comp-0/mut-out-pr-core-Gh9zlp`): 7 caught, 12 missed, 0 unviable, 0 timeout. The 12 missed were rerun against the in-file unit tests (`cargo test -p pr-core --lib journal_compaction_files`, 9 tests, dir `mut-out-pr-core-splF8u`): 10 caught, 2 missed (432:5, 448:11).
- 432:5 is the body of the `#[cfg(not(windows))]` twin of `rename_by_handle`. It is not compiled on Windows (the 4th such body, after 349:5, 368:5, 457:5).
- 448:11 (`ok != 0` -> `==` in the Windows `delete_file`) was unobservable: its only caller, `PhysicalStagedReplacement::drop`, discarded the result (`let _ = delete_file(..)`). See "Final source".

## Step 4: the 15 originally missed
They were rerun against the in-file unit tests (the `-F` filter also matched sibling variants, so that rerun held 21 identities: 12 caught, 8 missed, 1 unviable). That rerun stands for the 15: 9 are caught by the unit tests, 3 are equivalent, 3 are not compiled on Windows.

## Every mutant that was not a plain integration catch
A plain integration catch is one of the 53 real catches of step 2 (not listed one by one) or one of the 13 unviable. The other 34 identities, with final status:
| mutant | first run | final | reason |
|---|---|---|---|
| 290:25, 290:34, 291:13, 292:46, 293:13, 299:8, 312:54 | caught by flake only | caught (integration, after the helper fix) x7 | step 3 |
| 209:9 x2, 225:9, 292:24, 292:37, 292:62, 293:31, 293:39, 293:47, 293:63 | caught by flake only | caught (unit tests) x10 | step 3 |
| 432:5 `rename_by_handle` body | caught by flake only | not compiled on Windows | `#[cfg(not(windows))]` twin |
| 448:11 `ok != 0` -> `==` (`delete_file`) | caught by flake only | removed by refactor | unobservable, the caller ignored the result; gone after 55b8d6bc |
| 231:9 `source` -> None | missed | caught (unit) | lib test checks `Error::source` |
| 290:39 `==`->`!=` is_device | missed | caught (unit) | lib test |
| 292:71 `&&`->`||` is_device | missed | caught (unit) | lib test |
| 293:22, 293:55, 293:71 `==`->`!=` is_device | missed | caught (unit) x3 | lib tests |
| 339:57 `&&`->`||` map_root_path_not_found | missed | caught (unit) | lib test |
| 382:28, 382:41 `+`->`*` rename_info | missed | caught (unit) x2 | lib tests |
| 329:37 `|`->`^` (FILE_SHARE_READ 0x1 `|` FILE_SHARE_DELETE 0x4) | missed | equivalent | disjoint bits, `|` and `^` give 0x5; argued from the values, no run proof |
| 360:36 `|`->`^` (GENERIC_WRITE `|` DELETE) | missed | equivalent | disjoint bits; same |
| 387:52 `|`->`^` (REPLACE_IF_EXISTS 0x1 `|` POSIX_SEMANTICS 0x2) | missed | equivalent | disjoint bits; same |
| 349:5 open_deny_writers | missed | not compiled on Windows | body of the `#[cfg(not(windows))]` twin, dead code here (same label as CL-MUT-BENCH3, CL-PHYS5) |
| 368:5 create_new_staged | missed | not compiled on Windows | same |
| 457:5 delete_file | missed | not compiled on Windows | same (now 453:5, see "Final source") |

The `|`->`&` variants of 329:37 and 360:36 show as missed in the lib rerun only because lib tests never open files; the integration shards CAUGHT both (with a real failing test).

## Final source (commit 55b8d6bc)
`delete_file` now returns `()` (best effort, both cfg twins) and the caller no longer discards a `Result`. No behaviour change. This removes 448:11. `cargo mutants --list` on the new source: 99 mutants; the `delete_file` entries are now 441:5 `replace delete_file with ()` (Windows) and 453:5 (non-Windows twin). Checks: fmt ok, `clippy -p pr-core --all-targets -D warnings` ok, `io_journal_compaction_files` 15 passed native. Mutation run of 441:5 and 453:5 (dir `mut-out-pr-core-MNg6ZY`, preflight 75 ok): 441:5 caught, 453:5 missed = not compiled on Windows (it replaces the old 457:5). Line numbers after 440 shifted by a few lines, so the identities in this note use the old numbering. Mapping: old 440:5 (`replace delete_file -> io::Result<()> with Ok(())`, one of the 53 real catches) is now 441:5 (same mutant, caught again) and old 457:5 is now 453:5, so the 99 identities are the original 100 minus 448:11.

## Final verdict
On the original 100 identities: 79 caught (53 real integration catches + 7 integration after the helper fix + 10 + 9 by the in-file unit tests, so 19 catches need the unit tests), 13 unviable, 3 equivalent, 4 not compiled on Windows (349:5, 368:5, 432:5, 457:5), 1 removed by the refactor (448:11), 0 timeout, 0 NOT RUN (79+13+3+4+1 = 100). On today's source (99 mutants): 79 caught, 13 unviable, 3 equivalent, 4 not compiled on Windows (79+13+3+4 = 99).

## Const refactor: tried, no gain, reverted
Idea (as #717 did for `tool_path_guard.rs`): move the three OR-ed masks into named consts with `const _: () = assert!(NAME == value)`. Done on a scratch copy and built clean (fmt, clippy -D warnings, 9 lib tests). `cargo mutants --list` then showed 100 mutants again, not fewer: this cargo-mutants version DOES mutate const initialisers (new entries 51:46, 55:47, 59:62, each `|`->`&` and `|`->`^`; the same holds for `tool_path_guard.rs:266:46` on master). Mutation run of the 6 new entries (`-F`, 6 mutants): the 3 `&` became unviable (the assertion fails to compile), the 3 `^` still compile because the bits are disjoint (equivalent; 2 missed, 1 shown caught only through the test-helper flake of step 2, which was not yet fixed then). So the refactor turns 3 caught into 3 unviable and removes no equivalent mutant; it was reverted. Provenance: the refactor patch is not in the branch; these refactor-run numbers come from scratch runs (TEMP `mut-comp-new`, `mut-comp-3`, second `mut-comp-0` run) and are not reproducible from the branch. Only the `cargo mutants --list` fact for `tool_path_guard.rs:266` can be checked on master. The TPG-281 sentence "the 281 `|` mutants are gone" is true only for that run's name filter; the const entry is still listed.

## Limitations
- The 53 "real" catches are classified by reading the logs (a failing test other than the flake assertion), not by a rerun.
- The whole file was not rerun end to end on the final source. Only 441:5 and 453:5 were run on it; the rest comes from runs before 55b8d6bc, which touched only `delete_file` and its caller.
- The 3 equivalents are argued from constant values (disjoint bits), with no run proof and no compile-time check; the const refactor that would have given one gave no gain and stays reverted.
- The 4 not-compiled bodies are undecided: delete them as #709 did for `tool_path_guard.rs`, or accept the label as CL-MUT-BENCH3 and CL-PHYS5 did.
- 19 of the 79 catches come only from the in-file unit tests; the integration binaries alone do not kill them.
- Rule B (P-391) is therefore not met for this file, and the five journal PARITY rows stay Partial.
- The test-helper flake is fixed (9a95e069) and is no longer an open item.
