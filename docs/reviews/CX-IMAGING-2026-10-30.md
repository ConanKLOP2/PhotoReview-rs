# Imaging and benchmark follow-up, 2026-10-30

Worktree `cx-imaging`, branch `feat/cx-imaging`, starting HEAD
`45c89c7b9674c85031eea80eca8527781ea3d5e7`. The report date is the owner's
requested evidence date; command timestamps are UTC on 2026-10-29.
C# baseline: `7eaee2a2`, read only. No fixtures, dependencies or production code
changed. Continued after the first agent's quota ended; results below are final.

## Row-to-source and test map

Paths below are relative to `crates/`. Binaries are integration test names unless
`--lib` is explicitly stated. Full file inventories and selected identities are
saved separately; selected coverage never means a whole-file verdict.

| PARITY row | Source | Covering binaries / remaining boundary |
|---|---|---|
| W4-01c | `pr-imaging/src/decoder_traits.rs`, error rebuild in `src/service/mod.rs` | `decoder_traits`, `w4_mutation_killers`, service lib tests; historical incomplete inventory |
| W4-02b | `pr-imaging/src/embedded_thumbnail.rs` | `embedded_thumbnail`; real WIC twin `pr-platform-win: wic_imaging_twins_embedded`; production adapter absent |
| W4-10b | `pr-imaging/src/cache/thumbnails.rs` | `thumbnail_cache`; `pr-platform-win: wic_imaging_twins_thumbnail`; production wiring and independent legacy-WPF comparison |
| W4-11c | `pr-platform-win/src/wic/{decoder,exif_reader,interop}.rs` | `wic_decoder_native`, `wic_decoder`, `wic_exif_reader`, `wic_interop`, `wic_repair1`, `wic_decoder_buffers`; localization OQ1; raw COM excluded from mutation |
| W4-11e | `pr-platform-win/src/wic/{cache_image,cache_adapters}.rs` | `wic_cache_image`, `wic_cache_adapters`, `preview_cache_wic`, `wic_imaging_twins_preview`; independent legacy-WPF pixel reference absent |
| W4-14b | `pr-imaging/src/service/decode.rs`, error predicates in `service/mod.rs` | `preview_service_decode`, `imaging_mutation_killers`; source-byte-wrapped busy/cancel retry and large allocation boundary remain |
| W4-14c | `pr-imaging/src/service/disk.rs` | `preview_service_disk`, lib tests; real codec `pr-platform-win: wic_preview_service_broken_entry`; documented path-id difference |
| W4-14d | `pr-imaging/src/service/mod.rs` PreloadTarget adapter | `preview_service_preload`, `w4_mutation_killers`, lib tests; seeded cache/cancel/rebuilt localized error |
| W4-16a | `pr-imaging/src/pixels.rs` | `pixels`, `pixels_lifetime`, `pixel_memory`, `w4_mutation_killers`; selected span guard, remaining pixel/orientation inventory |
| I-05 | `pr-imaging/src/adaptive_preview.rs` | `adaptive_preview`; App viewport/fallback evidence is a separate boundary |
| I-11 (order, pace, backoff, policy) | `pr-imaging/src/{preload_order,navigation_pace,preload_backoff}.rs`, `preload/policy.rs` | corresponding integration binaries, `verify_navigation_pace`; historical 50 unrun identities are not silently closed |
| W5 final status | imaging `factory.rs`, `fallback.rs`, service/preload integration; RAW/TurboJPEG/LibRaw in other crates | all ordinary imaging binaries; no new full RAW/TurboJPEG/LibRaw mutation verdict, no real camera corpus |
| X-bench-slowlink-fs | `pr-bench/src/slow_link_fs.rs` | `slow_link_fs`; fake clock; enumeration/clock differences remain |
| X-bench-executor | `pr-bench/src/image_executor.rs` | `image_executor`, lib tests; historical formatting/default-log survivors |
| X-bench-workload-runner | `pr-bench/src/workload_runner.rs` | `workload_runner`, lib tests; historical dimension guards and D67 differences |
| X-bench-cli-guard | `pr-bench/src/cli/tool_path_guard.rs` | `cli_tool_path_guard`, lib tests; UNC sandbox failure and native replace difference |
| X-benchmark-compare | `pr-bench/src/compare.rs`, `cli/bench_compare.rs` | `bench_compare`, `bench_compare_cli`, `w7_followup_killers`; engine backend/attended W10-B2 absent |
| X-benchmark-parity-harness | `pr-bench/src/report.rs`, `cli/bench_compare.rs`, `tools/bench-compare.ps1` | `bench_report`, `bench_compare_cli`, `w7_followup_killers`; engine backend/attended W10-B2 absent |

