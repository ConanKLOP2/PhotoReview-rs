# Native evidence run, 2026-10-29 (claim PARTIAL-native-evidence, machine A)

Status: complete for the binaries listed. Every binary below was run on this machine's real desktop with
`cargo test -p <crate> --locked --test <binary> -- --include-ignored --test-threads=1` (library tests: `--lib <filter>`),
one native process at a time, `target\debug` and `target\debug\deps` staged with `tools/dev/stage-native.ps1`,
`CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, own `CARGO_TARGET_DIR` in the worktree.
Head: master 6ec2ec7a. Counts are `passed` (the ignored native tests are included in it because of `--include-ignored`);
failed was 0 and ignored 0 for every binary. No production code or test was changed.

## Result

87 binary runs, 0 failures, 0 timeouts. No defect found in production code, tests or the rig.
Skipped for safety: none (a grep for Recycle Bin access found only fakes: command enums, `TempFolderRecycleBin`,
`FileOperationType::Recycle` seeds in an isolated journal; none of the binaries touches the real bin).

Findings that are not failures:

- `shell_input_dispatch::native::g_input_every_recorded_script_replays_through_real_messages` passes by SKIPPING
  (the `input-scripts` golden is not synced). The G-INPUT replay is still unproven; X-input-shell-input stays Partial.
- `host_shell_window` prints a skip for the executable icon resource (test exe has no RT_GROUP_ICON).
- `DialogChrome::default_chrome` in `dialogs/native/dialog/core.rs` still builds `NoPainter`; the painter rows keep this as a gap.
- `host_ui_dispatcher_native` still drives a test-local twin of the `MessageLoop` pump; `host_message_loop_native` still uses the tiny
  dispatcher of `tests/common/ui_thread.rs`. No test runs the real `MessageLoop` with the real `Win32UiDispatcher` (open follow-up).
- Stale claims removed from rows: "temporary local stand-ins for the 13i/13j/13k stubs" (X-main-window-core/lifecycle/viewing/windowing),
  "12d smoke test not written because `create_window_surface` is still a stub" (it exists), "desktop run pending" (X-startup-*),
  "5 zoom e2e tests fail on a viewer RefCell double borrow" (X-menu-main-window, 22/22 pass now),
  "popup capture ports wait for W8-14f" (X-menu-owner-draw-session), "run once by hand offscreen" (painter rows).

## Table

| Row | Binaries (counts) | Old -> new | Remaining gaps |
| --- | --- | --- | --- |
| X-host-swap-chain-clock / -shell-contracts / -icons | host_swap_chain_clock 16, host_icons 7 | Partial -> Partial | `host::native` wrappers only through icons, exe icon resource (W8-20a), 12d surface smoke not in these binaries, no mutation |
| X-host-message-loop | host_message_loop 30, host_message_loop_native 10 | Partial -> Partial | real loop never driven with the real `Win32UiDispatcher` |
| X-host-ui-dispatcher | host_ui_dispatcher 9, host_ui_dispatcher_native 22 | Partial -> Partial | native tests use a test-local pump twin, not the real `MessageLoop`/`pending_panic` |
| X-host-wndproc-shell-window | host_wndproc 23, host_shell_window 32, host_shell_window_class_conflict 1 | Partial -> Partial | `show` of a visible window, exe icon resource branch (skips), no mutation |
| X-viewing-compare-layer | shell_viewing_compare_crossfade 29, shell_viewing_crossfade_native 7 | Partial -> Partial | C# scene golden not loaded, WARP only, no mutation |
| X-viewing-crossfade | same two binaries | Partial -> Partial | same, plus 2 C# tests without a twin |
| X-viewing-composer | shell_viewing_composer 56, shell_viewing_composer_native 4 | Partial -> Partial | 3 native twin families not ported, WARP only, no mutation |
| X-input-shell-input | shell_input_dispatch 16, _keys 21, _pointer 30 | Partial -> Partial | G-INPUT replay passes by skipping (golden not synced), mutation debt |
| X-input-main-window | shell_input_e2e 15, shell_input_surface 27 | Partial -> Partial | E2E drives the fixture, not `ShellStartup`; mutation debt |
| X-menu-native-popup-host | shell_menus_host 28 | Partial -> Partial | popup pixels depend on font/DPI, mutation debt |
| X-menu-main-window | shell_menus_main_window 20, shell_menus_e2e 22 | Partial -> Partial | single font/DPI, mutation debt |
| X-menu-owner-draw-session | shell_menus_owner_draw 16 | Partial -> Partial | font/DPI-dependent pixels, `owner_draw.rs` not mutated |
| X-main-window-core | host_main_window_core 28 | Partial -> Partial | mutation debt (14 missed, registry mutants), zero-client case |
| X-main-window-lifecycle | host_main_window_lifecycle 23 | Partial -> Partial | minimise/restore, F11 placement, panicking `close_session`, mutation survivors |
| X-main-window-frame | host_main_window_frame 18 | Partial -> Partial | minimised window, deferred frame, bar-press fan-out, 7 bar E2E, mutation survivors |
| X-main-window-viewing | host_main_window_viewing 14 | Partial -> Partial | `update_retained`, `PrefetchHost`, `draw_presentation`, device-lost crossfade, 100-step navigation, `path_at -> None` |
| X-main-window-windowing | host_main_window_windowing 38, dark_title_bar lib 3 | Partial -> Partial | fullscreen frame geometry tests, real F11 max/min, multi-monitor unplug, mutation survivors |
| X-dialogs-painter | dialogs_painter_geometry 17, dialogs_painter_attach 7 | Partial -> Partial | mutation, `default_chrome` still `NoPainter`, font/DPI pixels |
| X-chrome-painter-controls | dialogs_painter_controls 18 | Partial -> Partial | 144 dpi test, empty-rect `fill` test, mutation |
| X-chrome-painter-widgets | dialogs_painter_widgets 17 | Partial -> **Done** | none named in the row (no mutation verdict claimed) |
| X-chrome-painter-frame | dialogs_painter_frame 13 | Partial -> Partial | etched horz/vert not measured, production wiring (`NoPainter`), mutation |
| X-dialogs-secondary-port | dialogs_secondary_window_port 10, _native 11 | Partial -> Partial | production factories, W9 routing, mutation |
| X-dialogs-zoom-diagnostics-profiles-benchmark | zoom 6+2, action_profiles 4+11, benchmark layout 6, results 7, native 4, run_native 7 | Partial -> Partial | language/DPI/GDI, custom-draw, repeated-run scenarios, production codec benchmark, mutation |
| X-dialogs-recovery | layout 4, details 7, actions 3, flow 3, flow_e2e 6, data_safety 7, window_e2e 4 | Partial -> Partial | composition closures (P-43), DPI/GDI/cursor/modal, hand-break checks, mutation |
| X-settings-ui-framework | settings_framework 43, builder_inputs 23, builder_layout 15 | Partial -> Partial | mutation, two goldens not loaded |
| X-settings-ui-pages | diag_perf 6, display 2, files 2, general 12, mouse 1, shortcuts 16 | Partial -> Partial | mutation, per-page goldens not loaded, English Shortcuts names |
| X-settings-ui-window | dialog frame 7, state 9, apply 5, layout 4, e2e 11+2+2+1, external_editors_native 8 | Partial -> Partial | runtime page type labels, DPI/GDI/language, mutation |
| X-startup-main-window | pr-shell lib startup::window 5, startup_run_native 23, startup_wiring_native 13, startup_native 3 | Partial -> Partial | placement_read / close_secondary / arm_smoke_exit are separate work, one machine/DPI, mutation |
| X-startup-overlay-viewmodel | startup_forward_target 3, startup_overlay_vm 3 | Partial -> Partial | mutation, no full-run claim |
| X-startup-options-run | startup_run_native 23, wiring 13, native 3, secondary_dialogs 8, composition_root 13, phase_one 2, instance 1 | Partial -> Partial | W8-09b S4 graph, `shell-startup-texts` golden, process-level run, mutation |
| X-startup-settings-load | pr-app startup_settings_load 7, startup_ui_hop 4, pr-shell startup_phase_one 2 | Partial -> Partial | repair notice display, phase-one integration |
| X-startup-instance | pr-app startup_instance 3, pr-shell startup_instance 1, startup_phase_one 2 | Partial -> Partial | mode-specific unanswered UI, second-process exit-zero scenario |
| X-startup-early-decode | pr-app startup_early_decode 3, pr-shell startup_phase_one 2 | Partial -> Partial | full process scenario |
| U-initial-viewport | native_initial_viewport 2, shell_startup_local_time 1 | Partial -> Partial | expectations from this machine's monitor/DPI only |
| X-w9r8-native-rename-dialog-shell | rename_shell 18 | Partial -> Partial | 144 DPI / vi ink proxy only, composition wiring outside the row |
| X-w9r11-rename-dialog-e2e | rename_prompt_dialog 11, rename_shell_e2e 2 | Partial -> Partial | not the real `ShellStartup` process, structural ink check, no IME/Unicode |

Status counts (`bash tools/dev/parity.sh stats`): before Done 170 / Partial 102, after Done 171 / Partial 101 (363 status rows).

## Not run

- Mutation testing (none of these rows was mutated in this task; earlier mutation notes stay in the rows).
- Hardware GPU (WARP only), other DPI settings, other fonts or languages, attended measurements.
- The C# goldens named as "not loaded" (`input-scripts`, scene golden, `settings-layout`, `settings-builder`, `shell-startup-texts`).
- Inline `host::contracts::tests` and other library unit tests outside `dark_title_bar` and `startup::window`.
- Binaries outside the listed rows (for example `shell_overlay_*`, `dialogs_native_*`, `wic_*`, `ui_smoke_*`, `exe_process`).
