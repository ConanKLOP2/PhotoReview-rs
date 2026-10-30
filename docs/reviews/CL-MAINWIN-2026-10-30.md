# CL-MAINWIN 2026-10-30: main-window mutation killers (lane cl-mainwin)

Status: partial. No PARITY row `X-main-window-*` changes status: every row keeps Partial (rows were not edited).

Killers: `crates/pr-platform-win/tests/w8_mainwin_killers.rs` (6 native `#[ignore]` tests on a hidden, never shown shell window, all pass with
`--include-ignored --test-threads=1`) and `crates/pr-platform-win/src/host/main_window/core_registry_tests.rs` (4 in-crate unit tests, included from
`core.rs` with `#[cfg(test)] #[path]`). No production code changed.

## Mutation results (cargo-mutants, `--baseline skip --timeout 90 -j 1`)

| Scope | Tested | Caught | Missed | Note |
|---|---|---|---|---|
| `core.rs` registry (`register_window`, `unregister_window`, `with_window`, `with_current_window`, `find_live`, `newest_live`), lib tests | 17 | 8 | 5 | 4 unviable; missed = `with_window -> None`, `with_current_window -> None` and its `delete !`, `register_window` `>` -> `<`, `attach_renderer` `prefer_warp` field deletion: each needs a LIVE `MainWindowInner` (a real window), no seam in the lib test binary (native gap) |
| `lifecycle.rs` `InnerSteps` + `ClearOnDrop`, killers + lifecycle + viewing + windowing binaries | 15 | 12 | 3 | before the killers 11 `InnerSteps` mutants survived; missed: `on_navigation_changed -> ()` (prefetcher `navigated`, no observer), `on_compare_changed -> ()` (`update_retained` and `window.invalidate`, no observer) and `on_viewer_changed -> ()` (killed in the next run by `a_viewer_change_queues_a_layout_pass`) |
| `lifecycle.rs` `on_viewer_changed`, `update_fit_size_if_alive`, `is_minimized`, `restore`, close waiter, killers + lifecycle | 14 | 9 | 5 | missed: `is_minimized -> false`, `is_minimized !=`, `restore -> ()`, `restore !=` (minimise/restore shows a window: native gap, not run) and `close_when_file_action_done_waiter &&` -> `||` (line 484, not analysed) |

Killed by the new tests: `viewer_is_fit` true/false and `post_update_fit_size` (fit update posted only in Fit), `notify_current_index`, `is_disposed -> false`,
`ClearOnDrop::drop`, `on_viewer_changed`, `register_window` retain, `unregister_window`, `register_window -> ()`.

## Not run / open

- `core.rs` other than the registry, `frame.rs`, `scrollbars.rs`, `viewing.rs`, `windowing.rs`: no new mutation pass (time-box); their PARITY debts are unchanged.
- The first full `lifecycle.rs` run (81 mutants) was stopped after 13 outcomes to free the mutation slot; only the survivors were re-run.
- Native gaps unchanged: minimise/restore, F11 placement, zero client size, device-lost crossfade, real multi-monitor, bar end-to-end tests.
