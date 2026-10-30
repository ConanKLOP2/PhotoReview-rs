# CL-FSRERUN: real rerun of the CL-FSFLUSH mutation (D-05 durable flush, INV-8 SequentialScan)

Lane `cl-fsrerun`, run 2026-10-30 to 31, branch `feat/cl-fsrerun`. Context: [AUDIT-MUTATION-FLAG-2026-10-30](AUDIT-MUTATION-FLAG-2026-10-30.md) (earlier native mutation passes could run no test and still count mutants as caught). Source of the claim: [CL-FSFLUSH](CL-FSFLUSH-2026-10-30.md).

## Command and preflight

The mutated crate is `pr-core`, the tests live in `pr-platform-win`, so cargo-mutants needs `--test-package`:

```
cargo mutants -p pr-core --test-package pr-platform-win --baseline skip --timeout 300 -j 1 \
  --file "**/io/physical.rs" -F "write_all_text_atomic|open_read_shared|flush_temp_file|temp_file_flushes" \
  --cargo-test-arg=--test --cargo-test-arg=w2_fs_flush_killers \
  --cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1
```

Preflight (`stage-native.ps1`, then the same test without mutation): `running 9 tests ... 9 passed`. The mutant logs show `running 9 tests` and e.g. `8 passed; 1 failed` for a killed mutant, so tests really ran. Builds took 5 to 10 minutes per mutant (pr-core change rebuilds pr-app, pr-bench, pr-platform-win tests), so the run took about 2 hours.

## Result: 17 found, 17 tested, 8 caught, 3 unviable, 6 missed, 0 timeouts

Caught (8): `temp_file_flushes_on_this_thread -> 1`, `flush_temp_file -> Ok(())`, `+` -> `-` and `+` -> `*` in `flush_temp_file`, share-mode `|` -> `&` at col 29, `write_all_text_atomic_observed -> Ok(())`, `delete !` in it, `write_all_text_atomic -> Ok(())`.

Unviable (3): `temp_file_flushes_on_this_thread -> 0` (cargo-mutants reports it unviable, the build of that mutant failed), `open_read_shared_file -> Ok(Default::default())` (File) and the same for `open_read_shared` (Box<dyn ReadSeek>).

Missed (6), classified:

| Mutant | Verdict |
|---|---|
| `open_read_shared_file` 1143:48 `\|` -> `&` (`FILE_SHARE_WRITE \| FILE_SHARE_DELETE` becomes `&` = 0) | REAL gap: the flush killers never write to or rename the file while the stream is open. Killed by the new `w2_fsrerun_killers.rs` (a second reader, a writer, rename and delete while the stream is open); verified by hand-applying the mutant: 0 passed, 1 failed |
| 1143:48 `\|` -> `^` and 1143:29 `\|` -> `^` | Equivalent: the flag bits are disjoint (1, 2, 4), so `^` equals `\|` (hand-applied, new killer still passes) |
| `open_read_shared` 1326:24 `buffer_size > 1` -> `==`, `<`, `>=` | Equivalent for behaviour: the choice between `BufReader` and a plain `File` changes buffering only, never content or sharing; the C# `bufferSize <= 1` rule is a performance detail with no public observable |

The earlier CL-FSFLUSH note "share-mode `|` mutants ... caught" was therefore wrong for the `|` -> `&` mutant at 1143:48 when only the flush killers run (the old pass used an invalid flag form per the audit).

## Hand mutants (7), each applied, `w2_fs_flush_killers` run, reverted

| Mutant | Result | Killing test(s) |
|---|---|---|
| temp-file flags `if durable {WT} else {0}` -> `0` | caught, 8 passed 1 failed | `durable_atomic_write_opens_the_temp_file_write_through` |
| ... -> always `WT` | caught | `non_durable_atomic_write_does_not_open_write_through` |
| `if durable { observe(Flush) }` -> `if false` | caught, 6 passed 3 failed | flush-once, failed-flush, trait-count |
| ... -> `if true` | caught, 6 passed 3 failed | flush-once, non-durable, trait-count |
| `FILE_FLAG_SEQUENTIAL_SCAN` -> `0` in `open_read_shared_file` | caught | `open_read_shared_sets_the_sequential_scan_mode` |
| match arm `Flush => flush_temp_file(file)` -> `Ok(())` | caught | `trait_path_flushes_the_temp_file_once_...` |
| `open_append` flags -> `0` | caught | `open_append_durable_is_write_through_and_fast_is_not` |

## Verdicts

- `D-05` stays Done: every flush, counter, WRITE_THROUGH and guard mutant is caught on a real run. Row note: `rerun OK 2026-10-31`.
- `INV-8` (file-system part) stays Done: the SequentialScan hint is caught on a real run; the one real miss (share modes) is now killed by `w2_fsrerun_killers`. The source-reader open site remains unobserved (known limit, unchanged).