## Production embedded-thumbnail assessment

The production graph still assigns `UnavailableEmbeddedThumbnails` in
`pr-shell/src/composition/platform.rs`; its `open` returns no frame. The existing
working WIC adapter is approximately 125 lines in
`pr-platform-win/tests/common/imaging_twins_wic.rs`, with COM stream/frame/decoder
ownership, metadata queries and format conversion. C#
`src/PhotoReview.Imaging/Decoding/EmbeddedThumbnailReader.cs` at `7eaee2a2`
confirms those calls and the no-full-frame-decode requirement.

This is more than a small wiring change in the assigned `pr-imaging`/`pr-bench`
scope: it requires a new production platform adapter, shell registration, and
native lifetime/composition regressions in two additional crates. It was not
promoted to production here. W4-02b, W4-10b and W4-11c retain that exact gap.

## Method

Read AGENTS mutation rules and LESSONS by ranges. Native DLL staging uses
`powershell -NoProfile -ExecutionPolicy Bypass -File tools/dev/stage-native.ps1`.
Both DLL hashes matched the C# pins. Command-local `safe.directory` exceptions
allowed reading the C# checkout; no global Git configuration was changed.
The desktop probe reported no interactive input desktop; WIC tests do not need
cursor positioning. No real Recycle Bin or real user configuration is used.

