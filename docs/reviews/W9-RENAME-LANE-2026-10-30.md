# W9 rename lane evidence (2026-10-30)

Lane `cl-rename`, base `45c89c7b`. Production code and the pinned tests are unchanged; only PARITY rows and this report change. No new killer test was needed (no real survivor), so `crates/pr-app/tests/w9_rename_killers.rs` was not created.

## Mutation (cargo-mutants, `--baseline skip --timeout 90 -j 1`, own `target-mut`, `--cargo-arg=--test=<binary>`)

| File | Test binaries | Tested | Caught | Missed | Unviable | Timeouts |
|---|---|---:|---:|---:|---:|---:|
| `pr-app` `coordinators/zoom_detail.rs`, `-F retarget_path` | `presenter_rename`, `rename_presenter_matrix` | 6 | 5 | 1 | 0 | 0 |
| `pr-app` `input/shortcut_router.rs` (whole) | `shortcut_router`, `rename_file_shortcut_router`, `key_id` | 26 | 24 | 0 | 2 | 0 |
| `pr-app` `menus/builder.rs` (whole) | `menu_model`, `rename_file_menu_model`, `settings_field_map` | 34 | 28 | 0 | 6 | 0 |
| `pr-core` `settings/shortcuts.rs` (whole) | `settings_shortcuts`, `rename_file_settings`, `routing_settings_mutation_killers` | 80 | 79 | 0 | 1 | 0 |
| `pr-core` `settings/context_menu.rs` (whole) | `settings_context_menu`, `rename_file_settings`, `routing_settings_mutation_killers` | 68 | 59 | 0 | 9 | 0 |

Total 214 tested, 195 caught, 1 missed (equivalent), 18 unviable.

Equivalent survivor: `zoom_detail.rs:306:13` delete field `id` in `retarget_path` (the new target then keeps the old id). The old decode's cancel token is cancelled by `cancel_pending`, and `PreviewImageService::decode_original` re-checks the token after the decode (`service/decode.rs` line 125), so a stale result always arrives as `Canceled` and the id guard in `on_original_decoded` is defence in depth that no fake can reach. The existing `retarget_path_decode_in_flight_for_the_old_path_its_pixels_are_never_shown` still guards the observable outcome.

## Desktop (native) re-run, DLLs staged, `--include-ignored --test-threads=1`, isolated data root

`rename_prompt_dialog` 11 passed, `rename_shell` 18 passed, `rename_shell_e2e` 2 passed.

## Not run

Mutation of `rename_prompt` presenter and view model (covered by W9-RENAME-MUTATION-2, not repeated); native glue (`dialogs::rename_prompt`, `main_window`, `shell_menus`); `X-w9r9`/`X-w9r10` (tests only, no production code); `X-settings-presenter` (3 `try_start_explorer` mutants spawn `explorer.exe`, untestable), `X-recovery-window-presenter` and `X-benchmark-presenter` (desktop runs W8-18r..18v and the composition-root codecs W8-17c are human or other-lane gaps; rows unchanged). Real 144 DPI vi ink measurement, IME input and the real `ShellStartup` process run were not done.
