# ST-BENCH-BACKEND 2026-10-31: engine-backed benchmark backend

Status: report. Lane `st-bench-backend`. Source task: [ST-GAPMAP2](ST-GAPMAP2-2026-10-31.md) section 6, item 1 (replace
`UnwiredBackend` in `crates/pr-bench/src/cli/backend.rs`).

## What was done

- New `crates/pr-bench/src/cli/engine_backend.rs`: `EngineBackend<H: EngineHost>` implements `ProfileCatalog` and
  `BenchmarkBackend` over the real code of the crate:
  - catalog: `profiles::all/runnable/find` (30 profiles, 27 runnable; 1 correctness-only and 2 not-implemented are excluded
    from `runnable`);
  - `run_profile`: registry lookup, `BenchmarkImageExecutor::new` (errors mapped to `ArgumentException` /
    `InvalidOperationException`), `create_seeded_random`, `BenchmarkProfileScope::apply_logging` (flag set for the run and
    restored), `workload_runner::run_profile` on a scoped worker thread so `BenchmarkProgress` reaches the CLI callback
    live and in order, `executor.dispose()`, then the report and `report.Phases[0]` are stored (C# `reports`/`outcomes`);
  - `record_failed_profile`, `report_json`, `batch_summary_json` over `BenchmarkReport::to_json` /
    `BenchmarkBatchSummary::to_json` with the registry images-per-sample lookup.
- Platform pieces come from an `EngineHost` trait (decoder, platform codec, payload codec, memory probe/RAM, real Recycle
  Bin opt-in, log, `AppLog.Enabled` get/set, machine name). `UnwiredEngineHost` is the host of the binary today.
- `crates/pr-bench/src/main.rs` now uses `EngineBackend::new(UnwiredEngineHost)`. Consequence: `--benchmark-list-profiles`
  prints the real catalog; a profile run fails per profile with `the benchmark engine is not wired into this build`
  (batch continues, exit 1), exactly as before for runs. `UnwiredBackend` is kept (still exported, tests use the trait only).
- Un-ignored `tests/cli_process.rs::list_profiles_prints_eight_tab_separated_fields_per_line` (it needed the real catalog).

## What is NOT done (needs hardware or a crate that can name the platform)

- A real `EngineHost`. `pr-platform-win` depends on `pr-app`, which depends on `pr-bench` (D-W8B1-1), so the `pr-bench`
  binary cannot name `WicDecoder`, `WicPreviewPayloadCodec`, `WindowsMemoryProbe` or `WindowsRecycleBin` without a cycle.
  Options for the lead: (a) a host crate or `pr-app` feature that builds the `pr-bench` CLI with a Windows host; (b) move
  the binary out of `pr-bench`. This is a design decision, not done here.
- The logging write counter (`getWriteCount` of `ApplyLogging`) is passed as `None`; `FileLog` exposes no write count.
  Only matters when something else toggles logging during a run.
- No measurement exists: no timing, no attended W10-B2 run, no performance parity claim. The fake-host test proves
  wiring only.

## Tests (run on this machine)

- `cargo test -p pr-bench --test cli_engine_backend --test cli_process -j 2`: 9 + 2 passed. New file
  `crates/pr-bench/tests/cli_engine_backend.rs`: catalog (30/27, fields, FileAction flag), unwired host error and nothing
  stored, unknown profile, failed profile report + summary JSON, call order, a full `fast-sequential` run over the fake
  decoder (ordered progress, stored report, machine name, report/summary JSON) and the logging flag set/restored for
  `logging-on`. Files are placeholders in a `TempRoot`; the recycle bin is the temp-copy deleter, never the real one.
- `cargo check -p pr-bench --tests`, `cargo clippy -p pr-bench --all-targets -- -D warnings`, `cargo fmt -p pr-bench --check`:
  clean. Other pr-bench test binaries and the workspace were not run (CI).

## Suggested PARITY changes (not applied; orchestrator owns PARITY.md)

- X-benchmark-compare (L24), X-benchmark-parity-harness (L25): replace "engine-backed backend not wired" with "engine
  backend wired through `EngineHost`; no real host (WIC decoder, bin) in the binary; W10-B2 attended measurement not done".
  Stay Partial.
- X-bench-cli-batch (L504): "`main.rs` still uses `UnwiredBackend`" becomes "`main.rs` uses `EngineBackend` with
  `UnwiredEngineHost`"; add `tests/cli_engine_backend.rs` (9) as evidence. Stays Partial/Different.
