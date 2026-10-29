# W3-01 operation journal: mutation evidence (partial run)

Machine A, claim `PARTIAL-mut-journal`, 2026-10-29. This is a PARTIAL run, not a full mutation verdict: it is time-boxed
(about 25 minutes per file, about 100 minutes of wall time on 2 parallel processes) and 132 of 683 mutants were NOT run.

Command shape (one own `CARGO_TARGET_DIR` per process outside the worktree, `-j 1`, `--baseline skip`, `--timeout 90`,
`--shuffle`, test binaries restricted with `--cargo-arg=--test=<binary>`):

```
cargo mutants -p pr-core --baseline skip --timeout 90 -j 1 --shuffle --file '**/file_actions/<file>.rs' \
  [-F|-E '<regex>'] --cargo-arg=--test=<binary> [--cargo-arg=--test=<binary2>]
```

The files have no `#[cfg(test)]` unit tests, so no `--lib` pass was needed. Recycle Bin check: none of the binaries
below touches the real bin (`recovery_retry` uses the scripted `ScriptedBin`; `pr-core` has no `SHFileOperation` or
`SHEmptyRecycleBin` call). Test binaries: `file_actions_journal`, `file_actions_journal_compaction`,
`file_actions_compaction`, `file_actions_journal_rules`, `file_actions_transaction`, `journal_line`, `journal_reconcile`,
`journal_startup_recovery`, `recovery_retry`.

## Result per file (before the new tests)

| File | Found | Tested | Caught | Missed | Timeout | Unviable | Not run |
|---|---|---|---|---|---|---|---|
| `transaction.rs` | 52 | 52 | 36 | 2 | 0 | 14 | 0 |
| `compaction.rs` | 31 | 31 | 31 | 0 | 0 | 0 | 0 |
| `reconcile.rs` | 80 | 80 | 57 | 0 | 0 | 23 | 0 |
| `startup_recovery.rs` | 18 | 18 | 13 | 0 | 0 | 5 | 0 |
| `journal.rs` | 179 | 94 | 73 | 5 | 6 | 10 | 85 |
| `journal_line.rs` | 141 | 126 | 100 | 10 | 0 | 16 | 15 |
| `journal_rules.rs` | 22 | 20 | 17 | 2 | 0 | 1 | 2 |
| `recovery_retry.rs` | 160 | 130 | 110 | 7 | 0 | 13 | 30 |
| Total | 683 | 551 | 437 | 26 | 6 | 82 | 132 |

Notes on `journal.rs`: four passes were merged, best outcome per mutant. Pass 1 `file_actions_journal` (30 mutants, shuffled
sample), pass 2 `file_actions_journal_compaction` limited to the compaction functions (37), pass 3 `file_actions_journal`
with the compaction functions excluded (38), pass 4 `file_actions_journal_compaction` on the `STALE_COMPACTION_FILE_AGE_TICKS`
constant and `len_i64` (9). The first pass overlapped in time with the second and shared its output directory, so its late
results were not used; the second to fourth passes were rerun cleanly in their own directories. The six timeouts are
hangs (an endless retry or read loop), counted as detected: `journal.rs:329:24` (gate drop `==` to `!=`), `:58:50`,
`:693:29`, `:695:29`, `:938:11`, `:999:33`. 85 `journal.rs` mutants were never run (mostly the append, reader and
reverse-tail paths), so the append/torn-tail row F-13 has NOT received a verdict.

## The 26 missed mutants

Killed by the new tests (`crates/pr-core/tests/journal_mutation_killers.rs`, `journal_mutation_killers_2.rs`; each test
names its mutant in a comment). Proof: cargo-mutants run against the two new binaries ALONE
(`--cargo-arg=--test=journal_mutation_killers --cargo-arg=--test=journal_mutation_killers_2`, -F on the mutant lines):
all 28 targeted mutants caught, 0 missed (the run also swept 77 unrelated field-deletion mutants of other files that the
`-F` regex over-matched; those are irrelevant to this claim).

