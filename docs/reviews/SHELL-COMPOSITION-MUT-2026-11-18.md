Status: evidence report (2026-11-18). Whole-file mutation run of the eight pure `pr-shell/src/composition/` files (`contracts`, `decoders`, `factories`, `graph`, `main_view_model`, `registry`, `texts`, `update_http`) against the whole `pr-shell` suite (native `#[ignore]` tests not run), plus a confirmation run of the killers added. Measured on one machine (Windows 11), `--jobs 1`, `MUT_TIMEOUT=900`, 85 minutes.

# `pr-shell` composition: mutation verdict

## Result (first run)

272 identities: 200 caught, 51 unviable, 21 missed, 0 timeout. All 21 survivors were in `contracts.rs`, `graph.rs`, `main_view_model.rs` and `registry.rs`; `decoders.rs`, `factories.rs`, `texts.rs` and `update_http.rs` had none.

## Survivors and verdicts

New file `crates/pr-shell/tests/composition_lane_e_killers.rs` (18 tests); test-only change in `tests/common/composition_fixture.rs` (builder split, new `shared_service_inputs()`). No product code change, no seam.

| Mutants | Verdict | Killer |
|---|---|---|
| `Debug` impls: `contracts.rs:32`, `graph.rs:41`, `main_view_model.rs:55/77/143`, `registry.rs:96/216` (7) | killed | `debug_of_*` tests assert type name and key fields |
| `registry.rs:383/385/388/389/391` deleted `OperationJournalOptions` fields (durability, compaction_files, log, texts, recycle_bin) (5) | killed | `journal_durability_follows_the_settings_at_every_read`, `journal_is_wired_with_compaction_files_so_it_can_compact`, `journal_diagnostics_go_to_the_injected_log`, `journal_uses_the_localized_texts`, `journal_reconcile_asks_the_platform_recycle_bin` |
| `registry.rs:514` `viewport_size_source` -> `Arc::new(Default::default())` | killed | `viewport_size_source_is_one_shared_instance` |
| `registry.rs:572:21`, `572:49` in `decoder_factory` | killed, but only where `libraw.dll` fails to load | three `libraw_*` tests read the process AppLog for the LibRaw forced-error line; they pass vacuously on a machine where the DLL loads next to the exe |
| `registry.rs:643/644` deleted `ThumbnailCacheOptions` fields (log, clock) | killed | `thumbnail_cache_sweep_failures_go_to_the_injected_log`, `thumbnail_cache_startup_sweep_uses_the_injected_clock` |
| `registry.rs:784/785` deleted `PreloadSchedulerDeps` fields (log, snapshot_version) | killed | `preload_scheduler_writes_to_the_injected_log`, `preload_scheduler_reads_the_snapshot_version_it_was_given` |
| `registry.rs:749:45` `&&` -> `\|\|`, `749:72` `==` -> `!=` in `preview_service` | equivalent in this environment | the closure matters only when `raw_full_decoder` is `Some`, which needs `pr_libraw::availability::probe().available`; no `libraw.dll` sits next to the test exe here. `preview_service_raw_full_decode_needs_raw_support_and_on_zoom` has the three discriminating cases and would kill both on a machine where the DLL loads (it returns early here). Open: run it once on such a machine |

## Confirmation

Rerun of the 21 positions with `--tests composition_lane_e_killers`: 22 identities (the filter pulled in `registry.rs:384:21`), 19 caught, 3 missed: the 749 pair and `384:21` (`live_operations` deleted), which `journal_uses_the_platform_live_operation_registry` in `composition_registry.rs` already kills in the whole suite.
