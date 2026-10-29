# Wave 2 file-system rows: test and mutation evidence (claim w2fs-claude, Machine A, 2026-10-29)

> **Status: evidence of one session on `origin/master` 1b8abe63 plus the killers added by this PR.** It is measured, not
> inferred. It decides the PARITY status of the rows named below; every number comes from a command listed here.
> No production code was changed: no test and no mutant exposed a defect (see "Defects").

Rows covered: `X-physical-fs`, `X-counting-fs`, `D-05` (physical part), `RV-C05`, `AR16`, `SEC-01 / F-05`, `F-23`,
`X-perf-diagnostics`, and the parts of `P-09` that do not need the HTTP lane (`pr_core::updates::checker`).
Side effect: the only gap of `F-22` ("no mutation run exists for `session/writer.rs`") is closed by the F-23 run.

## 1. Test binaries (all green)

Environment: PowerShell, `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, own `CARGO_TARGET_DIR` in the
worktree, `-j 2`. Command per binary:

`cargo test -j 2 -p pr-core --test <binary> -- --include-ignored --test-threads=1`

The native (`#[ignore = "native"]`) tests create files in a `TempRoot` only; none of these binaries touches the Recycle Bin
(grepped for `recycle`, `SHFileOperation`, `IFileOperation` before the run: no hit).

| Binary | Result |
|---|---|
| `io_physical` | 64 passed, 0 failed (includes the native `golden_physical_fs_ops` 145 cases, the junction/reparse tests, `physical_non_durable_write_*`) |
| `io_physical_seams` | 3 passed |
| `io_counting` | 7 passed |
| `session_writer` | 48 passed (47 + the `slow` ignored test) |
| `session_store` | 45 passed |
| `diagnostics_perf` | 17 passed |
| `diagnostics_perf_csv` | 49 passed |
| `diagnostics_review_metrics` | 14 passed |
| `diagnostics` | 30 passed |
| `file_actions_destination_policy` | 20 passed (includes the 2 native reparse-loop tests) |
| `updates_checker` | 34 passed |
| `updates_version` | 21 passed |
| lib unit tests `diagnostics::*` (sha256 vectors, `elapsed_ms`) | 2 passed |
| `w2_fs_mutation_killers` (new) | 17 passed (16 plain, 1 native that runs this binary as a child process) |

C# twins: every public C# test method of `CountingFileSystemTests`, `PhysicalFileSystem*Tests`, `PhysicalIo*Tests`,
`SkippedKindTests`, `SessionWriter*Tests`, `PerfCsv*Tests`, `PhotoReviewPerf*Tests`, `ReviewMetrics*Tests`,
`ActionDestinationPolicyTests`, `FileSystemWrapperForwardingTests` and `PhysicalFileSystemReparseTests` at C# baseline
`7eaee2a2` was looked up by `Class.Method` in the Rust sources: every one has a twin, except the documented type-system
rows (`IsDestinationExists_NonIoExceptionWithTheSameHResult_IsNotRecognized`, `Update_NullSkipped_DoesNotThrow`, negative
debounce rows: a Rust `io::Error`, `Vec` and `Duration` cannot be those values). The helper methods (`Dispose`, `Delay`,
`FireAll`, `Reset`) are not tests.

## 2. Mutation runs

Tool: cargo-mutants 27.1.0 (`cargo mutants -p pr-core --baseline skip --timeout 90 -j 1 --file '**/<path>.rs' -F '<regex>'
--cargo-arg=--test=<binary> --output <dir>`), always `-j 1`, `CARGO_TARGET_DIR` and `--output` outside the worktree
(`.claude/worktrees/_mut-target-w2fs`, `_mut-out-w2fs-*`), one cargo-mutants process at a time, no normal test in that
target. `#[ignore]` tests are skipped by cargo-mutants, so every pass that needs a native test added
`--cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1` (they use `TempRoot` only). Passes that had to stop at
the time box were stopped by killing only the process tree this session started. The union below merges all passes of a file:
a mutant counts as caught when any pass caught it.