| Mutant | Test |
|---|---|
| `transaction.rs:62` Debug `fmt` -> `Ok(Default)` | `transaction_debug_names_the_type_and_every_state_flag` |
| `journal.rs:263` `OperationJournalOptions` Debug `fmt` | `journal_options_debug_names_the_type_and_reports_which_options_are_set` |
| `journal.rs:470..473` `dismiss`: delete `state`, `timestamp_utc`, `error`, `error_code` (ran only against the compaction binary, which does not cover `dismiss`) | `dismiss_sets_state_stamp_and_clears_error_in_the_outcome_and_in_the_written_line` |
| `journal.rs:77:55` `STALE_COMPACTION_FILE_AGE_TICKS` `*` to `/` (the compaction tests derive their ages from the constant, so it must be pinned) | `stale_compaction_file_age_is_one_hour_in_ticks` |
| `journal_rules.rs:42` `is_reconcile_code` -> `true` | `is_reconcile_code_accepts_only_the_two_reconcile_codes` |
| `journal_rules.rs:54` `&&` to `||` | `stale_reconcile_needs_a_reconcile_code_and_a_committed_previous_line` |
| `journal_line.rs:116` `eq` -> `true`, `:124` `hash` -> `()` | `journal_date_time_equality_and_hash_follow_the_instant` |
| `journal_line.rs:315`, `:320`, `:327` `nests_too_deep` | `line_with_an_escaped_quote_before_many_brackets_stays_inside_the_string`, `line_with_many_open_brackets_inside_a_string_is_not_too_deep`, `many_sequential_closed_containers_are_not_too_deep` (+ `line_nested_deeper_than_the_limit_is_refused`) |
| `recovery_retry.rs:364`, `:879` blank destination guard | `retry_with_a_blank_destination_is_refused_as_no_destination`, `is_null_or_white_space_follows_dotnet` |
| `recovery_retry.rs:383`, `:384`, `:387` (single retry Prepared line) | `single_retry_prepared_line_clears_the_old_error_and_code_and_takes_the_service_time` |
| `recovery_retry.rs:494`, `:495` (group retry Prepared line) | `group_retry_prepared_line_clears_the_old_code_and_takes_the_service_time` |

Equivalent or unobservable (no test can tell them apart; production code unchanged):

* `transaction.rs:315` `Drop::drop` -> `()`: the only work in `drop` is `live_marker = None`; the field is dropped right
  after anyway, so the marker is released either way. The existing `*_releases_the_live_marker_before_dispose` tests
  cover the explicit release paths.
* `journal_line.rs:596`, `:619`, `:647` `Visitor::expecting`: the text only appears in a serde error, and
  `try_parse_utf8` discards the error (`.ok()?`), so no caller can read it.
* `journal_line.rs:623` `GroupMembersVisitor::visit_unit` -> `Ok(Default::default())`: the default of
  `Option<Vec<_>>` is `None`, which is exactly what the original returns.
* `journal_line.rs:380:31` `integer_token` guard -> `true`: `str::parse::<i64>` already rejects `.`, `e` and `E`, so the
  guard is redundant.

No mutant exposed a production bug; no production code was changed.

## What was NOT run

* `journal.rs`: 85 of 179 mutants (the append, torn-tail, sharing retry, reader, reverse tail reader, `read_latest_entry`
  and `append_if*` paths were only sampled).
* `journal_line.rs`: 15 of 141. `journal_rules.rs`: 2 of 22. `recovery_retry.rs`: 30 of 160 (only the `recovery_retry`
  binary was used, not `recovery_retry_golden`).
* The native twins (`#[ignore]`d real-disk tests) were not run under mutation.

The PARITY rows stay `Partial`: the verdict is clean only for `compaction.rs`, `reconcile.rs`, `startup_recovery.rs` and
`transaction.rs`; the rest is a sample.
