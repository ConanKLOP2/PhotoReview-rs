Status: evidence report (2026-11-03), lane mut-journal-line-2. COMPLETES the run of `journal_line.rs` that JOURNAL-LINE-MUTATION-2026-11-03.md left at 92 of 136: the remaining 44 mutants, all tested, 0 killable survivors.

# journal_line.rs: the 44 mutants the first run did not reach

File: `crates/pr-core/src/file_actions/journal_line.rs` (657 lines). Tool: cargo-mutants 27.1.0 through `tools/dev/mutants.sh`. Proving tests (the same 7 binaries as the first run): `journal_line`, `file_actions_journal_rules`, `journal_reconcile`, `w3_jrnl_killers5`, `w3_jrnl_killers6`, `w3_jrnl_killers7`, `w3_jrnl_killers8`. The new binary `journal_line_dt_eq_killers` of the first report is not among them and was not needed here.

## Result (real numbers, from `mutants.out/outcomes.json` and `caught.txt` / `missed.txt` / `unviable.txt` / `timeout.txt`)

| Tested / selected | Caught | Missed | Unviable | Timeouts | NOT RUN |
|---|---|---|---|---|---|
| 44 / 44 | 35 | 1 | 8 | 0 | 0 |

Preflight (unmutated test command): `preflight ok: 98 tests executed`, exit 0. The 44 are exactly the mutants at source lines 427 to 655 of the 136 in the first report (`cargo mutants --list` with the same `--skip 'expecting|hash|reject'` gives 136; the line filter below selects 44, rows 93 to 136 of that list). The run took 13 minutes (-j 1, `CARGO_BUILD_JOBS=2`, about 21 s build + 3 s test per mutant); the 3600 s budget was not exhausted. Together with the first report: 136 of 136 mutants of the file (minus the 5 glue mutants skipped on purpose: `Visitor::expecting` x3, `Hash::hash`, `reject`) have now been run: 113 caught, 6 missed (5 + 1), 17 unviable, 0 timeouts.

## The 1 survivor

| Mutant | Verdict | Killer |
|---|---|---|
| `:623:9` `GroupMembersVisitor::visit_unit` -> `Ok(Default::default())` | EQUIVALENT, not killable. `Self::Value` is `Option<Vec<MemberSlot>>`, and `Default::default()` of an `Option` is `None`, which is exactly what the original body returns (`Ok(None)`). The mutated function is the same function. | none |

So no new test file was written: there is no killable survivor, and an extra test that cannot fail under any mutant would only look like evidence. `crates/pr-core/tests/journal_line_dt_eq_killers2.rs` was therefore NOT created (the task asked for it only for killable survivors).

## The 8 unviable mutants (do not compile, so cannot be tested)

`member_from_number` -> `Some(Default::default())` (:441), `EntryAcc::finish` / `MemberSlot::finish` / `MemberAcc::finish` -> `Some(Default::default())` (:489, :518, :527; the journal types have no `Default`), `GroupMembers::deserialize` and `MemberSlot::deserialize` -> `Ok(Default::default())` (:609, :637), `MemberSlotVisitor::visit_unit` / `visit_map` -> `Ok(Default::default())` (:651, :655; `MemberSlot` has no `Default`).

## The 35 caught mutants

The `==` -> `!=` in `member_from_text` (:427), the `member_from_number` `None` and `==` -> `!=` mutants (:441, :442), the five `is_usable` mutants (:447), `EntryAcc::finish` -> `None` and `delete !` (:489, :495), `MemberSlot::finish` and `MemberAcc::finish` -> `None` (:518, :527), `read_entry` -> `Ok(Default::default())` and each of its 14 deleted match arms (`Id`, `Type`, `State`, `Source`, `Destination`, `Size`, `LastWriteUtc`, `TimestampUtc`, `Error`, `ErrorCode`, `Permanent`, `Undo`, `GroupId`, `GroupMembers`), `read_member` -> `Ok(Default::default())` and its 5 deleted arms, `EntryVisitor::visit_map` -> `Ok(Default::default())` (:600), `GroupMembersVisitor::visit_seq` -> `Ok(Default::default())` (:627). All were killed by the existing 7 binaries; no mutated build passed them.

## Command

```
TEMP=<worktree>/m MUT_OUT=$TEMP/out CARGO_BUILD_JOBS=2 bash tools/dev/mutants.sh pr-core \
  crates/pr-core/src/file_actions/journal_line.rs \
  --tests journal_line,file_actions_journal_rules,journal_reconcile,w3_jrnl_killers5,w3_jrnl_killers6,w3_jrnl_killers7,w3_jrnl_killers8 \
  --budget 3600 --jobs 1 --skip 'expecting|hash|reject' \
  -F 'journal_line.rs:(427|4[4-9][0-9]|[5-9][0-9][0-9]):[0-9]+:'
```

Note: the helper's final table step needs `python` (`PY_BIN`), which is not on this machine's PATH; the counts above were taken from cargo-mutants' own summary line (`44 mutants tested in 13m: 1 missed, 35 caught, 8 unviable`) and from the line counts of its `*.txt` outcome files (35 / 1 / 8 / 0), which agree.

## Verdict

`journal_line.rs` now has a complete mutation result: every non-glue mutant was run; 113 caught, 4 of the 6 survivors were killable and got tests in the first report (manually RED-checked, not re-run through cargo-mutants), 2 are equivalent (`integer_token` guard at :380, `visit_unit` at :623), 17 unviable. Scope limits unchanged: this file only; `journal_rules`, `recovery_retry`, `transaction` and `compaction` are covered by other reports or not run, and the 5 skipped glue mutants are not claimed. The D-03 / F-11 / F-13 / F-16 / INV-6 rows are for the lead to rate; this report alone does not change any C# behaviour comparison.