Normal builds: `CARGO_TARGET_DIR=$PWD/target`; mutation:
`CARGO_TARGET_DIR=$PWD/target-mut`, `--output $PWD/mut-out-cx-GROUP`.
Both set `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, `CARGO_INCREMENTAL=0`,
`CARGO_BUILD_JOBS=2`, `PHOTOREVIEW_RS_ISOLATE_CONFIG=1` and an isolated
`PHOTOREVIEW_RS_DATA_ROOT` under target. Mutation temp copies live under
`target/mutants-tmp`. One mutation process at a time, `-j 1`, baseline skip,
per-mutant timeout 90 seconds, per-invocation hang guard 1200 seconds.
No ordinary test runs in target-mut. Listing uses matching `--file`, `-F` and
`--list --json`. In-file unit tests receive an additional lib selection where
applicable. Caught logs are checked for relevant assertion failures.

## Ordinary test evidence

- `cargo test -p pr-imaging --locked -- --include-ignored --test-threads=1 --skip samples_wrap_like_csharp`:
  **1240 passed, 0 failed**, one test filtered. Includes the three new W4 killers.
  The original unrestricted debug invocation reached the ignored
  `verify_navigation_pace::samples_wrap_like_csharp`, which loops over 2^31
  records and explicitly requests release. Stopped only its verified executable
  PID 22980 in this worktree; no other process was stopped. That slow test was
  not completed. Log: `target/imaging-final-tests.log`; first attempt:
  `imaging-tests.log`.
- Benchmark lib plus `slow_link_fs`, `image_executor`, `workload_runner`,
  `bench_compare`, `bench_report`, `bench_compare_cli`, `w7_followup_killers`,
  with include-ignored and one test thread: **143 passed, 0 failed**.
  Log: `target/bench-final-tests.log`.
- Separate `cli_tool_path_guard` include-ignored: **16 passed, 1 failed**.
  `missing_share_is_an_io_exception_like_dotnet` receives sandbox
  `UnauthorizedAccessException` rather than expected `IOException` for a
  nonexistent localhost UNC share. Assertion unchanged; this binary cannot
  supply catch evidence while that unrelated baseline failure is active.
  Log: `target/bench-tests.log`.
- An initial new seeded-cache assertion mistakenly assumed 64x48 fake pixels;
  the actual fixture defaults to 4x3, 48 bytes. Corrected the new test to the
  fixture's exact contract; final imaging run is green. No production fix.

## Mutation ledger and final gates

Counts are tested / caught / missed / unviable / timeout. A timeout is an
infinite loop in the mutant (position counter stepped backwards or multiplied);
it is detected by the hang, not by an assertion, and is listed separately.
Output directories: `mut-out-cx-*` (not committed).

| File (group) | Result | Notes |
|---|---|---|
| `pr-imaging/src/decoder_traits.rs` (W4-01c) | 14 / 11 / 0 / 3 / 0 | existing binaries |
| `pr-imaging/src/embedded_thumbnail.rs` (W4-02b) | 13 / 13 / 0 / 0 / 0 | first run 7 caught + 6 timeouts (`is_acceptable_size` filter let the reader allocate huge frames); killer `thumbnail_size_guard_rejects_invalid_sizes_before_any_allocation` re-run alone: 13/13 caught |
| `pr-imaging/src/pixels.rs` (W4-16a, span guard only) | 3 / 3 / 0 / 0 / 0 | `fits_in_span` selected; remaining pixel/orientation inventory not re-run |
| `pr-imaging/src/service/disk.rs` (W4-14c) | 11 / 11 / 0 / 0 / 0 | identity/path group |
| `pr-imaging/src/service/mod.rs` PreloadTarget adapter + error rebuild (W4-14d) | 24 / 20 / 0 / 4 / 0 | first run 19 caught, 1 MISSED (`active_viewer_decodes -> 0`, a coverage gap, no production bug); killer `preload_adapter_reports_active_viewer_while_decoder_is_blocked` (fake decoder gate, no sleep) re-run alone: 6/6 selected mutants caught |
| `pr-imaging/src/adaptive_preview.rs` (I-05) | 32 / 28 / 1 / 3 / 0 | the one survivor, `adaptive_preview.rs:59` `< 1200` -> `<= 1200`, is equivalent: it only multiplies by 1 at exactly 1200 |
| `pr-bench/src/report.rs` (X-benchmark-parity-harness) | 53 / 52 / 1 / 0 / 0 | first run 43 caught + 10 MISSED; new killers kill 9; survivor `report.rs:75` `value < 0.0` -> `<= 0.0` is equivalent (0.0 gives cents 0 so the sign is empty either way) |
| `pr-bench/src/compare.rs` (X-benchmark-compare) | 246 / 221 / 1 / 19 / 5 | first run with 2 test binaries: 156 caught, 67 MISSED (most in the JSON reader, covered by other binaries); with 4 binaries 180 / 45 missed / 2 timeouts; new killers alone then kill every survivor except `compare.rs:328:30` (`delete -` in `incomparable`, workload sentinel `-1` when `row` is `None`, unreachable because `first` is always `Some`: equivalent). Timeouts: `Reader::ws` 604 `+= -> *=`, `Reader::value` 682 `+= -> *=` and 670 `+= -> -=`, `Reader::string` 711 and 727 `+= -> -=` |

Not mutation-tested in this pass (rows keep their historical wording for these):
`pr-bench/src/{slow_link_fs,image_executor,workload_runner,cli/tool_path_guard}.rs`,
`pr-imaging/src/{cache/thumbnails,service/decode,preload_order,navigation_pace,preload_backoff,preload/policy,factory,fallback}.rs`,
the remaining `pixels.rs` inventory and `service/mod.rs` outside the PreloadTarget adapter,
and all of `pr-platform-win/src/wic/*` (raw COM, excluded). The native WIC test
binaries (`wic_*`, `*_twins_*`) were not re-run in the continuation session.

New killers: `crates/pr-imaging/tests/w4_mutation_killers.rs` (5 tests, 20
consecutive passes) and `crates/pr-bench/tests/w7_followup_killers.rs` (14 tests,
20 consecutive passes). Both are deterministic: TempRoot and fakes, no sleeps, no
clocks. One caveat found: cargo-mutants may report a surviving mutant that
was not recompiled when two mutants land in the same timestamp tick; every
"survivor" above was re-run in a separate invocation.

Gates: `cargo fmt -p pr-imaging --check`, `cargo fmt -p pr-bench --check`,
`cargo clippy -p <crate> --all-targets --locked -- -D warnings` for both crates,
both killers binaries 20x green, `python -I tools/dev/doc-links.py --quiet`.

No production code changed. Embedded-thumbnail production wiring (W4-02b) is
documented above, not done: it needs a new platform adapter in `pr-platform-win`
plus shell registration, outside this lane. All rows stay Partial.
