Status: evidence report (2026-11-21), run done, survivors NOT yet decided. Native mutation lane F for `pr-platform-win`: whole-file run of `shell_input/main_window.rs`. Measured on one machine (Windows 11, interactive session 1, owner present), `mutants.sh --native --jobs 1`, `MUT_TIMEOUT=600`, own target dir.

# `pr-platform-win` native lane F: `shell_input/main_window.rs` (survivors open)

## Result

98 identities, 3 hours: **45 caught, 33 missed, 20 unviable, 0 timeouts**. This note records the run; the 33 survivors are **open** (no killer test written, none argued equivalent yet). It is not a closed verdict.

Test binaries (`--include-ignored --test-threads=1`, preflight 267 tests, exit 0): the `shell_input_*` and `w8_input*` set of lane C, `nat_surface_forwarder_killers`, `nat_dispatch_killers`, `rename_shell_e2e`, `shell_menus_e2e`. A first preflight failed once on `shell_input_pan_glide_native::a_lost_capture_cancels_the_drag_and_no_glide_follows` (real cursor input on the owner's desktop while the test ran; it passed on the next run), so a desktop that is being used can make native tests flaky.

## The 33 open survivors (by function)

| Lines | Mutants | What they are |
|---|---|---|
| 211 | `MainWindowInputHost::action_profile_name -> String::new() / "xyzzy"` | the name of the profile of `RunAction`, used in the failure text |
| 222, 230, 234 | `ShellInputHost::has_images -> true`, `is_compare_visible -> false`, `current_has_compare_pair -> true / false` | host queries forwarded to the view model |
| 242, 246, 250, 261 | `next`, `previous` -> `()`, `cancel_duplicate_check -> false`, `select_compare_side -> ()` | host actions forwarded to the view model |
| 279:112 | `execute`: `delete -` (the `-1` direction of `PreviousFolder`) | |
| 512 | `MainWindowInput::on_settings_changed -> ()` | |
| 613, 623, 772 | `Debug` of `ZoomPromptOption`, `DialogServiceOption`, `MainWindowInputFeature` | formatter output never read |
| 702 | `MainWindowInputAdapter::ticks_per_ms` -> 0.0 / 1.0 / -1.0, `/` -> `%` / `*` | |
| 714:16 | `with_view_model` `delete !` | |
| 722, 725, 728, 731, 747 | `show_status`, `folder_open_failed_text` (2), `action_failed_text` (2), `action_names`, `invalidate_layout_and_window` | adapter forwarders |
| 778, 782, 788, 792 | `MainWindowInputFeature::name` (2), `dispose`, `on_current_index_changed`, `on_scroll_bar_press` | the feature glue |

Why they are open: these are forwarders of the production adapters over a real `MainWindow` and `MainViewModel` (`MainWindowInputHost`, `MainWindowInputAdapter`, `MainWindowInputFeature`). The existing coverage is the native `shell_input_e2e` set (a whole hidden window per test) plus the fake-services `FeatureRig`, which replaces exactly these adapters. Killing them needs either assertions in the e2e rig that look at each forwarded effect (view-model state after `next`, `select_compare_side`, `cancel_duplicate_check`, status text) or a fake-services twin of the adapters; the cost was judged to be a separate unit of work. Nothing was changed.

## Notes

- The run opens hidden windows on the owner's desktop; it needs an interactive session.
- Lanes not started after this one: `render/{surface,draw_context}.rs`, `display_clock.rs`, `host/ui_dispatcher.rs`, `instance_scope.rs`, `forward_{pipe,server,client}.rs`.
