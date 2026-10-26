# Wave 9 plan, lane E: external editors UI (FEAT-EXTERNAL-EDITORS)

> **Status: active unit table; lane E is implemented, all of W9-E1..E9 are on master.** Last verified 2026-10-26 against `origin/master` 8b93fbf.
> Evidence (`git log origin/master`): E1 #233, E3 #234, E2 #235, E7 #236, E4 #237, E6 #238, E5 #239, E8 #254, E9 #256. The "Survey" and "still stubs" sections below are the snapshot of 2026-10-23 (master f795e18) and describe the state before those PRs. Whether native and attended evidence exists for each unit is tracked in `docs/PARITY.md`, not claimed here. Combined view: [WAVE-9-PLAN.md](WAVE-9-PLAN.md).

Baseline: C# `7eaee2a2`; feature commit `d9623945` (`git -C C:\MyProjects\PhotoReview diff d92d6aae 7eaee2a2`). The lane covers only what that change adds for external editors. FEAT-RENAME (F2, `RenameFile`, `RenamePromptDialog`, `FileActionController` rename, ...) is another lane and is excluded; the places where both lanes edit the same file are listed under "Overlap".
Reviewed against the C# source and the merged Rust crates; the first draft's defects and their fixes are recorded in each `contracts/W9-E*.md` ("Review fixes").

## Survey (tái kiểm trên master f795e18, 2026-10-23; implementation vẫn chưa chạy thử)

