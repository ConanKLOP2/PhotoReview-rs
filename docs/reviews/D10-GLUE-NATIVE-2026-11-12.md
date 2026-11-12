# D10-GLUE-NATIVE: complete mutation runs of the three Settings dialog files (2026-11-12)

Status: evidence report (2026-11-12). Covers `dialog/host.rs`, `dialog/surface.rs` and `dialog/frame.rs` under `crates/pr-platform-win/src/dialogs/settings/`, the three files that [`D10-GLUE-MUT-2026-11-11.md`](D10-GLUE-MUT-2026-11-11.md) left open. With this note every Settings glue file has a mutation verdict. Row D-10 stays Partial (the attended file-picker run is not done). Base: origin/master 5d6cef03. cargo-mutants 27.1.0. One production change: a read-only test seam in `frame.rs` (see "The seam").

## Commands
One lane at a time, own `TEMP`/`TMP` (`mut-d10-c`, `mut-d10-d`), `--jobs 1`, `--budget 40000`, via `tools/dev/mutants.sh pr-platform-win <files> --tests <binaries> --native` (`--native` adds `-- --include-ignored --test-threads=1`, so the `#[ignore = "native: needs desktop"]` tests run on the real desktop). Binaries of the full runs: the 21 pure binaries of D10-GLUE-MUT plus `settings_dialog_{apply,frame,layout,state,import_export}_e2e`, `settings_reentrancy_native`, `settings_reset_page_native`; the second part of the `frame.rs` run also had `d10_host_native_killers` and `d10_surface_native_killers` (preflight 285 tests, ok).

The `frame.rs` run was done in two parts. The first process was stopped from outside after 83 mutants; cargo-mutants then recorded the rest as "Unviable" with a 0.1 s build. Those records were discarded and exactly those positions were run again (`-F` on file:line:col, 114 mutants found because the filter also matches sibling mutants at the same position). The two parts together give 190 distinct identities, each with one valid outcome.

Line numbers below are those of the base revision. The seam adds 9 lines near the top of `frame.rs`, so the confirmation run reports the same mutants 9 lines lower.

## Results of the full runs
| files | mutants | caught | missed | unviable | timeout |
|---|---|---|---|---|---|
| `dialog/host.rs`, `dialog/surface.rs` | 211 | 142 | 60 | 9 | 0 |
| `dialog/frame.rs` | 190 | 111 | 67 | 11 | 1 |

