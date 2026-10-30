# CL-FSFLUSH: durable flush (D-05) and SequentialScan (INV-8) made observable

Lane `cl-fsflush`, 2026-10-30, branch `feat/cl-fsflush`. C# baseline `7eaee2a2` (read only). Rows: `D-05` (physical part), `INV-8` (file-system part).

## What was unobservable and how it is observed now

Both rows were Partial for one reason: `FILE_FLAG_WRITE_THROUGH` + `sync_all` (D-05) and `FILE_FLAG_SEQUENTIAL_SCAN` (INV-8) left no trace through `std`. The kernel does keep them: `NtQueryInformationFile(FileModeInformation)` reports `FILE_WRITE_THROUGH` (0x2) and `FILE_SEQUENTIAL_ONLY` (0x4) for a handle (a plain `std::fs::File::open` reports 0 for both, so the query tells the cases apart). The problem was reaching the handle: `write_all_text_atomic` closes its temp file before it returns, and `open_read_shared` returns a `Box<dyn ReadSeek>`.

Production change (behaviour preserving, same pattern as the P-11 `FlushToDisk` seam), all in `crates/pr-core/src/io/physical.rs`:

- `PhysicalFileSystem::open_read_shared_file(path) -> File`: the old body of `open_read_shared`, which now wraps it in the same `BufReader`/plain file as before.
- `PhysicalFileSystem::write_all_text_atomic_observed(path, text, durable, observe)`: the old body of `write_all_text_atomic` with `observe(Written, &file)` after the write and `observe(Flush, &file)` only when durable. The trait method calls it with an observer that runs `flush_temp_file` (`File::sync_all`) on `Flush`. `flush_temp_file` increments a thread-local counter (`temp_file_flushes_on_this_thread`) so a test can see the production flush happen.
- Atomicity is untouched: same temp name, `FILE_SHARE_NONE`, `create_new`, rename retry, cleanup of the temp file on every failure. A new test shows a failing flush still leaves no temp file and no target.

## Tests (new file `crates/pr-core/tests/w2_fs_flush_killers.rs`, 9 native tests; plus 1 unit test in `abstractions/source_reader.rs`)

| Test | Pins |
|---|---|
| `durable_atomic_write_opens_the_temp_file_write_through` / `non_durable_atomic_write_does_not_open_write_through` | handle mode bit 0x2 set iff durable |
| `durable_atomic_write_flushes_once_after_writing_and_non_durable_never` | steps are `Written, Flush` (durable) and `Written` (fast) |
| `failed_flush_fails_the_write_and_removes_the_temp_file` | flush error propagates, no temp, no target |
| `trait_path_flushes_the_temp_file_once_when_durable_and_never_when_not` | the production `flush_temp_file` runs once per durable write |
| `trait_write_all_text_atomic_writes_the_complete_file_in_both_modes` | content intact through the trait path |
| `open_read_shared_sets_the_sequential_scan_mode` | handle mode bit 0x4 (INV-8), control: plain open reads 0 |
| `open_read_shared_reads_content_with_and_without_buffer` | the wrapper still reads the file |
| `open_append_durable_is_write_through_and_fast_is_not` | bonus: `open_append` durable flag, via `as_file_mut()` |
| `source_reader::tests::open_shared_sequential_sets_the_sequential_scan_mode` | the second INV-8 open site |

Runs: `stage-native.ps1` done first; `w2_fs_flush_killers -- --include-ignored --test-threads=1` 9/9, repeated 20 times 20/20; regressions `io_physical` 64, `io_physical_seams` 3, `session_store` 45, `session_writer` 48 all green; `cargo fmt -p pr-core --check` and `cargo clippy -p pr-core --all-targets --locked -- -D warnings` clean.

## Mutation

`cargo mutants -p pr-core --file "**/io/physical.rs" -F "write_all_text_atomic|open_read_shared|flush_temp_file|temp_file_flushes"` with `--cargo-arg=--test=w2_fs_flush_killers` alone: **17 found, 17 tested, 15 caught, 2 unviable (`Ok(Default::default())` for `File` and `Box<dyn ReadSeek>`), 0 missed, 0 timeouts** (the earlier 427/427 run covered the pre-split functions).

cargo-mutants does not mutate `if durable` guards or flag constants, so these were applied by hand, one at a time, each run against the killers binary and then reverted; all 7 were caught:

| Hand mutant | Caught by |
|---|---|
| temp-file flags `if durable {WT} else {0}` -> `0` | `durable_atomic_write_opens_the_temp_file_write_through` |
| ... -> always `WT` | `non_durable_atomic_write_does_not_open_write_through` |
| `if durable { observe(Flush) }` -> `if false` | flush-once, failed-flush and trait-count tests |
| ... -> `if true` | flush-once, non-durable and trait-count tests |
| `open_read_shared_file` flag `FILE_FLAG_SEQUENTIAL_SCAN` -> `0` | `open_read_shared_sets_the_sequential_scan_mode` |
| match arm `Flush => flush_temp_file(file)` -> `Ok(())` | `trait_path_flushes_the_temp_file_once_...` |
| `open_append` flags `if durable {WT} else {0}` -> `0` | `open_append_durable_is_write_through_and_fast_is_not` |

The `source_reader.rs` flag was checked by one hand mutant (`custom_flags(0)`): the new unit test fails.

## Verdicts

- `D-05`: Done. The write-through flag is read back from the real handle, the flush is requested exactly once and only when durable, and the production flush function is proved to run. Not testable by any software test and therefore not a row gap: that the medium holds the bytes after `FlushFileBuffers` on power loss (the C# twin `Measure_SessionSaveLatency_DurableVersusNonDurable` is Manual and measures latency only). The single `File::sync_all()` line inside `flush_temp_file` is a std call and is not spied; `flush_temp_file -> Ok(())` is caught through the counter.
- `INV-8` (file-system part): Done. Share mode and rename/delete were already proved; the SequentialScan hint is now proved on both open sites (`open_read_shared`, `open_shared_sequential`). Decoder handles stay in the decoder rows.

## Not run

Full-file cargo-mutants of `physical.rs` was not repeated (only the functions touched by the seam); `pr-bench` and `pr-app` were not rebuilt (trait signatures unchanged). Same-lane gap candidate: `journal_compaction_files.rs` and `atomic_file.rs` also use `FILE_FLAG_SEQUENTIAL_SCAN`; not examined here.
