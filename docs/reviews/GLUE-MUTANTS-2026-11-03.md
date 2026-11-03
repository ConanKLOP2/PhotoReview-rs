Status: evidence report (2026-11-03), task T10. Runs the glue mutants that RECOVERY-RETRY-MUTATION-2-2026-11-03.md and JOURNAL-LINE-MUTATION-2-2026-11-03.md left NOT RUN. Result: 0 NOT RUN left in both sets, 1 killable survivor (`Hash for JournalDateTime`) killed by a new test file, no production change.

# Glue mutants: recovery_retry.rs (a) and journal_line.rs (b)

Tool: cargo-mutants 27.1.0 through `tools/dev/mutants.sh`, `-j 1`, `CARGO_BUILD_JOBS=2`, `TEMP=<worktree>/m`, master `4c3e41b`. Counts below come from the cargo-mutants summary lines and the `caught.txt` / `missed.txt` / `unviable.txt` files of each run.

## (a) recovery_retry.rs

`cargo mutants --list` of the file gives 159 lines on this master (the report said 160). The helper's `--skip` (exclude-re) of the group-path function names selected 77 mutants (the "32 by arithmetic" in the old report was a guess; the real remainder is larger because `-E` still lets `retry_core` through, see below). Test binaries (preflight: 106 tests, exit 0): `recovery_retry`, `recovery_retry_golden`, `journal_mutation_killers_4`.

| Tested / selected | Caught | Missed | Unviable | Timeouts | NOT RUN |
|---|---|---|---|---|---|
| 77 / 77 (16 min) | 68 | 1 | 8 | 0 | 0 |

Selected set: the `EnglishRecoveryRetryTexts` string methods (11 pairs `String::new()` / `"xyzzy".into()`), `Debug::fmt`, `with_recycle_bin`, `with_allow_permanent_delete`, `with_file_action_gate`, `retry_move_or_copy`, `precheck_single` (5), `coded_error`, `failure_message` (2), `plain_error`, `select_pending` (8), `same_fingerprint` (5), `is_null_or_white_space` (2), `is_move_or_copy` (2), `length_as_i64` (4), plus 1 `retry_core` mutant. Some of these (select_pending, same_fingerprint, length_as_i64, precheck_single) may have been measured by the first report too; no attempt was made to de-duplicate against it, they are simply re-confirmed.

| Mutant | Status | Evidence |
|---|---|---|
| 1 `retry_core` `delete field size` (:375:13) | MISSED, known EQUIVALENT (old report: `prepared.size == failed.size` on every path reaching the struct). Not a new finding. | overlap with RECOVERY-RETRY-MUTATION-2 |
| `with_recycle_bin`, `with_allow_permanent_delete`, `with_file_action_gate`, `retry_move_or_copy`, `precheck_single`, `coded_error`, `plain_error` -> `Default::default()` (:279, :287, :294, :315, :390, :820, :840) and `select_pending -> vec![Default::default()]` (:853) | UNVIABLE (8), the types have no `Default` | compile error |
| all other 68 (both `String` replacements of every text method, `Debug::fmt`, `failure_message`, `select_pending`, `same_fingerprint`, `is_null_or_white_space`, `is_move_or_copy`, `length_as_i64`, ...) | CAUGHT | existing `recovery_retry`, `recovery_retry_golden`, `journal_mutation_killers_4` |

Note on `Debug::fmt`: the `Ok(Default::default())` mutant is among the 68 caught (a test formats the service with `{:?}`); the `with_*` builders had only the `Default::default()` mutant, which does not compile, so they have no viable mutant at all.

Verdict (a): nothing killable survives; no new test for this file. Together with the old reports every viable mutant of `recovery_retry.rs` that cargo-mutants lists is now measured: only the one known equivalent survives.

## (b) journal_line.rs, the 5 skipped glue mutants

`-F 'expecting|hash|reject'` selects exactly these 5 (checked with `--list` first). Test binaries (the same 7 as JOURNAL-LINE-MUTATION-2): `journal_line`, `file_actions_journal_rules`, `journal_reconcile`, `w3_jrnl_killers5..8`.

| Tested / selected | Caught | Missed | Unviable | Timeouts | NOT RUN |
|---|---|---|---|---|---|
| 5 / 5 | 0 | 4 | 1 | 0 | 0 |

| Mutant | Status | Evidence |
|---|---|---|
| `:124:9` `Hash for JournalDateTime::hash -> ()` | MISSED, KILLABLE. Nothing hashed a `JournalDateTime`. | New `crates/pr-core/tests/journal_line_hash_killers.rs` (3 tests, pass on current code). RED check: replacing the body with a no-op makes `different_ticks_hash_differently` fail; rerun of the mutant with only the new binary: `1 mutant tested in 16s: 1 caught`. |
| `:596:9` `EntryVisitor::expecting -> Ok(Default::default())` | MISSED, NOT KILLABLE without production change | `expecting` only shapes a serde error message. The only public entry points (`try_parse`, `try_parse_utf8`) do `.ok()?` and drop the error, `GroupMembers` / `MemberSlot` are private. The text is unobservable. |
| `:619:9` `GroupMembersVisitor::expecting` | same | same |
| `:647:9` `MemberSlotVisitor::expecting` | same | same |
| `:337:5` `reject -> E` with `Default::default()` | UNVIABLE, `E: de::Error` has no `Default` | compile error |

Verdict (b): 1 killer added, 3 `expecting` mutants are observably equivalent for the public API. Proposal P-390 below.

## Totals of this task

| Set | Tested | Caught | Missed | Unviable | Timeout | NOT RUN |
|---|---|---|---|---|---|---|
| (a) recovery_retry.rs | 77 | 68 | 1 | 8 | 0 | 0 |
| (b) journal_line.rs | 5 | 0 (+1 after the new test) | 4 (3 after) | 1 | 0 | 0 |
| Sum | 82 | 68 (69 after) | 5 (4 after) | 9 | 0 | 0 |

Batch caveat: `--skip`/`-F` did not narrow (a) exactly (P-111): the `retry_core` mutant ran although the function name was in the skip list. The sets above are the real `Found 77 mutants` / `Found 5 mutants` of the logs.

## Commands

```
export TEMP=<worktree>/m PY_BIN=$LOCALAPPDATA/Programs/Python/Python312/python.exe CARGO_BUILD_JOBS=2
SK='(retry_group|precheck_group_members|run_group_members|run_single|execute_group_retry|execute_retry|copy_new_or_keep_leftover|is_restored_file|::stat( |$)|committed_result|retry_core|already_handled|not_journaled)'
bash tools/dev/mutants.sh pr-core crates/pr-core/src/file_actions/recovery_retry.rs --tests recovery_retry,recovery_retry_golden,journal_mutation_killers_4 --budget 3300 --jobs 1 --skip "$SK"
bash tools/dev/mutants.sh pr-core crates/pr-core/src/file_actions/journal_line.rs --tests journal_line,file_actions_journal_rules,journal_reconcile,w3_jrnl_killers5,w3_jrnl_killers6,w3_jrnl_killers7,w3_jrnl_killers8 --budget 900 --jobs 1 -F 'expecting|hash|reject'
bash tools/dev/mutants.sh pr-core crates/pr-core/src/file_actions/journal_line.rs --tests journal_line_hash_killers --budget 600 --jobs 1 -F 'journal_line.rs:124:'
```
