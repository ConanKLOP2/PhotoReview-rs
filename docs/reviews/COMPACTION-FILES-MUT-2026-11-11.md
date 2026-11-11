# COMPACTION-FILES-MUT: mutation verdict for `io/journal_compaction_files.rs` (2026-11-11)

Status: evidence report (2026-11-11). Production code unchanged (a const refactor was tried and reverted, see below). PARITY.md, PROPOSED-DECISIONS.md and task_on_progress.md are synced by the follow-up commit on this branch, not by this note. Base: origin/master 844b6fbc; the file is unchanged since #17. cargo-mutants 27.1.0. This closes the open item of [`TRANSACTION-RERUN-2026-11-10.md`](TRANSACTION-RERUN-2026-11-10.md) (no valid verdict for this file).

## Why the 2026-11-10 attempt gave no verdict
It ran with `--timeout 90`. The unmutated compaction binaries need about 150 to 190 s on this machine (`file_actions_journal_compaction` 161 s, `journal_startup_recovery` 107 s in today's run), so every mutant that reached them timed out (15 timeouts, 9 unviable, nothing caught). A timeout is not a catch. This run used `MUT_TIMEOUT=600`.

## Commands and baseline
`TEMP=...\mut-comp-<n> TMP=...\mut-comp-<n> MUT_TIMEOUT=600 bash tools/dev/mutants.sh pr-core crates/pr-core/src/io/journal_compaction_files.rs --tests io_journal_compaction_files,file_actions_journal_compaction,journal_startup_recovery --native --budget 7200 --jobs 1` in 3 shards (own TEMP each: `mut-comp-0`, `-1`, `-2`). `--native` gives `--include-ignored --test-threads=1`. Preflight (unmutated): 75 tests executed, ok.
Today, on master content: `file_actions_journal_compaction` 30 passed, `io_journal_compaction_files` 15 passed, `journal_startup_recovery` 30 passed (75 total, 0 failed, 0 ignored); `cargo test -p pr-core --lib journal_compaction_files` 9 passed.

## Result (100 mutants, 0 NOT RUN)
| shard | caught | missed | unviable | timeout |
|---|---|---|---|---|
| 0 (34) | 22 | 2 | 10 | 0 |
| 1 (33) | 23 | 7 | 3 | 0 |
| 2 (33) | 27 | 6 | 0 | 0 |
| integration total | 72 | 15 | 13 | 0 |

The 15 missed were rerun against the in-file unit tests (`cargo test -p pr-core --lib journal_compaction_files`, 9 tests; the `-F` filter also matched sibling variants, so that rerun held 21 identities: 12 caught, 8 missed, 1 unviable).

| mutant | integration | final | reason |
|---|---|---|---|
| 231:9 `source` -> None | missed | caught (unit) | lib test checks `Error::source` |
| 290:39 `==`->`!=` is_device | missed | caught (unit) | lib test |
| 292:71 `&&`->`||` is_device | missed | caught (unit) | lib test |
| 293:22, 293:55, 293:71 `==`->`!=` is_device | missed | caught (unit) x3 | lib tests |
| 339:57 `&&`->`||` map_root_path_not_found | missed | caught (unit) | lib test |
| 382:28, 382:41 `+`->`*` rename_info | missed | caught (unit) x2 | lib tests |
| 329:37 `|`->`^` (FILE_SHARE_READ 0x1 `|` FILE_SHARE_DELETE 0x4) | missed | equivalent | disjoint bits, `|` and `^` give 0x5 |
| 360:36 `|`->`^` (GENERIC_WRITE `|` DELETE) | missed | equivalent | disjoint bits |
| 387:52 `|`->`^` (REPLACE_IF_EXISTS 0x1 `|` POSIX_SEMANTICS 0x2) | missed | equivalent | disjoint bits |
| 349:5 open_deny_writers | missed | not compiled on Windows | body of the `#[cfg(not(windows))]` twin, dead code here (same label as CL-MUT-BENCH3, CL-PHYS5) |
| 368:5 create_new_staged | missed | not compiled on Windows | same |
| 457:5 delete_file | missed | not compiled on Windows | same |

The `|`->`&` variants of 329:37 and 360:36 show as missed in the lib rerun only because lib tests never open files; the integration shards CAUGHT both.

Combined verdict: 81 caught, 13 unviable, 3 equivalent, 3 not compiled on Windows, 0 timeout, 0 NOT RUN (81+13+3+3 = 100).

## Const refactor: tried, no gain, reverted
Idea (as #717 did for `tool_path_guard.rs`): move the three OR-ed masks into named consts with `const _: () = assert!(NAME == value)`. Done on a scratch copy and built clean (fmt, clippy -D warnings, 9 lib tests). `cargo mutants --list` then showed 100 mutants again, not fewer: this cargo-mutants version DOES mutate const initialisers (new entries 51:46, 55:47, 59:62, each `|`->`&` and `|`->`^`; the same holds for `tool_path_guard.rs:266:46` on master). Mutation run of the 6 new entries (`-F`, 6 mutants): the 3 `&` became unviable (the assertion fails to compile), the 3 `^` still compile because the bits are disjoint (equivalent; 2 missed, 1 shown caught only through the test-helper flake below). So the refactor turns 3 caught into 3 unviable and removes no equivalent mutant; it was reverted. Provenance: the refactor patch is not in the branch (production code unchanged); these refactor-run numbers come from scratch runs (TEMP `mut-comp-new`, `mut-comp-3`, second `mut-comp-0` run) and are not reproducible from the branch. Only the `cargo mutants --list` fact for `tool_path_guard.rs:266` can be checked on master. The TPG-281 sentence "the 281 `|` mutants are gone" is true only for that run's name filter; the const entry is still listed.

## Limitations and an open defect
- The integration binaries alone do not kill 9 of the 15; they need the in-file unit tests.
- Test-helper flake: `path_of_length` in `crates/pr-core/tests/io_journal_compaction_files.rs` (around lines 288 to 308) fails `assert_eq!(count, units)` with 299 vs 300 for some temp-root lengths (when the remaining length hits exactly 202 the next folder leaves a remainder of 1, and the "never leave 1" rule only covers `left <= 201`). It made a preflight fail (TEMP `mut-comp-3`) and gave false "caught" results in two extra runs (TEMP `mut-comp-new`, `mut-comp-0` second run), including for an equivalent mutant. The three original shards passed preflight, and none of the 72 original catches was checked for this flake. Not fixed here (test change outside this task); it should be fixed before the 72 catches are treated as final.