| File (mutants listed by `--list`) | Union tested / total | Caught | Timeouts (detected) | Unviable | Survivors left | Test binaries |
|---|---:|---:|---:|---:|---:|---|
| `io/physical.rs` | 427 / 427 | 411 | 3 | 13 | 0 | `io_physical`, `io_physical_seams` (native tests included) |
| `io/counting.rs` | 37 / 37 | 30 | 0 | 7 | 0 | `io_counting`, then the killers for the one survivor |
| `session/writer.rs` | 91 / 91 | 50 | 11 | 29 | 1 (equivalent) | `session_writer`, killers |
| `diagnostics/perf_csv.rs` | 87 / 87 | 61 | 7 | 7 | 12 (all explained below) | `diagnostics_perf_csv`, killers |
| `diagnostics/metrics.rs` | 52 / 52 | 44 | 0 | 8 | 0 | `diagnostics_review_metrics` |
| `diagnostics/perf.rs` | 162 / 162 | 151 | 1 | 3 | 7 (all explained below) | `diagnostics_perf`, lib unit tests, killers |
| `updates/checker.rs` (P-09) | 254 / 355 | 248 | 1 | 4 | 1 (equivalent) | `updates_checker`, killers |
| `file_actions/destination_policy.rs` (SEC-01) | not re-run | | | | | `docs/reviews/W3-05-UNDO-MUTATION.md`: 15 / 15, 12 caught, 3 unviable, 0 missed; the file has not changed since Wave 2 (`git log`: only #7, #8) |

Pass-by-pass numbers (tested; caught / missed / unviable / timeouts):

- `physical.rs`: pass 1 (listed functions, plain tests) 40; 28 / 8 / 1 / 3. The 8 survivors (`final_link_target` guard,
  `strip_nt_prefix` x2, `try_copy_new_with_proof` guard, `write_all_text_atomic` `Ok(())` and `delete !`) were only reachable by
  the native tests. Pass 2 (same functions plus `resolve_*`, `immediate_link_target`, with `--include-ignored`) 35; 35 / 0 / 0 / 0.
  Passes 3-7 (listing, matcher, `create_directory_impl`, `is_device`, decoders, FFI wrappers, thin trait methods, with
  `--include-ignored`): 97 + 185 + 143 + 17 + 1 mutants; 0 missed. Pass 5's `-F` regex had been cut at a space by
  `Start-Process`, so 18 mutants were left out; they were found by comparing the union with `--list` and run in passes 6 and 7.
- `counting.rs`: 37; 29 / 1 / 7 / 0. The one survivor is the `Debug` impl (see 3).
- `writer.rs`: 21 (update, key_of, requeue_skipped, save_batch, wait_for_in_flight); 20 / 1 / 0 / 0. Whole file, 20 minute
  box: 54 reached (20 caught, 3 missed, 25 unviable, 6 timeouts). The 25 remaining function mutants: 14 / 0 / 4 / 7.
- `perf_csv.rs`: 34 (`build_row`, `format_value`, `format_g17`, `csv_escape`); 30 / 2 / 1 / 1. The rest of the file, boxed: the
  `Debug` impl, buffer constants, `writer_loop`, `create_trace_file`, listener methods.
- `metrics.rs`: 24 (bucket index, trim, EWMA, source open/read, presented) 24 caught; the other 28: 20 caught, 8 unviable.
- `perf.rs`: 61 (`sha256`, `path_id`); 57 / 3 / 0 / 1. The other 101: 84 / 14 / 3 / 0 (the 14 are explained below).
- `checker.rs`: 102 (classification, rate limit, `interpret`, Retry-After calendar, leap year, JSON key lookup); 94 / 5 / 3 / 0.
  Then 152 of the remaining 253 before the time box (141 caught, 9 missed, 1 unviable, 1 timeout).

## 3. Survivors and what became of them

Killers are in `crates/pr-core/tests/w2_fs_mutation_killers.rs`; each test carries a comment naming the mutant and the C#
test it extends. After they were written each killed mutant was re-run against the killers binary (and, where the
original test lives in the lib or another binary, together with it) and reported caught.

| Mutant | Verdict | Test or reason |
|---|---|---|
| `session/writer.rs:594:95` `==` -> `!=` in `requeue_skipped` | killed | `requeue_skipped_puts_back_every_drained_folder_not_only_the_first` (a bounded flush that gave up requeues two folders; the second was lost) |
| `session/writer.rs:46:9` `Display for InvalidDebounce` -> `Ok(Default)` | killed | `invalid_debounce_message_names_the_rejected_value` |
| `session/writer.rs:324:9` `Debug for SessionWriter` | killed | `session_writer_debug_output_shows_the_debounce` |
| `io/counting.rs:38:9` `Debug for CountingFileSystem` | killed | `debug_output_of_the_decorators_names_the_type` |
| `session/writer.rs:123:38` `-` -> `+` in `ThreadTimerRun::wait` | equivalent | only the sleep slice changes (up to 10 ms later wake-up); the outcome `Elapsed` is the same; a timing assertion is forbidden (no sleeps) |
| `diagnostics/perf_csv.rs:37:32` `8 * 1024` -> `8 + 1024` | killed | `writer_buffers_rows_in_an_eight_kib_buffer_before_the_sink` (largest chunk the sink sees is above 2000 and at most 8192); the `/` twin timed out |
| `perf_csv.rs:241:9` `Debug for PerfCsvListener` | killed | `perf_csv_listener_debug_output_names_the_type` |
| `perf_csv.rs:511:9` `try_start_from_environment` -> `None` | killed | `try_start_from_environment_reads_the_trace_variable_of_the_process` (native: runs this binary as a child with the variable set; `set_var` is unsafe in edition 2024) |
| `perf_csv.rs:39:31` `64 * 1024` -> `64 + 1024` and `/` | equivalent (timing only) | `FILE_BUFFER` changes only when bytes reach the OS; observable only against the 1 s idle flush, which would make a racy test |
| `perf_csv.rs:157:26` and `:170:24` `>` -> `>=` in `format_g17` | equivalent | at :157 the value is infinite, so it is never 0; at :170 the digit string is never `0` alone because zero returned earlier |
| `perf_csv.rs:366:44` `unflushed > 0` -> `== 0` / `>= 0` | equivalent (timing only) | `== 0` delays the idle flush by one interval, `>= 0` flushes an empty buffer once more; the new `idle_writer_delivers_the_row_itself_before_dispose` pins that the ROW (not only the header) is delivered while idle |
| `perf_csv.rs:439:36/43/90/93` long-path prefix in `create_trace_file`, `:456:5` (`cfg(not(windows))` copy, not compiled here) | equivalent | `std::fs::OpenOptions::open` converts a path over `MAX_PATH` to the verbatim form itself (probed: a 309 character path opens without the explicit prefix); the explicit form is only visible in a path string that is not exposed |
| `diagnostics/perf.rs:152:30`, `:159:31`, `:159:41` `^` -> `|` in `sha256` | equivalent | `Ch` joins disjoint bit sets and `Maj` has at most one term set unless all three are, so xor and or give the same word |
| `perf.rs:38:76` `+ 1` -> `- 1` / `* 1`, `:38:71` | equivalent | the constant offset of `timestamp` is observable only on the first call of the process; callers use differences. `/ -> %` and `/ -> *` are killed by `timestamp_counts_ticks_forward_and_is_never_zero` (needs at least 10 000 ticks per ms and never more than the wall clock) |
| `perf.rs:35:5` `timestamp` -> `1`, `:52:5` `ms_since_process_start` -> `0.0`/`1.0`, `:80:5` `init_process_start` -> `()` | killed | `timestamp_counts_ticks_forward_and_is_never_zero`, `ms_since_process_start_measures_from_the_captured_start_and_grows` (first caller in its binary: an hour-old start must be measured) |
| `perf.rs:58` `elapsed_ms` x4 | killed | the lib unit test `elapsed_ms_converts_ticks_to_milliseconds` (cargo-mutants skips lib tests unless `--cargo-arg=--lib` is passed) |
| `perf.rs:253:70` `==` -> `!=` in `Drop for PerfSubscription` | killed | `dropping_a_subscription_removes_that_listener_and_keeps_the_others` (the old test subscribed the same capture twice) |
| `perf.rs:101:24` `||` -> `&&` in `full_path_or_raw` | equivalent | an empty or NUL path makes `std::path::absolute` fail and the `Err` arm hashes the raw text anyway; `path_id_of_an_empty_or_nul_path_hashes_the_text_as_given` pins the documented divergence (SHA-256 prefixes computed outside the code) |
| `updates/checker.rs:336:9`, `:337:9` `days_in_month` match arms | killed | `retry_after_http_date_accepts_the_last_real_day_of_every_month_and_rejects_the_next` (weekdays computed independently for 2023) |
| `checker.rs:356:20` `*` -> `/` in `weekday_of` | equivalent | `era * 146097`: 146097 days is exactly 20871 weeks, so the term is 0 modulo 7 and only the weekday is used |
| `checker.rs:617:22`, `:617:59` `>` -> `>=` in `find_property` | killed | `lone_surrogate_name_as_long_as_the_key_or_escaping_right_after_it_is_skipped_not_unescaped` |
| `checker.rs:445:13`, `:449:10` (`read_digits`), `:474:27`, `:535:27`, `:543:22`, `:566:15/19/51` (`match_format`) | killed | `retry_after_http_date_enforces_digit_counts_literals_and_zone_signs` (41 of 41 mutants in these lines caught by the killers alone) |
| `checker.rs:96:9`, `:129:9` `Debug` impls | killed | `debug_output_of_the_decorators_names_the_type` |

Also added: `path_id_agrees_with_the_dotnet_full_path_on_windows_path_quirks` (13 quirk paths: trailing space/dot, `/`, `..`,
doubled separators, UNC, `\\?\`, drive-relative). It removes the old note "path_id needs W2-00 `get_full_path`": `path_id`
builds its absolute form with `std::path::absolute` and agrees with `dotnet_path::get_full_path` on all of them and on the 30
golden paths. It killed no mutant.

Manual mutations (no cargo-mutants mutant exists for them), run on a throw-away copy of the tree, never on the worktree:

- RV-C05: removing the best-effort temp cleanup in `write_all_text_atomic` makes `golden_physical_fs_ops`,
  `physical_non_durable_write_failed_rename_cleans_up_temp_file` and `write_all_text_atomic_rename_fails_leaves_no_temp_file`
  fail (3 of 64). The cleanup is guarded.
- D-05: removing `FILE_FLAG_WRITE_THROUGH` and `sync_all` (the "durable" half) leaves `io_physical` (64), `session_store`
  (45) and `session_writer` (48) all green. The durable flush is NOT observable by any test (the C# twin
  `Measure_SessionSaveLatency_DurableVersusNonDurable` is Manual and measures latency only). This is the gap that keeps D-05 Partial.

## 4. Verdicts per row

| Row | Old | New | Why |
|---|---|---|---|
| `X-physical-fs` | Partial (W2-14) | Done | 64 + 3 tests green with native tests, all C# twins present, `physical.rs` 427 / 427 mutants with 0 survivors |
| `X-counting-fs` | Partial (W2-14) | Done | 7 tests, 37 / 37 mutants, the single survivor (`Debug`) killed |
| `D-05` (physical part) | Partial (W2-14) | Partial | everything observable is proved; the durable flush has no observable effect and no test (see above) |
| `RV-C05` | Partial (W2-14) | Done | temp-file cleanup proved by 3 tests and a manual mutation; `write_all_text_atomic` mutants all caught |
| `AR16` | Partial (W2-14) | Done | `enumerate_files_with_stat` and its seam fully mutated (0 survivors); ListingInterrupted and the no-per-file-open probe tests are native and green |
| `SEC-01 / F-05` | Partial | Done | resolver mutants (`resolve_real_path`, `resolve_if_reparse_point`, `final_link_target*`, `immediate_link_target`, `strip_nt_prefix`) all caught; policy 15 / 15 in W3-05; both native fail-closed tests green |
| `F-23` | Partial (W2-13) | Done | `writer.rs` 91 / 91 mutants, 1 equivalent timing mutant; 29 C# twins; the no-twin rows are type-system impossibilities |
| `X-perf-diagnostics` | Partial | Done | `perf.rs` 162 / 162, `perf_csv.rs` 87 / 87, `metrics.rs` 52 / 52 with every survivor killed or explained; goldens and 110 tests green |
| `P-09` | Partial | Partial | `checker.rs` is now mutation tested (254 / 355); the HTTP-lane items (native Settings, TLS downgrade, runtime probes) and 101 `checker.rs` mutants are still open |
| `F-22` | Partial | Done | its only named gap was "no mutation run exists for `session/writer.rs`" |

## 5. What was NOT run

- `checker.rs`: 101 of 355 mutants (the JSON scanner `Scanner::*`, `skip_ws`/`word_at` remainder, `value`/`array`/`object`) and
  everything of the HTTP lane (`pr_platform_win::http`, `pr_shell::composition::update_http`, `pr_app` Settings presenter): TLS
  downgrade, native Settings close/ShellExecute, .NET runtime-default probes (W10-H).
- `file_actions/destination_policy.rs` was not mutated again (W3-05 evidence cited, file unchanged).
- Other files of the rows' crates (`session/store.rs`, `diagnostics/file_log.rs`, `bench_stats.rs`, `diag_options.rs`).
- Test-only constructs: `FILE_BUFFER` and the idle-flush interval have no deterministic observable (see the equivalent rows).
- `cargo test --workspace` and CI-only jobs; only the binaries listed in section 1 were run.
- The `#[cfg(not(windows))]` code of `physical.rs` and `perf_csv.rs` (not compiled on this target).
- `docs/PROPOSED-DECISIONS.md` P-03 ("4 surviving mutants accepted as equivalent", `MAX_FOLLOWED_LINKS` cap and `file_exists_full`):
  superseded by this run (all of them are caught when the native tests are included); the entry was left as it is.

## 6. Defects

None found. No production file was changed; the only added file is the killers test. A mutant that survived was always
either reachable only by a test that cargo-mutants skips (`#[ignore]`, lib unit tests), a weak assertion (listed above), or
equivalent.

## 7. Process notes

- cargo-mutants needs the `-F` regex to contain no space when it is passed through `Start-Process -ArgumentList` (the text
  after the first space is dropped silently and the pass then covers a different set). The `Found N mutants` line was
  compared with the expected count after each pass; pass 5 and three verification passes (`v-writer`, `v-perf`, `v-perf3`) were repeated for that reason.
- Time boxes: passes were stopped after 20 or 25 minutes; a stopped pass is described by the mutants it reached, not as a
  verdict for the file; the union above is what was reached by all passes together.