> Snapshot, superseded: the "Rust status" column below is as of master f795e18 (2026-10-23). Since then W9-E1..E9 merged (#233 to #256), so no row is still "missing" or "stub".

| C# member / delta | Rust status | Unit |
|---|---|---|
| `ExternalEditor`, `ExternalEditors` (`Core/Settings/ExternalEditor.cs`, 283 lines) | **stub rules**: `pr_core::settings::external_editors` types/signatures exist; function bodies remain todo | W9-E1 |
| `AppSettings.ExternalEditors`, `DefaultExternalEditorId`, `ExternalEditorSendsRawOriginal`, `CurrentConfigVersion` 4 | **merged #182**: version, fields and JSON read/write exist; rule bodies remain E1 | W9-E1 |
| `SettingsNormalizer` reconcile + trim, `SettingsStore` save + v3 -> v4 migration | **missing** | W9-E1 |
| `ShortcutMappings.OpenInDefaultEditor` (+ `OptionalNames`, validator, normalizer) | **partial**: field, OPTIONAL_NAMES and generic trim exist; reconcile/store migration remain E1 | W9-E1 (moved from E7) |
| `IDialogService.PromptAddExternalEditor` / `PromptDefaultExternalEditor` | **declared**: two default None trait methods; production forwarding tracked by E8 and review M1 | W9-E1 (moved from E8) |
| `ExternalEditorsEdit`, `ExternalEditorsDialog` | **missing** (`pr-platform-win`, not `pr-core`: internal to `Shell.Win32` in C#) | W9-E2 |
| `ExternalEditorProductName`, `TextPromptDialog`, `ListDialog` choice list (`ForChoice`, `InitialSelectedIndex`, `SelectedIndex`) | **missing** (stub adds the `ListDialogContent` field and `for_choice`) | W9-E3 |
| `SettingsFieldMap` four-property editor row + shortcut accessor, `SettingsFormState`, `SettingsPresenter.Defaults`, `FilesSettingsPage` group, `ShortcutsSettingsPage` row, `SettingsPageContext.EditExternalEditors`, `SettingsDialogOptions.ShowExternalEditors` | **partial**: field map/form/defaults/shortcut rows exist (#182); pages files/shortcuts implemented for old behavior; editor row semantics need E4; dialog/frame and host remain stubs | W9-E4 |
| `MainViewModel` default/by-id launch, add, choose default; launcher seam with a command line | **partial**: `open_in_external_editor`, `can_open_in_external_editor`, `ExternalEditorLauncher` exist with legacy-path behavior; launcher seam already takes 3 arguments (exe, file, command line) | W9-E5 |
| `ContextMenuModelBuilder.ExternalEditorItem` | **partial**: the `ExternalEditor` arm is a single command | W9-E6 |
| `ReviewCommand.OpenInDefaultEditor`, `ShortcutRouter` step 12f, `IgnoresAutoRepeat` | **partial**: enum 30 and ignores-auto-repeat exist; dedicated binding/router step remain E7 | W9-E7 |
| `ISecondaryWindowPort` three members, `SecondaryWindowKind.ExternalEditors` (variant exists), `Win32DialogService` forwarding, `Win32SecondaryWindowPort`/`SecondaryWindowRouter` | **partial**: variant and service forwarding in the stub; `secondary_window_port.rs` is a W8-18o `todo!()` stub | W9-E8 |
| `ContextMenuController` ids and arms, `ShellMainWindow.Menus`, `ShellMainWindow.Input` | **missing** (`ContextMenuActions` members, constants and the `OpenInDefaultEditor` arm are in the stub) | W9-E9 |
| en/vi catalog keys (34 at `7eaee2a2`, about 29 for this lane) | **synced #182** in en/vi; no asset edits needed here | none |

## Units

Order: E1 first; then E2, E3, E5, E6, E7 in parallel; then E4, E8, E9. Models: Sonnet for all (E5 is 🔴: manual review of the launcher seam and the settings mutation).

| Unit | C# files and exact lines at `7eaee2a2` | Rust | Col | Test | Depends on (plan edge; code edge checked by grep) |
|---|---|---|---|---|---|
| W9-E1 | `Core/Settings/ExternalEditor.cs` 1-283; `AppSettings.cs` 9, 255-276; `SettingsNormalizer.cs` 147, 253; `SettingsStore.cs` 249, 499-503; `ShortcutMappings.cs` 83-89, 101-104; `IDialogService.cs` 50-60 | `pr_core::settings::{external_editors, app_settings, normalizer, store, shortcuts}`, `abstractions::dialog` | 🟡 | G + F (oracle `external-editor-rules`, 77 cases) | none; merged `settings::*`, `io::physical::new_guid_n`, `catalog::types::RAW_EXTENSIONS`, `text::dotnet_path` |
| W9-E2 | `Shell.Win32/Dialogs/Native/ExternalEditorsDialog.cs` 1-408 | `pr_platform_win::dialogs::native::external_editors_dialog` | 🟡 | N (+ F helpers) | W8-18h..k; **W8-18p** (`DialogMeasure`), **W8-18q** (`DialogWarningSink`, `FileFilterText`), merged `ShellFileDialog`, `native::dialog`; E1 (types) |
| W9-E3 | `ExternalEditorProductName.cs` 1-34; `TextPromptDialog.cs` 1-99; `ListDialog.cs` 24-25, 39-50, 79-80, 154-165 | `pr_platform_win::dialogs::native::{external_editor_product_name, text_prompt_dialog, list_dialog}` | 🟡 | F + N | W8-18h..k; **W8-18p**; merged `native::list_dialog` (W8-18e) |
| W9-E4 | `SettingsFieldMap.cs` 61, 186-206; `SettingsFormState.cs` 48-58; `SettingsPresenter.cs` 298-301; `FilesSettingsPage.cs` 33-37, 54, 106-110, 147-151, 166-170, 177-225, 277-281; `ShortcutsSettingsPage.cs` 70-71; `SettingsPageContext.cs` 164-186; `SettingsDialog.cs` 54-58 | `pr_app::presenters::{settings_field_map, settings_form_state, settings::load}`, `pr_platform_win::dialogs::settings::{context, dialog::{frame, host}, pages::{files, shortcuts}}` | 🟡 | F + N | E1, E2; **W8-19f**, **W8-19i** (merged), **W8-19j..m** (dialog stubs: edit after they merge); merged W8-19a, W8-10* |
| W9-E5 | `App.Shared/ViewModels/MainViewModel.cs` 774-881 (+ the tests that move to `FakeEditorExe`) | `pr_app::viewmodels::main::commands` | 🔴 | F | E1 (types, `DialogService` methods); merged `SettingsSource::{begin_edit, commit_edit}`, `ReviewCatalog::find` |
| W9-E6 | `App.Shared/Menus/ContextMenuModelBuilder.cs` 75, 84-122 | `pr_app::menus::{builder, external_editors}` | 🟡 | G (oracle `external-editor-menu`, 18 cases) | E1; merged `menus::{builder, model}`; catalog sync for the texts |
| W9-E7 | `Input/ReviewCommand.cs` 38, 61, 101; `Input/ShortcutRouter.cs` 38, 84, 278-282 | `pr_app::input::{review_command, shortcut_router}` | 🟡 | G (oracle `open-in-default-editor-shortcut`, 13 scenarios + the auto-repeat table) | E1 (the shortcut field); merged `input::shortcut_router` |
| W9-E8 | `Win32DialogService.cs` 40-50, 69-71, 165-169; `Win32SecondaryWindowPort.cs` 82-85, 107-110, 242-295, 428-436 | `pr_platform_win::dialogs::{dialog_service, secondary_window_port}` | 🟡 | F | E1, E2, E3, E4 (options field); **W8-18o** (stub: edit after it merges); merged W8-18b |
| W9-E9 | `Menus/ContextMenuController.cs` 57-69, 92-95, 179-182; `Menus/ShellMainWindow.Menus.cs` 71-75; `Input/ShellMainWindow.Input.cs` 215-217 | `pr_platform_win::{shell_menus::{controller, main_window}, shell_input::main_window}` | 🟡 | F + N | E5, E6, E7; merged W8-14c/14e/14f. **Not** E8 (the shell reaches the dialogs through the view model) |

## Oracle commands and goldens (built and run against `7eaee2a2` while writing the contracts)

| Command | File | Golden | Cases |
|---|---|---|---|
| `external-editor-rules` | `tools/oracle-cs/Commands/W9-E1.cs` | `tests/golden/external-editor-rules.json` | 77 (constants, name, display name, arguments, reconcile, apply-legacy, effective/default/find, RAW pairing) |
| `external-editor-menu` | `W9-E6.cs` | `tests/golden/external-editor-menu.json` | 18 |
| `open-in-default-editor-shortcut` | `W9-E7.cs` | `tests/golden/open-in-default-editor-shortcut.json` | 13 scenarios + the `IgnoresAutoRepeat` and enum value table |
The dispatch lines are in `tools/oracle-cs/Program.cs`. Deleted from the first draft: `W9-E3.cs`, `W9-E5.cs` (empty bodies), and their inputs. The existing goldens `settings-json`, `settings-normalize`, `settings-parse-text`, `settings-store`, `settings-field-map`, `settings-validate`, `shortcut-canonical`, `settings-page-files`, `settings-page-shortcuts`, `secondary-window-supports` change at `7eaee2a2` and are regenerated by the lead when the baseline moves (not done here).

## Overlap with the FEAT-RENAME lane (same files, disjoint lines; the second lane to merge rebases)
`pr-core::settings::{app_settings, shortcuts, normalizer, store}` (RenameFile shortcut, F2 default, the same version 4), `pr-app::input::{review_command, shortcut_router}` (enum value 31, step 9b), `pr-app::menus::{builder, model}`, `pr-app::presenters::settings_field_map` (shortcut list, context menu rows), `shell_menus::controller`, `shell_input::main_window`, `secondary_window_port.rs` (`RenamePrompt`), `ShortcutsSettingsPage` rows. Counts that only hold after both lanes: 29 shortcut boxes, 16 optional shortcuts, 8 action rows, 19 context menu ticks.

## Dependencies on W8 units that are still stubs

> Snapshot, superseded: W8-18o (#231), W8-18p (#226), W8-18q (#227) and W8-19j..19m (#230) are on master now; no lane E dependency on a Wave 8 unit is open.

W8-18o (`secondary_window_port.rs`), W8-18p (`zoom_prompt.rs`: layout vocabulary), W8-18q (`action_profiles.rs`: `DialogWarningSink`, `FileFilterText`), W8-19f (`pages/files.rs`) and W8-19i (`pages/shortcuts.rs`) are already implemented; remaining dialog dependencies: W8-19j..19m (`dialog/frame.rs`, `dialog/host.rs`), W8-17c (the production `SecondaryWindowDialogs` adapter: P-43). E2, E3, E4 and E8 are written against their signatures and must be ported after them.

Historical oracle generation is recorded in the sync ledger. This documentation recheck reads committed metadata only; no oracle, GUI/native test or Rust test was run.
