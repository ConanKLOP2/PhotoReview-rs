# CL-MUT-ZOOMCORE 2026-10-31: partial mutation rerun (zoom_detail, main_window/core.rs registry)

Status: **Partial**, time-boxed (~70 min on a heavily loaded machine: 2-5 min per mutant). Flags: `--baseline skip --timeout 300 -j 1`, `--cargo-test-arg=--test --cargo-test-arg=<bin>` and, for the native part, `--cargo-test-arg=-- --cargo-test-arg=--include-ignored --cargo-test-arg=--test-threads=1` (correct form). Own `target-mut` and `mut-out-*` inside the worktree, deleted afterwards. Preflight: `zoom_detail` 66 + `w8_zoom_killers2` 15 + `w8_presenter_mutation_killers` 5 + `viewmodels_main_navigation` 45 tests ran and passed; `mut_zoomcore_killers` ran 2 passed natively (`running 2 tests`); mutant logs show real `running N tests` lines. No golden-only catches (none of these binaries are golden tests).

## Code change (seam, no behaviour change)
`MainWindow::{id, registered_id, current_registered_id}` in `crates/pr-platform-win/src/host/main_window/core.rs`: read-only probes of the UI-thread registry. New native killers: `crates/pr-platform-win/tests/mut_zoomcore_killers.rs` (2 `#[ignore]` tests, two live hidden windows).

## Results per file
| File | Selection | Tested | Caught | Missed | Timeout | Unviable | NOT RUN |
|---|---|---|---|---|---|---|---|
| `pr-app/.../zoom_detail.rs` | 73 of 89 (lines 40, 62, 200-720); first 10 outcomes completed in order, run stopped by owned PID | 10 | 7 | 0 | 0 | 3 (`catch_job -> Default`, `default_indicator_delay -> Default`, `+ -> *`) | 63 of the selection (lines >= 316, incl. `refresh`, `update`, `on_original_decoded`, `on_indicator_elapsed`) and 16 outside it (lines 74-150, historically covered by CX-ZOOM) |
| `pr-platform-win/.../main_window/core.rs` | 17 registry/`attach_renderer` mutants | 13 | 9 | 0 | 0 | 4 (`Some(Default)`/`Some(Rc::new(Default))` forms) | `register_window > -> >=` , `unregister_window` x2, `prefer_warp` deletion (+ the other ~117 core.rs mutants) |
| `swap_chain_clock.rs`, `message_loop.rs`, `host::contracts` inline tests | - | 0 | - | - | - | - | **CHƯA CHẠY** (time ran out; mutation slot used by the two files above). `message_loop.rs` still needs a fail-fast killer on the test thread for the hang mutants (see `w8_host3_killers.rs`, `w8_hostrest_killers.rs`). |

Caught in `core.rs` (all 5 earlier MISSED now die): `with_window -> None`, `with_current_window -> None`, its `delete !`, `find_live -> None`, `find_live == -> !=`, `newest_live -> None`, `register_window -> ()`, `register_window > -> ==` and `> -> <`. Caught in `zoom_detail.rs`: `ThreadRunner::run -> ()`, `+ -> -` in `default_indicator_delay`, Debug fmt, `retarget_path` body and id/token field deletions. The `catch_job` (PR #482) `-> Default` mutant is unviable (no `Default` bound); its panic path is not mutation-proven here.

## PARITY proposals (not edited here)
- X-main-window-core (L629): keep Partial; add "rerun OK 2026-10-31: registry 9 caught + 4 unviable, the 5 earlier MISSED killed via `MainWindow::registered_id`"; remaining: `>= `, unregister, `prefer_warp`, ~117 other mutants.
- X-zoom-detail-loader (L576): keep Partial; "rerun OK 2026-10-31 for 10 mutants (7 caught, 3 unviable)"; 63 current mutants still not rerun.
- X-host-swap-chain-clock / X-host-shell-contracts / X-host-icons (L598), X-host-message-loop (L599): unchanged, not run in this lane.
