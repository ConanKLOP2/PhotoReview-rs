# ST-SETTINGS-PRESENTER - X-settings-presenter gap (2026-10-31)

Status: lane done. Source: item 6 of [ST-GAPMAP2](ST-GAPMAP2-2026-10-31.md).

## Finding

The two behaviours listed as missing already exist in production code; the PARITY row text is stale.

| Gap in the row | Real state | Evidence |
|---|---|---|
| `export_translation` needs `export_todo` | Ported: `SettingsPresenter::export_translation` calls `LocalizationService::export_todo` (`crates/pr-app/src/localization/service.rs`), returns `/select,"<path>"` or the error text | `settings_presenter_apply.rs`: `export_translation_without_service_is_none_none`, `export_translation_with_service_selects_the_written_file`; no `#[ignore]` remains in that file |
| numeric `Operation` refused | Accepted (0 Move, 1 Copy, 2 Recycle) in `action_list_json.rs` and in the Actions row of `settings_field_map.rs` (test `actions_row_accepts_numeric_operation`) | the Save tests still used `"Move"` with a stale DIVERGENCE note |

## Change (tests only, no production change)

`crates/pr-app/tests/settings_presenter_save.rs`:
- `save_actions_json_is_validated` and `save_action_destination_problem_is_refused` now use `"Operation":0` exactly like the C# tests `Save_ActionsJson_IsValidated` / `Save_ActionDestinationProblem_IsRefused`; the DIVERGENCE note is removed.
- New `save_actions_json_numeric_operations_map_to_move_copy_recycle`: 1 -> Copy, 2 -> Recycle, `"Delete"` -> Recycle, undefined number 9 is refused with `dialog.settingsInvalid.actionsJson`.

Run: `cargo test -p pr-app --test settings_presenter_save -j 2`: 36 passed, 0 failed. `cargo fmt -p pr-app` applied.

## Still open (not in this lane)
`SettingsTexts` / `ShellIntegration` registry / HTTP transport seams (the registry seam was closed by `X-platform-adapters`; Texts and HTTP stay with the human-only rows D-10, X-http-composition).

## Suggested PARITY edits (orchestrator)
Row `X-settings-presenter`: delete "`export_translation` ... returns a fixed English error text until `export_todo` exists (contract O1)", delete "(+1 ignored: needs W8-04a `export_todo`)" and "10b Actions row refuses a numeric `Operation`"; add `settings_presenter_save` 27 incl. numeric Operation. Keep Partial (native dialog run and seams remain).
