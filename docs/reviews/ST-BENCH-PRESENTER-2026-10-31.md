# ST-BENCH-PRESENTER: ProductionBenchmarkSession lifecycle tests (2026-10-31, lane `st-bench-presenter`)

Status: evidence report. Branch `st-bench-presenter`. No production change. Row: `X-benchmark-presenter` (PARITY).

## (a) Remaining `dispose` mutants

None is missing a seam. The seam already exists since cl-rest2: `BenchmarkSessionDeps::scratch_parent` and `ProductionBenchmarkSession::scratch_directory()`. [CL-REST2RERUN](CL-REST2RERUN-2026-10-31.md) applied both `dispose` mutants by hand (`-> ()`, delete `!`): both fail the 2 tests of `w8_rest2_killers`; the `Debug::fmt` mutants are killed by `w8_presenter_killers2`. Reading `dispose` again (`if !self.disposed { self.disposed = true; self.executor.dispose(); }`) finds no further operator mutant that the 2 tests miss, except that the "second dispose is a no-op" check is a bounded `yield_now` loop (2000 turns), not a deterministic wait (the executor's `teardown_background_task()` is not reachable through the session). No seam was added; not needed for the mutation claim. I did not run cargo-mutants (user limit).

## (b) New file `crates/pr-app/tests/st_bench_presenter_session.rs` (5 tests, all pass, `cargo test -p pr-app --test st_bench_presenter_session`)

Fakes only: `PixelsCodec`/`FakePayloadCodec`, `NoDecoder`, `FakeRecycleBin`, `TempRoot` scratch parent, a capturing `AppLog` target (so nothing reaches the real log file). No engine iteration runs, nothing is measured.

| Test | Covers |
|---|---|
| `run_rejects_a_blank_folder_and_a_missing_folder_before_any_iteration` | `FolderRequired`, `DirectoryNotFound(path)`, no progress reported |
| `run_with_a_cancelled_token_returns_canceled_and_restores_the_logging_flag` | pre-cancelled run -> `Canceled`; the profile logging switch applied then restored (both `detailed_logging` values); engine entered ("Benchmark start") |
| `dispose_after_a_failed_and_after_a_cancelled_run_still_removes_the_scratch_directory` | create/run/dispose lifecycle; `run` leaves the scratch directory, `dispose` removes it on both paths; parent stays |
| `factory_builds_one_decoder_per_session_and_boxes_a_disposable_session` | `production_session_factory`: decoder per session, double dispose through the trait object |
| `factory_maps_an_executor_construction_failure_to_an_operation_error` | blank file path -> `BenchmarkError::Operation` |

The presenter-level lifecycle with a fake session (create/run/cancel/dispose/finish) is already covered by `benchmark_presenter.rs` (the 32 C# `BenchmarkPresenterTests` ported + 9). The C# has no test of `BenchmarkProfileSession` itself (only `BenchmarkPresenterTests` and the native `BenchmarkDialogTests` use it), so these 5 tests have no C# counterpart.

## Not doable here (needs W10-B2 attended run)

- Cancel in the middle of a real iteration, real progress sequence, a real report from `run`: needs the real engine on a real image folder (measurement); out of scope by instruction.
- Real Recycle Bin path (`PHOTOREVIEW_RS_BENCH_REAL_RECYCLE_BIN=1`) and the live scratch directory with persisted cache files after a real run.
- Human desktop run of the window.

## Proposed PARITY change

None to the status: `X-benchmark-presenter` stays Partial (attended run). Optionally add `st_bench_presenter_session` (5 F tests) to its test list.

Verification: `cargo fmt -p pr-app`, one `cargo clippy -p pr-app --test st_bench_presenter_session -- -D warnings` clean, the test binary 5/5 ok. Nothing else run; CI runs the rest.
