# Startup/composition lane verification

Worktree `cx-startup`, branch `feat/cx-startup`, base `45c89c7b`. Execution started 2026-10-29; filename uses the requested 2026-10-30 evidence date. C# oracle is read-only at `7eaee2a2`. No production change. This report records lane-local evidence, not workspace or attended hardware validation.

## Native and ordinary covering tests

`CARGO_TARGET_DIR=<worktree>/target`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, `CARGO_INCREMENTAL=0`. `PHOTOREVIEW_RS_ISOLATE_CONFIG=1` and an isolated data root were supplied; process fixtures use their own TempRoot data roots and instance prefixes. No real user configuration or Recycle Bin was exercised. Source DLLs were staged with `powershell -NoProfile -ExecutionPolicy Bypass -File tools/dev/stage-native.ps1` and verified against pinned SHA-256 values. Git ownership required process-local safe.directory entries for the worktree and C# checkout; global config was unchanged.

All binaries below ran in one bounded 1200-second command, with `--include-ignored --test-threads=1`. Child fixture results are not counted twice. The native-tests desktop probe said false, but the actual native/process/WARP tests below passed in this sandbox session. This is no claim of attended input-desktop or hardware coverage.

| Binary | Passed | Failed | Ignored |
|---|---:|---:|---:|
| `pr-shell lib` | 5 | 0 | 0 |
| `build_res_writer.rs` | 9 | 0 | 0 |
| `build_version_info.rs` | 2 | 0 | 0 |
| `composition_adapters.rs` | 4 | 0 | 0 |
| `composition_decoders.rs` | 13 | 0 | 0 |
| `composition_factories.rs` | 10 | 0 | 0 |
| `composition_graph.rs` | 4 | 0 | 0 |
| `composition_registry.rs` | 31 | 0 | 0 |
| `composition_texts.rs` | 13 | 0 | 0 |
| `entry_args.rs` | 7 | 0 | 0 |
| `exe_process.rs` | 3 | 0 | 0 |
| `exe_resources.rs` | 3 | 0 | 0 |
| `startup_composition_root.rs` | 13 | 0 | 0 |
| `startup_forward_target.rs` | 3 | 0 | 0 |
| `startup_instance.rs` | 1 | 0 | 0 |
| `startup_native.rs` | 3 | 0 | 0 |
| `startup_options.rs` | 2 | 0 | 0 |
| `startup_overlay_vm.rs` | 5 | 0 | 0 |
| `startup_phase_one.rs` | 2 | 0 | 0 |
| `startup_run_native.rs` | 23 | 0 | 0 |
| `startup_secondary_dialogs.rs` | 8 | 0 | 0 |
| `startup_wiring_native.rs` | 13 | 0 | 0 |
| `ui_parity_map.rs` | 6 | 0 | 0 |
| `ui_smoke.rs` | 6 | 0 | 0 |
| `ui_smoke_dialogs.rs` | 7 | 0 | 0 |
| `ui_smoke_overlay.rs` | 6 | 0 | 0 |
| `ui_smoke_startup.rs` | 4 | 0 | 0 |
| `ui_smoke_viewing.rs` | 9 | 0 | 0 |

`composition_decoders` reports 13 pass, but its ORF corpus test returns immediately without `PHOTOREVIEW_RS_RAW_CORPUS_ORF`; no camera corpus evidence is claimed. The missing-ORF LibRaw test actually ran. The existing `ui_smoke_viewing` binary includes a thumb-drag measurement; it passed, but it is not `ui_smoke_perf` or an attended performance verdict. `ui_smoke_perf` was neither edited nor run.

The new `exe_resources::embedded_manifest_matches_the_source_manifest_byte_for_byte` reads the built PE's RT_MANIFEST (type 24, id 1) as data and compares its entire payload to the source manifest. It is independent of the icon check and does not load/execute the PE to inspect resources. Existing manifest marker, icon and ordinary-build no-VERSIONINFO checks also pass. Release builds, Windows Properties, SmartScreen and company/legal review were not repeated here.

The separate bounded `pr-app` run covered `startup_crash` 5, `startup_recovery` 4, `startup_shutdown` 4, `startup_smoke` 3, `startup_placement_read` 1, `startup_settings_load` 7, `startup_ui_hop` 4, `startup_instance` 3, `startup_early_decode` 3: 34 passed, 0 failed. Normal commands never used target-mut.

## Mutation scope and results

Runs: one cargo-mutants process at a time, baseline skip, timeout 90, jobs 1, target-mut, output `mut-out-cx-*`. No selected file has inline unit tests, so no extra `--lib` run. Win32/COM/window composition glue was excluded by policy.

| Group | Tested | Caught | Missed | Unviable | Timeout |
|---|---:|---:|---:|---:|---:|
| pr-app crash, recovery, shutdown, smoke, placement_read (pure 17e) | 23 | 18 | 0 | 5 | 0 |
| pr-app early_decode | 5 | 2 | 1 | 2 | 0 |
| pr-app instance | 9 | 7 | 1 | 1 | 0 |
| pr-app settings_load | 37 | 14 | 21 | 2 | 0 |
| pr-shell startup error | 6 | 0 | 1 | 5 | 0 |
| pr-shell entry | 6 | 6 | 0 | 0 | 0 |
| pr-shell factories | 7 | 1 | 0 | 6 | 0 |
| pr-shell decoders (selected pure-policy subset) | 6 | 5 | 0 | 1 | 0 |
| Total | 99 | 53 | 24 | 22 | 0 |

All 24 survivors were Debug/Display formatters or localized-text adapters whose text was not compared with an independent oracle (no production bug). Seven regressions in `crates/pr-shell/tests/w8_startup_mutation_killers.rs` were added. Re-run of exactly those 24 mutants against the killers binary alone (`--test-package pr-shell --cargo-arg=--test=w8_startup_mutation_killers`): pr-app 23/23 caught (`mut-out-cx-verify-app`), pr-shell error.rs 1/1 caught (`mut-out-cx-verify-error`). Not run: 13 decoder candidates outside the pure-policy subset, native glue.

## Row-by-row reconciliation

- X-exe-manifest: Partial -> Done (PE read-back test plus build_res_writer 9/9).
- X-startup-composition-root, X-startup-main-window, X-startup-overlay-viewmodel, X-startup-options-run, X-startup-settings-load, X-startup-instance, X-startup-pure-17e, X-composition-factories, X-composition-texts-adapters, X-ui-smoke: stay Partial with updated evidence and the exact remaining gaps named in each row (golden, repair-notice UI, unanswered-owner UI, monitor/DPI, ORF corpus, attended `ui_smoke_perf`).
- An exhausted selected mutation set alone does not make an entire row Done.

## Final gates

pr-shell and pr-app: `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` clean; killers binary 7/7, repeated 20x all pass (no clock or timing dependence); doc-links checked at commit. No workspace gate or release build is claimed.
