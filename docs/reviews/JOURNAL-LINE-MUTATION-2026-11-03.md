Status: evidence report (2026-11-03), lane mut-journal-line. PARTIAL run of `journal_line.rs` (92 of 136 mutants); PARITY status NOT raised.

# journal_line.rs: partial mutation run with the correct flags

File: `crates/pr-core/src/file_actions/journal_line.rs` (657 lines). Tool: cargo-mutants 27.1.0 through `tools/dev/mutants.sh`. Proving tests (7 binaries): `journal_line`, `file_actions_journal_rules`, `journal_reconcile`, `w3_jrnl_killers5`, `w3_jrnl_killers6`, `w3_jrnl_killers7`, `w3_jrnl_killers8`.

## Result (real numbers, from `mutants.out/outcomes.json`)

| Tested / found | Caught | Missed | Unviable | Timeouts | NOT RUN |
|---|---|---|---|---|---|
| 92 / 136 | 78 | 5 | 9 | 0 | 44 |

Preflight (unmutated test command): `preflight ok: 98 tests executed`, exit 0. cargo-mutants found 141 mutants in the file; 5 were excluded on purpose with `--skip 'expecting|hash|reject'` (the three `Visitor::expecting` messages, `Hash::hash`, `reject`: glue without branching), leaving 136. The 1800 s budget ran out ("BUDGET EXHAUSTED", exit 124) at about 20 s per mutant (-j 1, one idle build each). Mutants run, in file order, reach line 424. **NOT RUN (44)**: everything after `member_from_text` line 424, i.e. `member_from_text` 427, `member_from_number`, `is_usable`, the three `finish` fns, `read_entry` (14 match arms), `read_member` (5 arms), and the `GroupMembers` / `MemberSlot` visitor and deserialize functions. This report says nothing about them. (The helper's own summary prints "tested 92 / total 92" because cargo-mutants wrote `total_mutants` for the interrupted run; the 136 above comes from its "Found 136 mutants" line.) The other files named in the PARITY notes (journal_rules, recovery_retry, transaction, compaction) were NOT run.

Skipped as glue (not mutated): `Visitor::expecting` x3, `Hash::hash`, `reject`. Serde `serialize`/`deserialize` shells were included but are not branching.

## The 5 survivors

| Mutant | Verdict | Killer |
|---|---|---|
| `:116:9` `JournalDateTime::eq` -> `true` | killable, no test compared two different timestamps | `different_ticks_are_not_equal` |
| `:315:25` `==` -> `!=` in `nests_too_deep` (closing-quote test) | killable: brackets inside a string value counted as nesting | `brackets_inside_a_string_value_do_not_count_as_nesting` |
| `:320:17` delete arm `b'"'` in `nests_too_deep` | killable, same test (strings never entered) | same |
| `:327:17` delete arm `b'}' \| b']'` in `nests_too_deep` | killable: depth never decreased, so 100 sibling arrays would be rejected | `closed_containers_lower_the_depth_again` |
| `:380:31` match guard `!raw.bytes().any(..)` -> `true` in `integer_token` | equivalent, not killable: without the guard `raw.parse::<i64>()` still returns `Err` for any token containing `.`, `e` or `E`, so the result is the same `None` (the guard is a redundant shortcut) | none |

New tests (test-only diff, `crates/pr-core/tests/journal_line_dt_eq_killers.rs`, 4 tests). RED FIRST, checked by hand-applying each mutant to the source (then restored, `git diff` of the source is empty): with `eq` -> `true`, `different_ticks_are_not_equal` FAILED; with 315 `!=`, `brackets_inside_a_string_value...` FAILED; with the 320 arm removed, `brackets_inside_a_string_value...` FAILED; with the 327 arm replaced by `{}`, `closed_containers_lower_the_depth_again` FAILED. On the unmodified source all 4 pass (`4 passed`). The run itself was NOT repeated with the new test file (it is not in the 7 binaries above), so "killed" here means the manual RED check, not a second cargo-mutants pass.

## Command

```
TEMP=<repo>/.claude/tmp-mut-journal-line MUT_OUT=$TEMP/out bash tools/dev/mutants.sh pr-core \
  crates/pr-core/src/file_actions/journal_line.rs \
  --tests journal_line,file_actions_journal_rules,journal_reconcile,w3_jrnl_killers5,w3_jrnl_killers6,w3_jrnl_killers7,w3_jrnl_killers8 \
  --budget 1800 --jobs 1 --skip 'expecting|hash|reject'
```

## Verdict

For the 92 mutants run (the writer, `parse_json`, `stj_shape_ok`, `to_line`, `push_unit`, `try_parse*`, `nests_too_deep`, the `decode_*` helpers, `integer_token`, `recognize`, most of `member_from_text`): 78 caught, 4 killable survivors now covered by new tests, 1 equivalent, 9 unviable. The 44 NOT RUN mutants (the enum/text recognition tail, the field accumulators and the member readers) are still open. The D-03 / F-11 / F-13 / F-16 / INV-6 rows stay Partial; no P-number was needed.

## Update 2026-11-10 (MUT-2): the `integer_token` equivalent survivor is removed

The `:380:31` guard (`!raw.bytes().any(|b| matches!(b, b'.' | b'e' | b'E'))`) was removed from `integer_token`: `str::parse::<i64>` already rejects `.`, `e` and `E`, so the guard never changed a result (the equivalence argued in the table above is now closed by removal, not by argument). Test `size_integer_token_accepts_only_plain_i64_integers` (`journal_line` binary) pins `1.5`, `1e5`, `1E5`, `-0.0`, `-`, `+1`, `007` and 2^63 to rejected, `-0` to 0, i64::MIN/MAX accepted. Re-run of `integer_token` mutants (`tools/dev/mutants.sh pr-core ... -F integer_token`, 8 test binaries, `-j 1`, preflight 103 tests executed): 5 tested / 5, 5 caught, 0 missed, 0 unviable, 0 timeouts. Only the `integer_token` mutants were run; the rest of the file is unchanged by this note.
