# Wave 8 presenters: postmerge mutation verification (2026-10-29, Machine A, claim `w8pres-claude`)

Status: verification report. Base `1b8abe63` (master after #343), branch `feat/claude-w8pres`. C# reference not needed: no production code changed and no mutant exposed a bug.
Rows: `X-settings-presenter`, `X-recovery-window-presenter`, `X-benchmark-presenter`, `X-zoom-detail-loader` (docs/PARITY.md). Killers: `crates/pr-app/tests/w8_presenter_mutation_killers.rs` (15 tests, fakes and `TempRoot` only, no sleeps, no real Recycle Bin).

## Method

`cargo mutants -p pr-app --baseline skip --timeout 90 -j 1 --file '**/<path>.rs' [-F '<regex>'] --cargo-arg=--test=<binary>[ ...]`, one cargo-mutants process at a time, its own `CARGO_TARGET_DIR` outside the worktree
(`.claude/worktrees/_mut-target-w8pres`), output in `.claude/worktrees/_mut-out-w8pres-<group>`. Whole files were mutated (no `-F`): after the first cold build a mutant cost 10 to 17 s, not 60 s, so the
about 20 minute per-file budget covered every file except `zoom_detail.rs` (38 minutes, 9 timeouts of 90 s each) and `benchmark.rs` (24 minutes). Normal tests (killers, clippy) used a separate target dir.

## First pass (before the killers)

| Group (file) | Test binaries | Tested | Caught | Missed | Unviable | Timeout |
|--------------|---------------|-------:|-------:|-------:|---------:|--------:|
| `settings/validate.rs` + `settings/shortcuts.rs` | `settings_presenter_save`, `_shortcuts`, `_load`, `_apply` | 25 | 22 | 0 | 3 | 0 |
| `settings/apply.rs` | `settings_presenter_apply` | 68 | 40 | 7 | 21 | 0 |
| `settings/load.rs` | `settings_presenter_load`, `_save`, `_apply` | 86 | 79 | 7 | 0 | 0 |
| `recovery_window.rs` | `recovery_window_presenter` | 96 | 77 | 6 | 13 | 0 |
| `coordinators/zoom_detail.rs` | `zoom_detail`, `viewmodels_main_navigation` | 89 | 59 | 14 | 7 | 9 |
| `viewmodels/main/commands.rs` (`-F on_viewer_zoom_mode_changed`, matched 2 mutants) | `viewmodels_main_navigation`, `zoom_detail` | 2 | 1 | 1 | 0 | 0 |
| `benchmark.rs` | `benchmark_presenter` | 114 | 82 | 13 | 18 | 1 |
| Total | | 480 | 360 | 48 | 62 | 10 |

The 9 `zoom_detail.rs` timeouts are `CompletionInbox::{send, drain, len, is_empty}`, `ThreadRunner::run`, `pending_load`, `queued_events` and `pump` replaced by constants: the test binary hangs on a hang guard, which counts as detected. The `benchmark.rs` timeout is `on_profile_finished` `+=` to `*=` (the run never advances). The `commands.rs` miss is the incidental `refresh_view` `viewer` field mutant, which `viewmodels_main_commands` catches (below); `on_viewer_zoom_mode_changed -> Default::default()` is caught by `viewmodels_main_navigation`.

## Survivors and dispositions

Killed (test names in `w8_presenter_mutation_killers.rs`, each with a comment naming file:line and operator):

| Mutant | Killer |
|--------|--------|
| `apply.rs:125` Debug of `UpdateCheckJob` | `update_check_job_debug_names_the_type_and_the_version` |
| `apply.rs:334` `context_menu_item_label -> "xyzzy"` | `context_menu_item_labels_are_the_localized_item_texts_and_distinct` |
| `apply.rs:410` `reload_translations -> None` | `reload_translations_with_a_service_reports_a_result` |
| `apply.rs:657` `set_update_url` no-op | `set_update_url_stores_the_url_that_open_update_page_validates_and_opens` |
| `load.rs:125`, `:180` Debug impls | `settings_presenter_and_options_debug_name_the_type_and_fields` |
| `load.rs:246` `%` to `+` in `next_page_index` | `next_page_index_wraps_with_the_truncating_remainder_of_csharp` (equivalent for in-range input, differs for `-30`) |
| `load.rs:542` `<` to `==`/`<=` in `one_decimal_half_up` | `ram_cache_hint_rounds_a_value_between_one_and_ten_hundredths_of_a_gib_half_up` (64 MiB must read `0.1`) |
| `recovery_window.rs:83` guard `!members.is_empty()` to `true` | `recovery_row_with_an_empty_member_list_is_titled_by_the_file_name_alone` |
| `recovery_window.rs:217, :254, :306, :342` Debug impls | `recovery_debug_impls_name_their_types` |
| `recovery_window.rs:701` `<` to `<=` in `can_start_retry` | `can_start_retry_refuses_an_index_equal_to_the_row_count` |
| `zoom_detail.rs:82` Debug of `CompletionInbox`, `:121` `is_empty -> true` | `completion_inbox_reports_emptiness_and_has_a_named_debug` (also fails the `-> false` timeout twin at once) |
| `zoom_detail.rs:196` `+` to `-` in `default_indicator_delay` | `production_indicator_delay_waits_at_least_the_requested_time` (lower bound on a monotonic clock) |
| `zoom_detail.rs:274` Debug of `ZoomDetailLoader` | `zoom_detail_loader_debug_names_the_type_and_state` |
| `benchmark.rs:131, :179, :255, :310` Debug of scan job, profile job, save job, presenter | `benchmark_step_jobs_have_named_debug_output` (drives a two-profile run through to the save step with fakes) |
| `zoom_detail.rs:291, 307-309, 312` `retarget_path` | already killed by `presenter_rename` and `rename_presenter_matrix` (the first pass did not include those binaries) |
| `commands.rs:533` `refresh_view` `viewer` field | already killed by `viewmodels_main_commands` |

Not killed, with reason:

| Mutant | Disposition |
|--------|-------------|
| `apply.rs:466` `try_start_explorer -> None / Some("") / Some("xyzzy")` (3) | Not testable: the function spawns the real `explorer.exe` (a window on the desktop); C# has no test either. |
| `load.rs:276` `<` to `<=`, `:279` `>` to `>=` in `load_fields` | Equivalent: at equality the branch assigns the value it already has. |
| `benchmark.rs:390` `>` to `>=` in `apply_image_limit` | Equivalent: with `len == limit` the slice `[..limit]` is the whole list. |
| `benchmark.rs:832`, `:833` (4) in `enumerate_image_files` (`seen +=`, `== 0`, `%`) | Equivalent in outcome: they only change how often the token is polled; the poll after the loop returns the same `Canceled`. |
| `benchmark.rs:900`, `:927` Debug of `BenchmarkSessionDeps` and `ProductionBenchmarkSession`, `:1004` `dispose` no-op and `delete !` | NOT killed and not run by any test: needs the composition-root codecs (W8-17c, see the PARITY row). |
| `zoom_detail.rs:306` `retarget_path` without the new target `id` | NOT killed. `cancel_pending()` cancels the running old-path decode and the fake decoder honours the cancel, so the stale completion is dropped either way. Killing it needs a fake decoder that ignores the cancel; `retarget_path_with_a_decode_in_flight_shows_only_the_new_paths_pixels` pins the visible outcome but does not discriminate. |
| `zoom_detail.rs:442` `held \|\| showing_original` to `&&` in `refresh` | Equivalent: after `update` with the refresh hold set, `showing_original` is true exactly when an original was held. |
| `zoom_detail.rs:604` `load.id == load_id` to `!=` | Log text only (`ms=` of the "original ready" line), no state. |
| `zoom_detail.rs:632` delete `-` of `(-1, "")` | Log text only (`token=-1` of the failure line when the target is gone). |
| `zoom_detail.rs:705` `slot.id == indicator_id && !cancelled` to `\|\|` | NOT killed: needs a stale indicator event arriving while a newer, uncancelled indicator slot exists (the injected `IndicatorGates` plus a RAW fixture). Open. |

## Re-runs (proof)

| Run | Binaries | Tested | Caught | Missed | Unviable |
|-----|----------|-------:|-------:|-------:|---------:|
| killed mutants alone (`-F` of each killed mutant text, 5 files) | `w8_presenter_mutation_killers` only | 31 | 27 | 4 (incidental `retarget_path` field deletions, see next row) | 0 |
| `zoom_detail.rs` `retarget_path`, `refresh`, `on_original_decoded`, `on_indicator_elapsed` | `zoom_detail`, `presenter_rename`, `rename_presenter_matrix`, `viewmodels_main_navigation` | 27 | 21 | 5 (the survivors listed above) | 1 |
| `commands.rs` `on_viewer_zoom_mode_changed` and `refresh_view` | `viewmodels_main_commands` | 6 | 4 | 1 (`on_viewer_zoom_mode_changed -> Default`, caught by `viewmodels_main_navigation` in the first pass) | 1 |

Every mutant that a killer targets is caught by the killers binary alone. The `retarget_path` `id` mutant was also run against the killers binary once more (`-F 'delete.field.id.from.struct.Target.expression'`): missed, as stated above.

## Gate

`cargo fmt -p pr-app --check`, `cargo clippy -p pr-app --all-targets --locked -- -D warnings` and `cargo test -p pr-app --test w8_presenter_mutation_killers` (15 passed) are clean; `python -I tools/dev/doc-links.py --quiet`.

## What was NOT run

- Mutation of `settings_field_map.rs`, `settings_form_state.rs`, `action_profiles.rs`, `action_list_json.rs`, `recovery_filter.rs`, `recovery_presentation.rs`, `benchmark_presentation.rs` (other rows or already covered by golden tests).
- Mutation of `viewmodels/main/commands.rs` beyond the `-F on_viewer_zoom_mode_changed` selection (2 mutants plus the incidental `refresh_view` one).
- Any native or desktop run (W8-18r/18s/18t..18v, W8-19), the production benchmark session, and a real camera RAW corpus for the zoom loader.
- The rows stay `Partial`: each still names an open gap (settings native dialog run and seams, recovery/benchmark desktop runs, the production benchmark session, the zoom `on_indicator_elapsed` and `retarget_path` `id` survivors).
