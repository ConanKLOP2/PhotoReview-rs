# COMPACTION-FILES-MUT-2: whole-file rerun of `io/journal_compaction_files.rs` (2026-11-20)

Status: evidence report. Base: origin/master 1c5cabec (the file itself is unchanged since #722, `1213c6f3`). cargo-mutants 27.1.0. Supersedes the open items of [`COMPACTION-FILES-MUT-2026-11-11.md`](COMPACTION-FILES-MUT-2026-11-11.md).

## Commands
- `cargo mutants --list` found 99 mutants. They were split by line number into 3 shards (`-F` on `.rs:<line>:`: lines up to 289 = 31, 290 to 348 = 34, 349 and up = 34).
- Each shard: `TEMP=TMP=/tmp/mut-cmp<n> MUT_TIMEOUT=600 bash tools/dev/mutants.sh pr-core crates/pr-core/src/io/journal_compaction_files.rs --tests io_journal_compaction_files,file_actions_journal_compaction,journal_startup_recovery --native -F <range> --budget 300000 --jobs 1`. Preflight (unmutated): 75 tests executed, ok, in each shard.
- Survivor rerun against the in-file lib tests: `--cargo-test-arg --lib --cargo-test-arg journal_compaction_files`, filter on the 25 survivor positions (32 identities, siblings included), preflight 9 tests ok.

## Result of the integration shards (99 of 99, 0 NOT RUN)
| shard | caught | missed | unviable | timeout |
|---|---|---|---|---|
| lines <= 289 (31) | 17 | 4 | 10 | 0 |
| lines 290..348 (34) | 18 | 14 | 2 | 0 |
| lines >= 349 (34) | 25 | 8 | 1 | 0 |
| total | 60 | 26 | 13 | 0 |

## The 26 integration survivors
- 19 are caught by the lib tests (lib rerun: 22 caught, 1 unviable, 9 missed of 32 identities; the 9 missed contain the 7 below plus the `|`->`&` siblings of 329:37 and 360:36 that the integration shards already caught): 209:9 (2 variants), 225:9, 231:9, 290:39, 292:24, 292:37, 292:62, 292:71, 293:22, 293:31, 293:39, 293:47, 293:55, 293:63, 293:71, 339:57, 382:28, 382:41.
- 3 equivalent: 329:37 (`FILE_SHARE_READ | FILE_SHARE_DELETE`), 360:36 (`GENERIC_WRITE | DELETE`), 387:52 (`FILE_RENAME_FLAG_REPLACE_IF_EXISTS | FILE_RENAME_FLAG_POSIX_SEMANTICS`), all `|` -> `^`. The operands share no bit, so `a | b == a ^ b`. This was argued in the 2026-11-11 note; it is now checked: the new lib test `or_ed_flag_pairs_share_no_bit_so_or_and_xor_agree` holds compile-time asserts that each pair has `a & b == 0` (plus a runtime `a | b == a ^ b`). A change of a constant that broke the premise stops compiling.
- 4 not compiled on Windows: 349:5 `open_deny_writers`, 368:5 `create_new_staged`, 432:5 `rename_by_handle`, 453:5 `delete_file`, bodies of the `#[cfg(not(windows))]` twins. Decision P-412 (accept the label, same as CL-MUT-BENCH3 and CL-PHYS5; deleting the twins stays the alternative).

## Final verdict (99 identities)
79 caught (60 integration + 19 lib-only), 13 unviable, 3 equivalent (proof test), 4 not compiled on Windows, 0 missed otherwise, 0 timeout, 0 NOT RUN (79+13+3+4 = 99). This equals the 2026-11-11 verdict on today's source, now from one end-to-end run on the current code. The only code change of this PR is the one lib test (test-only).

## Limitations
- The 19 lib-only catches again need the in-file unit tests; the integration binaries alone do not kill them.
- The 4 twin bodies are accepted, not killed (P-412 is an unconfirmed proposal).
- Raw outputs stay under the user temp folder (`mut-cmp<n>`, `mut-cmpL`), not in the repository.
- Checks: `cargo fmt -p pr-core --check` ok; `cargo test -p pr-core --lib journal_compaction_files` 10 passed.
