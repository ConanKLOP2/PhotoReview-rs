# CL-JOURNAL 2026-10-30: journal.rs mutation re-run (INV-6, F-16 / D-14)

Status: report. Result: the re-run could NOT be completed, the "179 of 179 covered and clean" claim is NOT reproduced, and
the PARITY rows INV-6 (journal part) and F-16 / D-14 are demoted from Done to Partial. No production code changed.

## Why

The independent audit (AUDIT-DONE-2026-10-30.md, PR 394, not yet in this branch) found that `PARITY.md` said `journal.rs` "179 of 179 covered
and clean" (commit 5946f0f6, PR #331, pass `journal_a`), while `W3-01-JOURNAL-MUTATION.md` documents only 94 of 179 run
(73 caught, 5 missed, 6 timeouts, 10 unviable, 85 not run) and concludes "the PARITY rows stay Partial". The raw
`mutants.out` of the second pass is lost and the evidence file was never updated.

## What was run (machine A, claim cl-journal)

* Mutants: `crates/pr-core/src/file_actions/journal.rs`, 179 found (`--list`), split by line range into three `-F` groups
  (44, 82 and 61 by `--list`; the regexes overlap slightly).
* Command: `cargo mutants -p pr-core --baseline skip --timeout 90 -j 1 --file "**/file_actions/journal.rs" -F <range>`
  with `--cargo-arg=--test=` for `file_actions_journal`, `file_actions_journal_compaction`, `journal_mutation_killers`,
  `_2`, `_3`, `_4`.
* The machine was at 100 % CPU from other lanes. Group 1 got through 11 mutants in about 20 minutes (about 2 minutes each)
  and was then stopped by me (own PIDs only); groups 2 and 3 were started and stopped immediately with 0 mutants tested.

| Group | Tested | Caught | Missed | Timeout | Unviable | Not run |
|---|---|---|---|---|---|---|
| 1 (lines < 500) | 11 | 0 | 0 | 10 | 1 | 33 |
| 2 (500..799) | 0 | 0 | 0 | 0 | 0 | all |
| 3 (800..) | 0 | 0 | 0 | 0 | 0 | all |

The 10 timeouts (`:56:49` x2, `:58:50` x2, `:77:55` x2, `:117:9` x2, `:215:9` and one more) and the 1 unviable (`:152:9`)
are NOT usable as a verdict: a baseline timing taken afterwards shows the `file_actions_journal` binary alone needs 126 s
and `file_actions_journal_compaction` 49 s on the loaded machine (the `random_history_*` tests run over 60 s each), so with
`--timeout 90` every mutant that survives the first tests times out regardless of what it changed. `:117:9` (`as_str` to
`""`) and `:215:9` (`english` to `String::new()`) are plain logic mutants that must be caught, not hangs, which confirms
the timeouts are load artifacts. The same baseline run also failed 2 tests
(`english_journal_texts_localized_is_the_english_text`, `reconcile_journal_impl_delegates_to_the_journal`) because the
shared `target-mut` held a stale build of a mutated `journal.rs`; after touching the source both pass (2 passed). Lesson:
do not point `CARGO_TARGET_DIR` of a normal `cargo test` at the `target-mut` directory used by cargo-mutants.

So the honest numbers for `journal.rs` are: 179 found, 94 documented (73 caught, 5 missed all killed or explained, 6 hangs,
10 unviable; from W3-01), 85 NOT proven. The claim "144 unique, 102 caught, 3 missed, 15 hangs, 24 unviable" of pass
`journal_a` cannot be checked (no raw output).

## Test counts today

`file_actions_journal`: 83 tests, 19 `#[ignore]`d native/slow, 64 run by default (62 passed + the 2 stale-build failures
above, which pass on a clean build). `file_actions_journal_compaction`: 15 run + 15 ignored. Killers: `journal_mutation_killers`
11, `_2` 4, `_3` 3, `_4` 1, all green. The PARITY text "all 64 green" meant the default run; the whole binary is 83.

## Killers

No `w3_journal_killers2.rs` was written: no mutant was reliably MISSED in this run, and the 85 unrun mutants (append,
torn-tail, sharing retry, reader, reverse tail reader, `read_latest_entry`, `append_if*`) were not reached.

## What is needed to close this

1. Run the three groups on a quiet machine (or with `--timeout 300`), about 3 hours at the observed pace with 6 binaries;
   consider excluding `random_history_*` for the survey pass and running them only on survivors.
2. Use a dedicated `CARGO_TARGET_DIR` for cargo-mutants and never reuse it for plain `cargo test`.
3. Keep the raw `mutants.out` summaries in the evidence this time.
4. The `#[ignore]`d native twins were not run under mutation.

Rows changed in `docs/PARITY.md`: INV-6 (journal part) and F-16 / D-14 Done to Partial; D-03 / F-11 / F-19 / F-20 / S-21,
F-12 / F-15 and F-13 keep their status but their "179 of 179" wording now reads "94 of 179 documented" (F-13 is the append
path whose 85 unrun mutants are exactly the unproven part; the lead should decide whether to demote it too).