Flake check: every caught log has at least one named failing test. The tests that are the only failing test for many mutants were looked at (`every_frame_case_matches_the_csharp_golden`, `every_apply_case_matches_the_csharp_golden`, `page_combobox_wheel_scrolls_the_page_until_its_dropdown_is_open`, `clicking_the_reset_button_resets_only_the_showing_page`): they compare fixed values (C# goldens, a scroll offset `left: 0 right: 45`, button state) and the mutants they catch are in the code they exercise. The other catch logs were classified by script only.

The misses were real test gaps: with for example `PageSurface::place -> ()` every existing test passed. Most of the missed methods are one-line forwards from `SettingsDialog` or `PageSurface` to the content or the native window, which the existing tests reached through another path.

## New tests
| binary | tests | covers |
|---|---|---|
| `d10_host_native_killers` | 16 | `host.rs` survivors |
| `d10_surface_native_killers` | 9 | `surface.rs` survivors |
| `d10_frame_native_killers` | 8 | `frame.rs` part 1 (`preferred_client_size`, `minimum_client_size`, external-editor focus path) |
| `d10_frame2_content_native_killers` | 10 | `on_ok`, `on_scroll`, `on_dialog_key`, `Debug`, `is_page_created`, `ensure_page_created`, `cycle_page`, `focus_external_editor`, `is_key_box`, `is_key_box_id`, `capture_key`, `close` |
| `d10_frame2_delegates_native_killers` | 12 | `warn`, `post_to_ui`, `refresh_localized_texts`, tooltip setters, `set_control_checked`, `set_combo_selected_index`, slider get/set/range, `set_control_enabled`, `is_control_enabled`, `focus_control`, `relayout_active_page` |
| `d10_frame2_registry_native_killers` | 2 | `forget_subclass_key` (655:9, 656:16) and `Drop for SettingsContent` (670:9), through the seam |

All but one test (`debug_names_the_dialog_type`) need a desktop and carry `#[ignore = "native: needs desktop"]`; default `cargo test` and CI do not run them. Each new binary passed three consecutive runs on the unmutated code. The tests of the three `d10_frame2_*` files were written without hand-applied mutations and were reviewed by a second, read-only agent that tried to refute each claimed kill; none was refuted. The cargo-mutants reruns below are the proof.

## Confirmation reruns (restricted to the survivors, only the new binaries plus `d10_settings_glue_killers`)
| files | found | caught | missed | unviable | timeout |
|---|---|---|---|---|---|
| `host.rs`, `surface.rs` | 80 | 72 | 5 | 3 | 0 |
| `frame.rs` | 79 | 70 | 8 | 0 | 1 |

"Found" is larger than the survivor count because `-F` also matches already-caught siblings at the same position; the `frame.rs` rerun also includes the two mutants of the new seam function (both caught). The misses of the reruns are exactly the 13 mutants argued equivalent below.

The timeout is `frame.rs:818:9` `SettingsDialog::close -> ()`. In the rerun log the test `close_marks_the_dialog_closed_immediately` FAILED on its assertion, and so did six other tests of the same binary; a later test then waited on a modal loop that the mutated `close` can no longer end, until the 400 s limit. The mutant is detected, by a failing assertion and by the hang. It stays counted as a timeout because cargo-mutants reports it so.

## Equivalent (13 identities, argued from the code, NOT run-proven)
- `|` -> `^` on operands with no common bit: `host.rs:58:36` (0x400 and 0x100), `surface.rs:182:20` and `182:29` (SWP flags), `frame.rs:464:46`, `484:26`, `484:40`, `502:36`, `502:42` (window styles in `build_items`).
- `surface.rs:199:91` `&` -> `|` and `&` -> `^`: the only difference is one more `ShowWindow(SW_HIDE)` on a control that is already hidden.
- `frame.rs:593:69`, `594:71` delete `-` in `unwrap_or(-1)`: the fallback needs an index above `i32::MAX`.
- `frame.rs:702:9` `default_button_id -> 1`: `SAVE_ID` is `ID_OK`, which is 1.

## The seam
`frame.rs` gains `#[doc(hidden)] pub fn subclass_registry_len() -> usize`, which returns the number of entries of the calling thread's private `SUBCLASS_REGISTRY`. It only reads. Without it the three cleanup mutants (655:9, 656:16, 670:9) had no observable effect: a stale entry is a dead `Weak` under a key that is never issued again. Recorded as P-405 in [`PROPOSED-DECISIONS.md`](../PROPOSED-DECISIONS.md) (same pattern as P-396, P-400, P-404).

## Verdict for the three files
| | `host.rs` + `surface.rs` (211) | `frame.rs` (190 + 2 seam) |
|---|---|---|
| caught | 197 (142 + 55) | 172 (111 + 59 + 2) |
| unviable | 9 | 11 |
| equivalent (argued) | 5 | 8 |
| timeout (detected, see above) | 0 | 1 |
| missed, NOT RUN | 0 | 0 |

Together with D10-GLUE-MUT the Settings glue has 849 identities: 741 caught, 58 unviable, 49 equivalent by argument, 1 timeout, 0 missed, 0 NOT RUN.

## What this does not show
- The full runs were not repeated on the final test set; the 114 kills (55 + 59) are confirmed by the restricted reruns only.
- The 13 equivalents have no run proof.
- The kills depend on native tests that only run on a machine with a desktop (`tools/dev/native-tests.sh` or the nightly native job). `focus_control_moves_the_focus` and `focus_external_editor_answers_true_and_focuses` also assert the keyboard focus, which can fail on a locked desktop or when the test window cannot be activated; this was not seen in the runs here.
- D-10 stays Partial: the attended file-picker run (import/export through the real dialog) is not done.
- Raw outputs are under the user temp folder (`mut-d10-c/mut-out-pr-platform-win-{tFS7uQ,XT6I7A}`, `mut-d10-d/mut-out-pr-platform-win-{TGyJZ1,vIGjIW,eCx8HB}`), not in the repository.
