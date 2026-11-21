Status: evidence report (2026-11-21), run done, all 33 lane F survivors decided (33 killed by new tests, 0 equivalent, 0 left open). Native mutation lane G for `pr-platform-win`: the survivors of `shell_input/main_window.rs` from [lane F](PLATFORM-NATIVE-F-MUT-2026-11-21.md). Measured on one machine (Windows 11, interactive session 1, owner present), `mutants.sh --native --jobs 1`, `MUT_TIMEOUT=600`, own target dir.

# `pr-platform-win` native lane G: the 33 survivors of `shell_input/main_window.rs`

## Result

Lane F left 33 identities of `main_window.rs` open (host and adapter forwarders). Two new test binaries and two read-only seams kill all of them:

| Survivor group (lane F line) | Killer | Binary |
|---|---|---|
| `has_images -> true` (222), `is_compare_visible -> false` (230), `current_has_compare_pair -> true / false` (234) | a window with no folder reports no images; a single photo has no pair; `photo.jpg` + `photo (1).jpg` is a pair; the host reports exactly what `vm.compare().is_visible()` shows in both states | `nat_main_window_input_host_killers` |
| `next`, `previous -> ()` (242, 246) | `host.next()` x2, `host.previous()` move the selection 0, 1, 2, 1 | host |
| `cancel_duplicate_check -> false` (250) | a duplicate check that hashes (`remove_duplicates`, runner tag `Hashing`) is reported and cancelled, then the host reports false | host |
| `select_compare_side -> ()` (261) | right then left selected on the compare view | host |
| `delete -` of the `-1` of `PreviousFolder` (279:112) | in a folder with no sibling `NextFolder` and `PreviousFolder` end in different boundary statuses | host |
| `action_profile_name -> String::new() / "xyzzy"` (211) | seam P-433: names of profile 0, 1, last, past the end (empty), negative (empty) | host |
| `Debug` of `ZoomPromptOption`, `DialogServiceOption`, `MainWindowInputFeature` (613, 623, 772) | `format!("{:?}")` names the type | host |
| `MainWindowInputFeature::name` (2), `dispose`, `on_current_index_changed`, `on_scroll_bar_press` (778..792) | name is `"input"`; `dispose` removes the `Closed` subscription; a new index and a scroll bar press stop a running glide (fake frame clock subscriber count) | host |
| `MainWindowInput::on_settings_changed -> ()` (512) | the store keeps the settings object the input saw last (`Refresh` saves nothing), `on_settings_changed` is handed settings that bind `Refresh` to F9: F9 refreshes, R no longer does. The old test of `shell_input_surface.rs` could not tell: toggling the info overlay saves a setting, so the lazy rebuild at the next key press hid the missing eager rebuild. | host |
| `ticks_per_ms -> 0.0 / 1.0 / -1.0`, `/` -> `%` / `*` (702) | seam P-432: the real adapter returns exactly 10000.0 (10 MHz stopwatch / 1000) | `nat_main_window_input_adapter_killers` (native) |
| `with_view_model` `delete !` (714:16) | a `CountingFeature` registered on the real window counts applied change sets: a status change is applied once, the same status again and an empty `ChangeSet` are not | adapter |
| `show_status` (722) | the view model's `status_text()` shows it | adapter |
| `folder_open_failed_text`, `action_failed_text` (725, 728), `action_names` (731) | equal to the `pr_l10n::tr` texts, contain the error text and action name, differ from each other, every action name non-empty | adapter |
| `invalidate_layout_and_window -> ()` (747) | `viewport().is_layout_posted()` turns true after the call (the `window.invalidate()` half is part of the same whole-function mutant and is not separately observable here) | adapter |

## Rerun

`mutants.sh --native --jobs 1 --tests nat_main_window_input_host_killers,nat_main_window_input_adapter_killers` on `main_window.rs` with a `-F` regex of the survivor functions (preflight: 17 tests executed, exit 0): **46 identities, 63 minutes, 40 caught, 6 missed, 0 unviable, 0 timeouts**. All 33 lane F survivors and the 2 new seam mutants (`action_profile_name_for_test`) are in the 40 caught. The 6 missed are identities the regex also matched but that these two binaries do not cover; lane F caught all of them with its other binaries (`shell_input_*`, `w8_input*`, `rename_shell_e2e`, `shell_menus_e2e`): `ViewportInputHooks` `Debug` (112), `LoggedError` `Display` (131), `rename_file` (163), `zoom_action_name` x2 (364), `MainWindowInput` `Debug` (379). Line numbers moved by 14 after line 491 (the seams); lane F numbers are the old ones.

The two new binaries ran alone before the mutation run: `nat_main_window_input_host_killers` 13 tests (no desktop), `nat_main_window_input_adapter_killers` 4 native tests (`--include-ignored --test-threads=1`).

## Production change

Two read-only `#[doc(hidden)]` accessors on `MainWindowInput` (no behaviour change): `services_for_test` (P-432) and `action_profile_name_for_test` (P-433). `action_profile_name` is otherwise dead data (the only caller names a failure of `RunAction`, which cannot fail), see [PROPOSED-DECISIONS](../PROPOSED-DECISIONS.md). P-434..P-439 were not needed.

## Notes

- The adapter run opens hidden windows on the owner's desktop; it needs an interactive session.
- Lanes not started after this one: `render/{surface,draw_context}.rs`, `display_clock.rs`, `host/ui_dispatcher.rs`, `instance_scope.rs`, `forward_{pipe,server,client}.rs`.
