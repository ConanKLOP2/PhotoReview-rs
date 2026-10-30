# CL-CINATIVE 2026-10-30: nightly native test workflow

Status: done (lane cl-cinative). Closes cross-cutting finding 1 of [AUDIT-DONE-2026-10-30.md](AUDIT-DONE-2026-10-30.md): CI never ran `--include-ignored`.

## What was added

`.github/workflows/native-nightly.yml`: `schedule` (18:30 UTC) and `workflow_dispatch`, `windows-latest`, `permissions: contents: read`,
`concurrency: native-nightly` (no overlap, no cancel), 60 min job limit, same pinned `checkout` and `rust-cache` actions as `ci.yml` (cache saved only on master).
`ci.yml` is unchanged; the new job is not in `needs` of `build-test`, so it is not a required check. Every binary runs as
`cargo test -p <crate> --locked --test <bin> -- --include-ignored --test-threads=1` with `PHOTOREVIEW_RS_ISOLATE_CONFIG=1` and
`PHOTOREVIEW_RS_DATA_ROOT=$RUNNER_TEMP/photoreview-native`. A failing binary does not stop the others; the step fails at the end listing them.

## Binaries run

- pr-core (17): catalog_sorting, file_actions_destination_policy, file_actions_journal, file_actions_journal_compaction, file_actions_partial_cleanup,
  file_actions_recovery_check, file_actions_service_native, io_journal_compaction_files, io_physical, io_physical_seams, journal_reconcile,
  journal_startup_recovery, rename_file_action_matrix, rename_recovery, rename_service, session_store, w2_fs_mutation_killers.
- pr-platform-win (4): w2_fs_flush_killers, w4_mutation_killers2, w6_platform_native, physical_memory.
- pr-app (2): folder_load_load, windowing_placement_store.

## Left out

- Need pinned DLLs (stage-native.ps1 copies from the local C# checkout; not available in CI; fetching is blocked by P-55): pr-libraw (availability, decoder_native, native, synthetic_raw), pr-turbojpeg (availability, decoder_native, native, native_golden), pr-imaging-raw raf_reader, pr-imaging cache_key / cache_preview_file / memory_headroom / preview_service_disk, pr-platform-win wic_imaging_twins_preview and wic_preview_service_broken_entry.
- Need a real desktop, DWM/GPU or the real cursor: window families of pr-platform-win and pr-shell. The runner's hidden-window support was not verified, so none were added; a later lane can try one family with `workflow_dispatch`.
- `pr-app/overlay_button` has no `#[ignore]` native test (it runs in the normal test job); `pr-app/shell_viewing_image_layer` only mentions a native twin in a comment.
- pr-bench, pr-shell, pr-test-support, pr-imaging remaining files: tool paths, process start-up or DLLs; out of scope.

## Local check

- `file_actions_destination_policy` (pr-core): 20 passed. `w2_fs_flush_killers` (pr-platform-win): 9 passed. Same command and env vars as the workflow.
- YAML parses with PyYAML (keys name, on, permissions, concurrency, env, jobs). actionlint is not installed and was not installed. The workflow was not run on GitHub; the first `workflow_dispatch` run is the real test.
- `python -I tools/dev/doc-links.py --quiet`: see the PR.
- PARITY.md was not edited: the rows named by the audit (X-physical-fs, AR16, SEC-01, D-05, INV-8, F-16) keep their status and wording; the job is documented in docs/DEVELOPING.md.
